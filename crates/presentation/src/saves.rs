//! Development quicksave controls. Player menus use the same checkpoint and store.
mod fixture;
mod menu;
mod menu_probe;
mod overworld_test;
mod probe;
mod replay;
pub use fixture::prepare_checkpoint_fixture;
pub use overworld_test::{prepare_overworld_test_fixture, run_overworld_field_probe};
pub(crate) use replay::record_live;
pub use replay::{CheckpointReplay, record_checkpoint, record_checkpoint_with_display};
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

/// Field saves retain their existing JSON shape. World saves carry a separate
/// scene checkpoint and the suspended field package needed by the scene owner.
#[derive(Clone, serde::Serialize)]
#[serde(untagged)]
pub(super) enum SceneCheckpoint {
    Field(FieldCheckpoint),
    World(WorldCheckpoint),
}
impl<'de> serde::Deserialize<'de> for SceneCheckpoint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Serde's untagged buffer loses JSON's numeric map-key conversion.
        // Inventory, bestiary and event records all use integer keys. Select
        // the existing wire shape explicitly, then use the JSON deserializer.
        let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
        if value.get("overworld").is_some() {
            serde_json::from_value(value).map(Self::World)
        } else {
            serde_json::from_value(value).map(Self::Field)
        }
        .map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorldCheckpoint {
    pub overworld: resonance_game::overworld::Checkpoint,
    pub anchor_field: u32,
}
impl SceneCheckpoint {
    fn menu_snapshot(&self) -> FieldCheckpoint {
        match self {
            Self::Field(checkpoint) => checkpoint.clone(),
            Self::World(checkpoint) => checkpoint.overworld.menu_snapshot(),
        }
    }
    pub fn map(&self) -> u32 {
        match self {
            Self::Field(c) => c.map_id,
            Self::World(c) => c.anchor_field,
        }
    }
    fn played_ticks(&self) -> u64 {
        match self {
            Self::Field(c) => c.played_ticks(),
            Self::World(c) => c.overworld.played_ticks,
        }
    }
    fn location(&self) -> String {
        match self {
            Self::Field(c) => format!("Field {}", c.map_id),
            Self::World(c) => match c.overworld.state.world {
                resonance_game::overworld::World::Sylvarant => "Sylvarant".into(),
                resonance_game::overworld::World::TetheAlla => "Tethe'alla".into(),
            },
        }
    }
}
#[derive(Resource)]
pub(super) struct WorldLoad(loading::Pending);

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
struct Capture {
    started: Instant,
    frames: u32,
    requested: bool,
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
        app.insert_resource(Capture {
            started: Instant::now(),
            frames: 0,
            requested: false,
        });
    }
    app.insert_resource(Persistence {
        store: Store::new(directory),
        slot,
        writing: Mutex::default(),
    });
    title::install(app);
    Ok(())
}

pub(super) fn capture(world: &mut World) {
    let options = world.resource::<super::RunOptions>();
    let Some(path) = options
        .capture
        .clone()
        .filter(|_| options.saves.load.is_some())
    else {
        return;
    };
    let state = world.resource::<Capture>();
    if state.requested {
        return;
    }
    if state.started.elapsed().as_secs() > 60 {
        error!("Saved-field capture timed out");
        world.write_message(AppExit::error());
        return;
    }
    let Ok(checkpoint) = scene_checkpoint(world) else {
        return;
    };
    let mut state = world.resource_mut::<Capture>();
    state.frames += 1;
    if state.frames < 30 {
        return;
    }
    state.requested = true;
    let metadata = serde_json::json!({
        "checkpoint": checkpoint, "audio_device": false,
        "width": resonance_content::WIDTH, "height": resonance_content::HEIGHT,
        "identity": world.resource::<new_game::Session>().identity,
    });
    let target = world.resource::<super::Framebuffer>().0.clone();
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    world.spawn(Screenshot(target)).observe(
        move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            if let Err(error) = crate::screenshot::write(&event.image, &path, Some(&metadata)) {
                error!("Saved-field capture failed: {error:#}");
                exit.write(AppExit::error());
            } else {
                exit.write(AppExit::Success);
            }
        },
    );
}

