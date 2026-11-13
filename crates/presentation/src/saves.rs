//! Player save menus, shared checkpoint diagnostics, and scene restoration.
mod fixture;
mod harness;
mod menu;
mod menu_probe;
mod overworld_test;
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
use resonance_game::field::FieldCheckpoint;
use resonance_persistence::{Header, Kind, SlotId, Store};
use std::{
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
pub use title_probe::run_title_load_probe;

pub(super) use resonance_game::Checkpoint as SceneCheckpoint;

#[derive(Default)]
pub struct SaveOptions {
    pub directory: Option<PathBuf>,
    pub load: Option<PathBuf>,
}

#[derive(Resource)]
struct Persistence {
    store: Store,
}

pub(super) fn install(app: &mut App, options: &SaveOptions, user_config: bool) -> Result<()> {
    let directory = options
        .directory
        .clone()
        .map_or_else(resonance_persistence::default_directory, Ok)?;
    info!(
        "Save directory: {}; initial save: {:?}",
        directory.display(),
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
        "checkpoint unavailable while the game is paused"
    );
    ensure!(
        scene_prepared(world),
        "checkpoint unavailable while preparing the scene"
    );
    ensure!(
        !world.resource::<super::movie::Playback>().active,
        "checkpoint unavailable during a movie"
    );
    let session = world
        .get_resource::<new_game::Session>()
        .context("checkpoint requires an active scene")?;
    ensure!(
        session.audio.is_none(),
        "checkpoint unavailable during audio preparation"
    );
    match &session.scene {
        new_game::Scene::Field(field) => {
            ensure!(
                session.ready_for_field,
                "checkpoint unavailable during a scene presentation"
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
        && world
            .resource::<loading::Resident>()
            .active
            .load(std::sync::atomic::Ordering::Acquire)
        && new_game::scene_ready(world)
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
    menu::update(world);
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
    fn checkpoint_rejects_paused_or_unprepared_scene() {
        let mut world = World::new();
        world.init_resource::<Time<Virtual>>();
        world.init_resource::<loading::Resident>();
        world.resource_mut::<Time<Virtual>>().pause();
        assert_eq!(
            checkpoint(&mut world).unwrap_err().to_string(),
            "checkpoint unavailable while the game is paused"
        );
        world.resource_mut::<Time<Virtual>>().unpause();
        assert_eq!(
            checkpoint(&mut world).unwrap_err().to_string(),
            "checkpoint unavailable while preparing the scene"
        );
    }
}
