//! Development quicksave controls. Player menus use the same checkpoint and store.
mod fixture;
mod menu;
mod menu_probe;
mod probe;
mod replay;
pub use fixture::prepare_checkpoint_fixture;
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
    let Ok(checkpoint) = checkpoint(world) else {
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
        !world.contains_resource::<loading::Pending>()
            && !world.contains_resource::<loading::FieldPending>()
            && world
                .resource::<loading::Resident>()
                .active
                .load(std::sync::atomic::Ordering::Acquire)
            && field_view::ready(world),
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
        session.ready_for_field && session.audio.is_none(),
        "quicksave unavailable during a scene presentation"
    );
    session.field.checkpoint()
}

fn save(world: &mut World) -> Result<String> {
    let started = Instant::now();
    let state = checkpoint(world)?;
    let persistence = world.resource::<Persistence>();
    let mut writing = persistence.writing.lock().unwrap();
    ensure!(writing.is_none(), "previous save is still being written");
    let header = Header {
        identity: world.resource::<new_game::Session>().identity.clone(),
        label: format!("Quicksave {}", persistence.slot.as_str()),
        location: format!("Field {}", state.map_id),
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
    checkpoint(world)?;
    let persistence = world.resource::<Persistence>();
    ensure!(
        !persistence.is_writing(),
        "quicksave is still being written"
    );
    let bytes = persistence.store.read(Kind::Quicksave, &persistence.slot)?;
    let (_, checkpoint): (_, FieldCheckpoint) =
        resonance_persistence::decode(&bytes, &world.resource::<new_game::Session>().identity)?;
    let changing_field = checkpoint.map_id != world.resource::<new_game::Session>().assets.map_id;
    let started = Instant::now();
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
    if !world.contains_resource::<RetainedFrame>() || checkpoint(world).is_err() {
        return;
    }
    for (entity, active) in world.remove_resource::<RetainedFrame>().unwrap().0 {
        if let Some(mut camera) = world.get_mut::<Camera>(entity) {
            camera.is_active = active;
        }
    }
}

pub(super) fn update(world: &mut World) {
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