/// Only the live, prepared field can supply a checkpoint. Other modes have no
/// free-control session, or explicitly hold it while presenting their UI.
pub(super) fn checkpoint(world: &mut World) -> Result<FieldCheckpoint> {
    ensure!(
        !world.resource::<Time<Virtual>>().is_paused(),
        "quicksave unavailable while the game is paused"
    );
    ensure!(
        field_prepared(world),
        "quicksave unavailable while preparing the field"
    );
    ensure!(
        !world.resource::<super::movie::Playback>().active,
        "quicksave unavailable during a movie"
    );
    let session = world
        .get_resource::<new_game::Session>()
        .context("quicksave requires free field control")?;
    ensure!(
        session.overworld.is_none() && session.ready_for_field && session.audio.is_none(),
        "quicksave unavailable during a scene presentation"
    );
    session.field.checkpoint()
}

fn scene_checkpoint(world: &mut World) -> Result<SceneCheckpoint> {
    if world
        .get_resource::<new_game::Session>()
        .is_none_or(|s| s.overworld.is_none())
    {
        return checkpoint(world).map(SceneCheckpoint::Field);
    }
    ensure!(
        !world.resource::<Time<Virtual>>().is_paused(),
        "cannot save while paused"
    );
    ensure!(
        field_prepared(world),
        "cannot save while preparing the scene"
    );
    let session = world.resource::<new_game::Session>();
    ensure!(
        session.audio.is_none(),
        "cannot save during audio preparation"
    );
    Ok(SceneCheckpoint::World(WorldCheckpoint {
        overworld: session.overworld.as_ref().unwrap().session.checkpoint()?,
        anchor_field: session.assets.map_id,
    }))
}

fn field_prepared(world: &mut World) -> bool {
    !world.contains_resource::<loading::Pending>()
        && !world.contains_resource::<loading::FieldPending>()
        && !world.contains_resource::<loading::WorldPending>()
        && !world.contains_resource::<WorldLoad>()
        && world
            .resource::<loading::Resident>()
            .active
            .load(std::sync::atomic::Ordering::Acquire)
        && if world
            .get_resource::<new_game::Session>()
            .is_some_and(|s| s.overworld.is_some())
        {
            super::overworld::ready(world)
        } else {
            field_view::ready(world)
        }
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
    let (_, checkpoint): (_, SceneCheckpoint) =
        resonance_persistence::decode(&bytes, &world.resource::<new_game::Session>().identity)?;
    if matches!(checkpoint, SceneCheckpoint::World(_))
        || world.resource::<new_game::Session>().overworld.is_some()
        || recovering
    {
        let pending = loading::Pending::start(
            world.resource::<super::RunOptions>().assets.clone(),
            world.resource::<super::RunOptions>().script_root.clone(),
            Some(bytes),
            world.resource::<loading::Resident>(),
        )?;
        world.insert_resource(WorldLoad(pending));
        return Ok("Loading quicksave".into());
    }
    let SceneCheckpoint::Field(checkpoint) = checkpoint else {
        unreachable!()
    };
    let changing_field = checkpoint.map_id != world.resource::<new_game::Session>().assets.map_id;
    let started = Instant::now();
    let scripts = world.resource::<super::RunOptions>().script_root.clone();
    if scripts.is_some() {
        let resident = world.resource::<loading::Resident>().clone();
        world.resource_mut::<new_game::Session>().refresh_scripts(
            checkpoint.map_id,
            scripts,
            &resident,
        )?;
    }
    world
        .resource_mut::<new_game::Session>()
        .restore(checkpoint)?;
    restored(world, changing_field);
    Ok(format!(
        "Quicksave loaded (field initialization {:.2} ms)",
        started.elapsed().as_secs_f64() * 1000.
    ))
}

fn restored(world: &mut World, changing_field: bool) {
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
        || !field_prepared(world)
        || world
            .get_resource::<new_game::Session>()
            .is_none_or(|session| {
                (session.overworld.is_none() && !session.ready_for_field) || session.audio.is_some()
            })
    {
        return;
    }
    for (entity, active) in world.remove_resource::<RetainedFrame>().unwrap().0 {
        if let Some(mut camera) = world.get_mut::<Camera>(entity) {
            camera.is_active = active;
        }
    }
}

