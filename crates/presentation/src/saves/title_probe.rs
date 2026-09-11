//! A separate process uses real keyboard edges to cancel and reopen title Load.
use super::*;
use resonance_game::menu::{Mode, Page, SlotFocus};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const SOUTH_PATH_X: f32 = 1800.;

pub fn run_title_load_probe(root: &Path, directory: &Path, output: &Path) -> Result<()> {
    let bytes = Store::new(directory).read(Kind::Save, &SlotId::new("a-001")?)?;
    let (_, expected) = resonance_persistence::decode(&bytes, &new_game::Session::identity(root)?)?;
    let mut app = probe::app_with_saves(
        root,
        output,
        SaveOptions {
            directory: Some(directory.into()),
            ..Default::default()
        },
    )?;
    let completed = Arc::new(AtomicBool::new(false));
    app.insert_resource(Probe {
        step: 0,
        settled: 0,
        started: Instant::now(),
        output: output.into(),
        expected,
        title_tick: 0,
        key_tick: None,
        captured: Arc::new(AtomicBool::new(true)),
        completed: completed.clone(),
    })
    .add_systems(Update, drive.before(update));
    ensure!(
        app.run() == AppExit::Success && completed.load(Ordering::Acquire),
        "title load probe did not finish"
    );
    Ok(())
}
#[derive(Resource)]
struct Probe {
    step: u8,
    settled: u8,
    started: Instant,
    output: PathBuf,
    expected: FieldCheckpoint,
    title_tick: u32,
    key_tick: Option<u32>,
    captured: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
}
fn drive(world: &mut World) {
    let mut probe = world.remove_resource::<Probe>().unwrap();
    if probe
        .key_tick
        .is_some_and(|tick| tick != world.resource::<crate::Clock>().0.tick())
    {
        world.resource_mut::<ButtonInput<KeyCode>>().release_all();
        probe.key_tick = None;
    }
    if let Err(error) = probe.advance(world) {
        error!("Title load probe failed: {error:#}");
        world.write_message(AppExit::error());
    }
    world.insert_resource(probe);
}
impl Probe {
    fn advance(&mut self, world: &mut World) -> Result<()> {
        ensure!(
            self.started.elapsed().as_secs() < 90,
            "title load probe timed out at step {}",
            self.step
        );
        if !self.captured.load(Ordering::Acquire)
            || !world
                .resource::<crate::RenderReady>()
                .0
                .load(Ordering::Acquire)
        {
            return Ok(());
        }
        self.settled += 1;
        if self.settled < 12 {
            return Ok(());
        }
        self.settled = 0;
        match self.step {
            0 => {
                if !world.resource::<crate::timing::Ready>().0
                    || world.resource::<crate::Menu>().0.opacity < 128
                {
                    return Ok(());
                }
                self.press(world, KeyCode::ArrowDown);
            }
            1 => {
                if world.resource::<crate::Menu>().0.selected != 1 {
                    return Ok(());
                }
                self.press(world, KeyCode::Enter);
            }
            2 => {
                if !ready(world, SlotFocus::Bank) {
                    return Ok(());
                }
                ensure!(checkpoint(world).is_err(), "title menu allowed a quicksave");
                self.title_tick = world.resource::<crate::Menu>().0.tick;
                self.capture(world, "title-load-banks");
            }
            3 => {
                ensure!(
                    world.resource::<crate::Menu>().0.tick == self.title_tick,
                    "title advanced underneath Load"
                );
                self.press(world, KeyCode::Escape);
            }
            4 => {
                if world.contains_resource::<title::LoadMenu>() {
                    return Ok(());
                }
                ensure!(
                    world.resource::<crate::Menu>().0.selected == 1,
                    "cancel lost title selection"
                );
                self.press(world, KeyCode::Enter);
            }
            5 => {
                if !ready(world, SlotFocus::Bank) {
                    return Ok(());
                }
                self.press(world, KeyCode::Enter);
            }
            6 => {
                if !ready(world, SlotFocus::List) {
                    return Ok(());
                }
                self.capture(world, "title-load-slot");
            }
            7 => {
                self.press(world, KeyCode::Enter);
            }
            8 => {
                if world.resource::<title::LoadMenu>().0.confirmation != Some(true) {
                    return Ok(());
                }
                self.capture(world, "title-load-confirm");
            }
            9 => {
                if world.resource::<title::LoadMenu>().0.confirmation != Some(true) {
                    return Ok(());
                }
                self.press(world, KeyCode::Enter);
            }
            10 => {
                let Ok(state) = checkpoint(world) else {
                    return Ok(());
                };
                ensure!(
                    state.map_id == self.expected.map_id
                        && state.position == self.expected.position
                        && state.heading == self.expected.heading
                        && state.camera == self.expected.camera
                        && state.progress.script_globals == self.expected.progress.script_globals
                        && state.progress.event_flags == self.expected.progress.event_flags
                        && serde_json::to_value(&state.progress.party)?
                            == serde_json::to_value(&self.expected.progress.party)?,
                    "title load changed progress, party or pose"
                );
                ensure!(
                    !world.contains_resource::<title::LoadMenu>(),
                    "load menu survived field activation"
                );
                let late = world
                    .resource::<loading::Resident>()
                    .late_reads
                    .load(Ordering::Acquire);
                ensure!(late == 0, "title load caused late field asset reads");
                std::fs::write(
                    self.output.join("result.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "checkpoint":state,"audio_device":false,"late_reads":late,"title_cancel_reopen":true,
                        "loaded_previous_process_save":true,"keyboard_input":true
                    }))?,
                )?;
                self.capture(world, "loaded-field");
            }
            11 => {
                if self.expected.map_id != 332 {
                    self.completed.store(true, Ordering::Release);
                    world.write_message(AppExit::Success);
                    return Ok(());
                }
                let session = world.resource::<new_game::Session>();
                if session.assets.map_id == 332 {
                    // Reach the middle of the southern path before following
                    // it out; walking straight south from the circle hits a fence.
                    let actor = &session.field.events.world.actors
                        [&session.field.events.world.controlled_actor];
                    let left = actor.position[0] > SOUTH_PATH_X;
                    let mut input = world.resource_mut::<ButtonInput<KeyCode>>();
                    input.release(if left {
                        KeyCode::ArrowDown
                    } else {
                        KeyCode::ArrowLeft
                    });
                    input.press(if left {
                        KeyCode::ArrowLeft
                    } else {
                        KeyCode::ArrowDown
                    });
                    return Ok(());
                }
                world.resource_mut::<ButtonInput<KeyCode>>().release_all();
                let Ok(state) = checkpoint(world) else {
                    return Ok(());
                };
                ensure!(
                    state.map_id == 330
                        && state.progress.script_globals == self.expected.progress.script_globals
                        && state.progress.event_flags == self.expected.progress.event_flags
                        && serde_json::to_value(&state.progress.party)?
                            == serde_json::to_value(&self.expected.progress.party)?,
                    "continued exploration lost saved progress or entered the wrong field"
                );
                let late = world
                    .resource::<loading::Resident>()
                    .late_reads
                    .load(Ordering::Acquire);
                ensure!(late == 0, "continued exploration read unprepared assets");
                std::fs::write(
                    self.output.join("continued-exploration.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "checkpoint":state,"from_saved_map":self.expected.map_id,
                        "keyboard_input":true,"audio_device":false,"late_reads":late,
                    }))?,
                )?;
                self.capture(world, "continued-village");
            }
            _ => {
                self.completed.store(true, Ordering::Release);
                world.write_message(AppExit::Success);
            }
        }
        self.step += 1;
        Ok(())
    }
    fn press(&mut self, world: &mut World, key: KeyCode) {
        world.resource_mut::<ButtonInput<KeyCode>>().press(key);
        self.key_tick = Some(world.resource::<crate::Clock>().0.tick());
    }
    fn capture(&self, world: &mut World, name: &str) {
        probe::capture(
            world,
            self.output.join(format!("{name}.png")),
            &self.captured,
        );
    }
}
fn ready(world: &World, focus: SlotFocus) -> bool {
    world.get_resource::<title::LoadMenu>().is_some_and(|menu| {
        !menu.0.busy
            && menu.0.tick >= 20
            && menu.0.page == Page::Slots(Mode::Load)
            && menu.0.focus == focus
    }) && world
        .get_resource::<crate::field_ui::MenuOverlay>()
        .is_some_and(|art| art.ready(world.resource::<Assets<Image>>()))
}
