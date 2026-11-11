//! Development quicksave controls. Player menus use the same checkpoint and store.
mod fixture;
mod menu;
mod menu_probe;
mod overworld_test;
mod probe;
mod replay;
pub use fixture::prepare_checkpoint_fixture;
pub use overworld_test::{prepare_overworld_test_fixture, run_overworld_field_probe};
#[cfg(test)]
pub(crate) use replay::assert_checkpoint;
pub use replay::{
    CheckpointRecordingOptions, CheckpointReplay, record_battle, record_checkpoint,
    record_checkpoint_window, record_new_game,
};
pub(crate) use replay::{
    Event as ScenarioEvent, ScenarioInput, Step as ScenarioStep, capture_image,
    install_benchmark_replay, record_app, recording_scene, scenario_consumed,
};
pub(super) mod title;
mod title_probe;
use super::{field_view, loading, new_game};
use anyhow::{Context as ContextExt, Result, ensure};
use bevy::prelude::*;
pub use menu_probe::run_menu_probe;
pub use probe::run_quicksave_probe;
use resonance_game::field::FieldCheckpoint;
use resonance_persistence::{Header, Kind, SlotId, Store, WriteTask};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
pub use title_probe::run_title_load_probe;

pub(super) use resonance_game::Checkpoint as SceneCheckpoint;

#[derive(Resource)]
pub(super) struct Quickload(loading::Pending);

#[derive(Default)]
pub struct SaveOptions {
    pub directory: Option<PathBuf>,
    pub quick_slot: Option<String>,
    pub load: Option<PathBuf>,
}

#[derive(Resource)]
struct Persistence {
    store: Store,
    slot: SlotId,
    writing: Mutex<Option<WriteTask>>,
}

impl Persistence {
    fn is_writing(&self) -> bool {
        self.writing.lock().unwrap().is_some()
    }
    fn poll_write(&self) -> Option<Result<()>> {
        let mut writing = self.writing.lock().unwrap();
        let result = writing.as_ref()?.poll()?;
        writing.take();
        Some(result)
    }
}

impl Drop for Persistence {
    fn drop(&mut self) {
        if let Some(task) = self.writing.get_mut().unwrap().take()
            && let Err(error) = task.wait()
        {
            error!("Save did not complete: {error:#}");
        }
    }
}

pub(super) fn install(app: &mut App, options: &SaveOptions, user_config: bool) -> Result<()> {
    let directory = options
        .directory
        .clone()
        .map_or_else(resonance_persistence::default_directory, Ok)?;
    let slot = SlotId::new(options.quick_slot.as_deref().unwrap_or("quick"))?;
    info!(
        "Save directory: {}; quick slot: {}; initial save: {:?}",
        directory.display(),
        slot.as_str(),
        options.load
    );
    if let Some(path) = &options.load {
        app.insert_resource(new_game::Request(Some(
            resonance_persistence::read_bounded(path)
                .with_context(|| format!("reading startup save {}", path.display()))?,
        )));
    }
    if user_config {
        app.insert_resource(crate::config::Preferences::load(
            directory.join("config.toml"),
        )?);
    }
    app.insert_resource(Persistence {
        store: Store::new(directory),
        slot,
        writing: Mutex::default(),
    });
    title::install(app);
    Ok(())
}

/// Only files addressable through Continue affect the title default.
pub(super) fn title_selection(world: &World) -> usize {
    match menu::has_save_data(&world.resource::<Persistence>().store) {
        Ok(saved) => usize::from(saved),
        Err(error) => {
            warn!("Cannot check title save data: {error:#}");
            0
        }
    }
}

/// Field diagnostics use the same admission as a normal scene save.
pub(super) fn checkpoint(world: &mut World) -> Result<FieldCheckpoint> {
    match scene_checkpoint(world)? {
        SceneCheckpoint::Field(checkpoint) => Ok(checkpoint),
        SceneCheckpoint::World(_) => anyhow::bail!("checkpoint requires an active field"),
    }
}

fn scene_checkpoint(world: &mut World) -> Result<SceneCheckpoint> {
    ensure!(
        !world.resource::<Time<Virtual>>().is_paused(),
        "quicksave unavailable while the game is paused"
    );
    ensure!(
        scene_prepared(world),
        "quicksave unavailable while preparing the scene"
    );
    ensure!(
        !world.resource::<super::movie::Playback>().active,
        "quicksave unavailable during a movie"
    );
    let session = world
        .get_resource::<new_game::Session>()
        .context("quicksave requires an active scene")?;
    ensure!(
        session.audio.is_none(),
        "quicksave unavailable during audio preparation"
    );
    match &session.scene {
        new_game::Scene::Field(field) => {
            ensure!(
                session.ready_for_field,
                "quicksave unavailable during a scene presentation"
            );
            field.session.checkpoint().map(SceneCheckpoint::Field)
        }
        new_game::Scene::World(scene) => scene.session.checkpoint().map(SceneCheckpoint::World),
    }
}

pub(crate) fn scene_prepared(world: &mut World) -> bool {
    !world.contains_resource::<loading::Pending>()
        && !world.contains_resource::<loading::FieldPending>()
        && !world.contains_resource::<loading::WorldPending>()
        && !world.contains_resource::<Quickload>()
        && world
            .resource::<loading::Resident>()
            .active
            .load(std::sync::atomic::Ordering::Acquire)
        && new_game::scene_ready(world)
}