pub(super) fn update(world: &mut World) {
    if let Some(pending) = world.get_resource::<WorldLoad>() {
        let result = match pending.0.poll() {
            Ok(None) => return,
            Ok(Some(result)) => result,
            Err(error) => Err(error),
        };
        world.remove_resource::<WorldLoad>();
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
    fn field_and_world_save_shapes_preserve_numeric_inventory_and_event_keys() -> Result<()> {
        let field: FieldCheckpoint = serde_json::from_value(serde_json::json!({
            "map_id": 330, "position": [0, 0, 0], "heading": 0,
            "progress": {
                "script_globals": [], "event_flags": [22], "random_state": 0, "tick": 100,
                "event_records": {"12": {"value": 1, "extra": 0, "tick": 50}},
                "party": {
                    "members": [], "formation": [], "items": {"58": 1},
                    "found_items": [58], "recent_items": [58], "gald": 0, "spent_gald": 0,
                    "settings": resonance_events::party::Settings::default()
                }
            }
        }))?;
        let state = resonance_content::overworld::TravelState {
            world: resonance_content::overworld::World::Sylvarant,
            position: resonance_content::overworld::Position::from_map([9770., 23500., 0.])?,
            heading: 0.,
            camera_yaw: 0.,
            alternate_perspective: false,
            map_display: Default::default(),
            mount: resonance_content::overworld::Mount::Rheairds,
            altitude: 600.,
        };
        let world = WorldCheckpoint {
            overworld: resonance_game::overworld::Checkpoint {
                state,
                progress: field.progress.clone(),
                played_ticks: 0,
            },
            anchor_field: 330,
        };
        for checkpoint in [SceneCheckpoint::Field(field), SceneCheckpoint::World(world)] {
            let bytes = serde_json::to_vec(&checkpoint)?;
            let decoded: SceneCheckpoint = serde_json::from_slice(&bytes)?;
            assert_eq!(
                serde_json::to_value(&checkpoint)?,
                serde_json::to_value(decoded)?
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked fields and the captured slope quicksave; no window or audio device"]
    fn restored_frame_releases_during_authored_notice_without_enabling_saves() {
        use std::{fs, path::Path, sync::atomic::Ordering};
        struct Directory(PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let scripts = Directory(
            std::env::temp_dir().join(format!("resonance-retained-frame-{}", std::process::id())),
        );
        fs::create_dir(&scripts.0).unwrap();
        fs::write(
            scripts.0.join("fields.json"),
            r#"{"332":{"module":"entry","task":"run","on":"entry"}}"#,
        )
        .unwrap();
        fs::write(
            scripts.0.join("entry.sym"),
            "use game::field; pub task run() { await field::notice(\"Ready.\"); }",
        )
        .unwrap();
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
            PathBuf::from,
        );
        let identity = new_game::Session::identity(&root).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/milestone-3/slope-quicksave.json");
        let (_, saved): (_, FieldCheckpoint) =
            resonance_persistence::decode(&fs::read(fixture).unwrap(), &identity).unwrap();
        let mut cache = loading::Cache {
            scripts: Some(resonance_game::authored::FieldScripts::new(
                scripts.0.clone(),
            )),
            ..default()
        };
        let package =
            new_game::FieldPackage::prepare(&root, saved.map_id, &mut cache, || false).unwrap();
        let mut session =
            new_game::Session::load_prepared(&root, package.files, Some(saved), &mut cache)
                .unwrap();
        session.audio = None;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_resource::<loading::Resident>()
            .init_resource::<super::super::movie::Playback>();
        let art =
            field_view::prepared_test_art(&session.assets, app.world().resource::<AssetServer>());
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

        world
            .resource_mut::<new_game::Session>()
            .field
            .step(default())
            .unwrap();
        assert!(
            !world
                .resource::<new_game::Session>()
                .field
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
