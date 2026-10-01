//! Development quicksave controls. Player menus use the same checkpoint and store.
mod fixture;
mod menu;
mod menu_probe;
mod overworld_test;
mod probe;
mod replay;
#[cfg(test)]
use crate::test_support::field_checkpoint;
pub use fixture::prepare_checkpoint_fixture;
pub use overworld_test::{prepare_overworld_test_fixture, run_overworld_field_probe};
#[cfg(test)]
pub(crate) use replay::assert_checkpoint;
pub use replay::{
    CheckpointRecordingOptions, CheckpointReplay, record_checkpoint, record_new_game,
};
pub(crate) use replay::{
    Event as ScenarioEvent, ScenarioInput, Step as ScenarioStep, capture_image,
    install_scenario_input, record_app, recording_scene, scenario_consumed,
};
pub(super) mod title;
mod title_probe;
use super::{field_view, loading, new_game};
use anyhow::{Context, Result, ensure};
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

#[derive(Resource)]
struct RetainedFrame(Vec<(Entity, bool)>);
impl Drop for Persistence {
    fn drop(&mut self) {
        if let Some(task) = self.writing.get_mut().unwrap().take()
            && let Err(error) = task.wait()
        {
            error!("Save did not complete: {error:#}");
        }
    }
}

pub(super) fn install(app: &mut App, options: &SaveOptions) -> Result<()> {
    let directory = options
        .directory
        .clone()
        .map_or_else(resonance_persistence::default_directory, Ok)?;
    let slot = SlotId::new(options.quick_slot.as_deref().unwrap_or("quick"))?;
    if let Some(path) = &options.load {
        app.insert_resource(new_game::Request(Some(
            resonance_persistence::read_bounded(path)?,
        )));
    }
    app.insert_resource(Persistence {
        store: Store::new(directory),
        slot,
        writing: Mutex::default(),
    });
    title::install(app);
    Ok(())
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

fn scene_prepared(world: &mut World) -> bool {
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
        identity: world.resource::<new_game::Session>().identity.clone(),
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

pub(super) fn reset_scene(world: &mut World, changing_field: bool) {
    if let Some(mut menu) = world.get_resource_mut::<super::dungeons::Menu>() {
        menu.testing = false;
    }
    world.remove_resource::<new_game::TransitionFailure>();
    let files = world.resource::<new_game::Session>().files();
    *world.resource::<loading::Resident>().files.write().unwrap() = Some(files);
    if changing_field {
        world
            .resource::<loading::Resident>()
            .active
            .store(false, std::sync::atomic::Ordering::Release);
    }
    field_view::reset_live(world);
    super::field_audio::retire(world);
}

fn restored(world: &mut World, changing_field: bool) {
    reset_scene(world, changing_field);
    // Keep the last field image until its new actor instances are prepared.
    // Output compositing continues; only cameras writing the scene image pause.
    let source = world.resource::<super::display::Targets>().source.id();
    let cameras = world.query::<(Entity, &mut Camera, &bevy::camera::RenderTarget)>()
        .iter_mut(world).filter_map(|(entity, mut camera, target)| {
            if !matches!(target, bevy::camera::RenderTarget::Image(image) if image.handle.id() == source) { return None; }
            let active = camera.is_active;
            camera.is_active = false;
            Some((entity, active))
        }).collect();
    world.insert_resource(RetainedFrame(cameras));
}

pub(super) fn release_frame(world: &mut World) {
    if !world.contains_resource::<RetainedFrame>()
        || !scene_prepared(world)
        || world
            .get_resource::<new_game::Session>()
            .is_none_or(|session| {
                (session.is_field() && !session.ready_for_field) || session.audio.is_some()
            })
    {
        return;
    }
    release_retained_frame(world);
}

pub(super) fn release_retained_frame(world: &mut World) {
    let Some(frame) = world.remove_resource::<RetainedFrame>() else {
        return;
    };
    for (entity, active) in frame.0 {
        if let Some(mut camera) = world.get_mut::<Camera>(entity) {
            camera.is_active = active;
        }
    }
}

pub(super) fn update(world: &mut World) {
    if let Some(pending) = world.get_resource::<Quickload>() {
        let result = match pending.0.poll() {
            Ok(None) => return,
            Ok(Some(result)) => result,
            Err(error) => Err(error),
        };
        world.remove_resource::<Quickload>();
        match result {
            Ok(candidate) => {
                world
                    .resource_mut::<new_game::Session>()
                    .replace_loaded(candidate);
                restored(world, true);
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

fn report(world: &mut World, result: Result<String>) {
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

    #[test]
    #[ignore = "requires current prepared fields; no window or audio device"]
    fn restored_frame_releases_during_authored_notice_without_enabling_saves() {
        use std::{fs, path::Path, sync::atomic::Ordering};
        let scripts = tempfile::tempdir().unwrap();
        fs::write(
            scripts.path().join("fields.json"),
            r#"{"332":{"module":"entry","task":"run","on":"entry"}}"#,
        )
        .unwrap();
        fs::write(
            scripts.path().join("entry.sym"),
            "script field; use game::field; pub task run() { await field::notice(\"Ready.\"); }",
        )
        .unwrap();
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            PathBuf::from,
        );
        let mut cache = loading::Cache {
            scripts: Some(resonance_game::authored::FieldScripts::new(
                scripts.path().to_path_buf(),
            )),
            ..default()
        };
        let package = new_game::FieldPackage::prepare(&root, 332, &mut cache, || false).unwrap();
        let saved = field_checkpoint(&package.files).unwrap();
        let mut session =
            new_game::Session::load_prepared(&root, package.files, Some(saved), None, &mut cache)
                .unwrap();
        session.audio = None;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_resource::<loading::Resident>()
            .init_resource::<super::super::movie::Playback>()
            .insert_resource(crate::diagnostics::Diagnostics(
                resonance_content::diagnostics::Diagnostics::new(true),
            ))
            .insert_resource(Persistence {
                store: Store::new(scripts.path().join("slots")),
                slot: SlotId::new("unused").unwrap(),
                writing: Mutex::default(),
            });
        let art = field_view::prepared_test_art(
            session.field_assets(),
            app.world().resource::<AssetServer>(),
        );
        app.insert_resource(art).insert_resource(session);
        let world = app.world_mut();
        let visible = world
            .spawn(Camera {
                is_active: false,
                ..default()
            })
            .id();
        let hidden = world
            .spawn(Camera {
                is_active: false,
                ..default()
            })
            .id();
        world.insert_resource(RetainedFrame(vec![(visible, true), (hidden, false)]));

        release_frame(world);
        assert!(world.contains_resource::<RetainedFrame>());
        world
            .resource::<loading::Resident>()
            .active
            .store(true, Ordering::Release);
        world.resource_mut::<field_view::Art>().ready = false;
        release_frame(world);
        assert!(world.contains_resource::<RetainedFrame>());
        world.resource_mut::<field_view::Art>().ready = true;
        assert!(checkpoint(world).is_err(), "queued entry must block saving");
        release_frame(world);
        assert!(!world.contains_resource::<RetainedFrame>());
        assert!(world.get::<Camera>(visible).unwrap().is_active);
        assert!(!world.get::<Camera>(hidden).unwrap().is_active);
        assert!(checkpoint(world).is_err());

        let published = world
            .resource::<new_game::Session>()
            .restored_checkpoint
            .clone()
            .unwrap();
        let tick = world.resource::<new_game::Session>().field().events.tick();
        let mut timed_out = false;
        replay::wait_ready(&mut app, &mut timed_out).unwrap();
        assert!(
            !timed_out,
            "a queued Restore event must not block replay admission"
        );
        let world = app.world_mut();
        assert_eq!(
            world.resource::<new_game::Session>().field().events.tick(),
            tick
        );
        assert_checkpoint(
            world
                .resource::<new_game::Session>()
                .restored_checkpoint
                .as_ref()
                .unwrap(),
            &published,
        )
        .unwrap();

        world
            .resource_mut::<new_game::Session>()
            .field_mut()
            .step(default())
            .unwrap();
        assert!(
            !world
                .resource::<new_game::Session>()
                .field()
                .events
                .world
                .dialogue
                .is_empty()
        );
        world.get_mut::<Camera>(visible).unwrap().is_active = false;
        world.insert_resource(RetainedFrame(vec![(visible, true)]));
        release_frame(world);
        assert!(!world.contains_resource::<RetainedFrame>());
        assert!(world.get::<Camera>(visible).unwrap().is_active);
        assert!(
            checkpoint(world).is_err(),
            "active notice must block saving"
        );
    }
}