fn save(world: &mut World) -> Result<String> {
    let started = Instant::now();
    let state = scene_checkpoint(world)?;
    let persistence = world.resource::<Persistence>();
    let mut writing = persistence.writing.lock().unwrap();
    ensure!(writing.is_none(), "previous save is still being written");
    let header = Header {
        version: resonance_persistence::VERSION,
        label: format!("Quicksave {}", persistence.slot.as_str()),
        location: state.location(),
        played_ticks: state.played_ticks(),
        saved_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    };
    let bytes = resonance_persistence::encode(&header, &state)?;
    let size = bytes.len();
    *writing = Some(persistence.store.write_async(
        Kind::Quicksave,
        persistence.slot.clone(),
        bytes,
    )?);
    Ok(format!(
        "Writing quicksave {} ({size} bytes, capture {:.2} ms)",
        persistence.slot.as_str(),
        started.elapsed().as_secs_f64() * 1000.
    ))
}

fn load(world: &mut World) -> Result<String> {
    let recovering = world.contains_resource::<new_game::TransitionFailure>();
    if !recovering {
        scene_checkpoint(world)?;
    }
    let persistence = world.resource::<Persistence>();
    ensure!(
        !persistence.is_writing(),
        "quicksave is still being written"
    );
    let bytes = persistence.store.read(Kind::Quicksave, &persistence.slot)?;
    let pending = loading::Pending::start(
        world.resource::<super::RunOptions>().assets.clone(),
        world.resource::<super::RunOptions>().script_root.clone(),
        Some(bytes),
        None,
        world.resource::<loading::Resident>(),
    )?;
    world.insert_resource(Quickload(pending));
    Ok("Loading quicksave".into())
}

pub(super) fn reset_scene(world: &mut World) {
    crate::config::restore(world);
    world.remove_resource::<new_game::TransitionFailure>();
    let files = world.resource::<new_game::Session>().files();
    *world.resource::<loading::Resident>().files.write().unwrap() = Some(files);
    // Resetting actors creates fresh material bindings even on the same map.
    // Retain prepared assets, but require a new opening-frame GPU submission.
    world
        .resource::<loading::Resident>()
        .active
        .store(false, std::sync::atomic::Ordering::Release);
    field_view::reset_live(world);
    super::field_audio::retire(world);
}

/// Every save loader commits through the same scene installation.
pub(crate) fn install_loaded(world: &mut World, candidate: new_game::Session) {
    crate::game_over::loaded(world);
    if let Some(mut session) = world.get_resource_mut::<new_game::Session>() {
        session.replace_loaded(candidate);
        reset_scene(world);
    } else {
        new_game::activate(world, candidate);
        field_view::reset_live(world);
    }
    new_game::scene_fade::loaded(world);
}

pub(super) fn update(world: &mut World) {
    crate::config::update(world);
    if let Some(pending) = world.get_resource::<Quickload>() {
        let result = match pending.0.poll() {
            Ok(None) => return,
            Ok(Some(result)) => result,
            Err(error) => Err(error),
        };
        world.remove_resource::<Quickload>();
        match result {
            Ok(candidate) => {
                install_loaded(world, candidate);
                report(world, Ok("Quicksave loaded".into()));
            }
            Err(error) => report(world, Err(error.context("Quicksave preparation failed"))),
        }
    }
    menu::update(world);
    if let Some(result) = world.resource::<Persistence>().poll_write() {
        report(world, result.map(|()| "Quicksave written".into()));
    }
    if !world.contains_resource::<ScenarioInput>() {
        shortcuts(world);
    }
}

pub(super) fn shortcuts(world: &mut World) {
    let input = world.resource::<ButtonInput<KeyCode>>();
    let save_pressed = input.just_pressed(KeyCode::F5);
    let load_pressed = input.just_pressed(KeyCode::F9);
    if save_pressed || load_pressed {
        let result = if save_pressed {
            save(world)
        } else {
            load(world)
        };
        report(world, result);
    }
}

pub(crate) fn report(world: &mut World, result: Result<String>) {
    if result.is_err() {
        super::diagnostics::record(world, true);
    }
    let message = match result {
        Ok(message) => {
            info!("{message}");
            message
        }
        Err(error) => {
            warn!("{error:#}");
            format!("{error:#}")
        }
    };
    for mut window in world.query::<&mut Window>().iter_mut(world) {
        window.title = format!("Resonance — {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_quicksave_does_not_queue_a_write() {
        let mut world = World::new();
        world.init_resource::<Time<Virtual>>();
        world.init_resource::<loading::Resident>();
        world.insert_resource(Persistence {
            store: Store::new(std::env::temp_dir()),
            slot: SlotId::new("unused").unwrap(),
            writing: Mutex::default(),
        });
        world.resource_mut::<Time<Virtual>>().pause();
        assert!(save(&mut world).is_err());
        world.resource_mut::<Time<Virtual>>().unpause();
        assert!(save(&mut world).is_err());
        assert!(!world.resource::<Persistence>().is_writing());
        assert!(world.resource::<Persistence>().poll_write().is_none());
    }
}
