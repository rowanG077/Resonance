use super::*;
use resonance_game::{
    field::FieldInput,
    menu::{Menu, Mode, Page, Slot},
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub fn run_menu_probe(root: &Path, checkpoint: &Path, output: &Path) -> Result<()> {
    let mut app = probe::app(root, checkpoint, output, crate::Resolution::default())?;
    let completed = Arc::new(AtomicBool::new(false));
    app.insert_resource(Probe {
        output: output.into(),
        step: 0,
        settled: 0,
        started: Instant::now(),
        baseline: None,
        captured: Arc::new(AtomicBool::new(true)),
        completed: completed.clone(),
    })
    .add_systems(Update, drive.before(update));
    ensure!(
        app.run() == AppExit::Success && completed.load(Ordering::Acquire),
        "menu probe failed"
    );
    Ok(())
}
#[derive(Resource)]
struct Probe {
    output: PathBuf,
    step: u8,
    settled: u8,
    started: Instant,
    baseline: Option<FieldCheckpoint>,
    captured: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
}
fn drive(world: &mut World) {
    let mut probe = world.remove_resource::<Probe>().unwrap();
    if let Err(error) = probe.advance(world) {
        error!("Menu probe failed: {error:#}");
        world.write_message(AppExit::error());
    }
    world.insert_resource(probe);
}
impl Probe {
    fn advance(&mut self, world: &mut World) -> Result<()> {
        ensure!(
            self.started.elapsed().as_secs() < 90,
            "menu probe timed out at step {}",
            self.step
        );
        if !self.captured.load(Ordering::Acquire) || !field_view::ready(world) {
            return Ok(());
        }
        if self.step == 0 {
            if checkpoint(world).is_err() {
                return Ok(());
            }
        } else if self.step != 10 && menu(world).is_some_and(|m| m.busy || m.main_animating()) {
            return Ok(());
        }
        self.settled += 1;
        if self.settled < 12 {
            return Ok(());
        }
        self.settled = 0;
        match self.step {
            0 => {
                let state = checkpoint(world)?;
                ensure!(
                    world
                        .resource::<new_game::Session>()
                        .field
                        .events
                        .world
                        .save_points
                        .iter()
                        .any(|s| s.active),
                    "menu probe needs free control on a previously activated memory circle"
                );
                self.baseline = Some(state);
                press(world, Action::Confirm)?;
            }
            1 => {
                ensure!(
                    checkpoint(world).is_err(),
                    "quicksave accepted an open menu"
                );
                let menu = menu(world).unwrap();
                ensure!(
                    menu.page == Page::Slots(Mode::Save),
                    "memory circle did not open Save"
                );
                self.capture(world, "save-empty");
            }
            2 => {
                press(world, Action::Confirm)?;
                press(world, Action::Confirm)?;
                self.capture(world, "save-confirm");
            }
            3 => {
                press(world, Action::Confirm)?;
            }
            4 => {
                let menu = menu(world).unwrap();
                ensure!(
                    matches!(menu.slots[0], Slot::Saved { .. }) && menu.notice.is_some(),
                    "save did not finish"
                );
                self.capture(world, "save-success");
            }
            5 => {
                press(world, Action::Confirm)?;
                press(world, Action::Cancel)?;
                press(world, Action::Cancel)?;
                let state = checkpoint(world)?;
                ensure!(
                    state.position == self.baseline.as_ref().unwrap().position,
                    "menu moved the player"
                );
                press(world, Action::Menu)?;
                self.capture(world, "field-menu");
            }
            6 => {
                tap_direction(world, [-1., 0.])?;
                press(world, Action::Confirm)?;
                ensure!(
                    menu(world).is_some_and(|menu| menu.page == Page::System),
                    "System navigation failed"
                );
                self.capture(world, "system-menu");
            }
            7 => {
                tap_direction(world, [0., -1.])?;
                press(world, Action::Confirm)?;
            }
            8 => {
                press(world, Action::Confirm)?;
                let menu = menu(world).unwrap();
                ensure!(
                    menu.page == Page::Slots(Mode::Load)
                        && matches!(menu.slots[0], Slot::Saved { .. }),
                    "load menu lost the saved slot: page={:?}, slot={:?}",
                    menu.page,
                    menu.slots[0]
                );
                self.capture(world, "load-slot");
            }
            9 => {
                press(world, Action::Confirm)?;
                press(world, Action::Confirm)?;
            }
            10 => {
                let Ok(state) = checkpoint(world) else {
                    return Ok(());
                };
                let expected = self.baseline.as_ref().unwrap();
                ensure!(
                    state.position == expected.position
                        && state.heading == expected.heading
                        && state.progress.script_globals == expected.progress.script_globals
                        && state.progress.event_flags == expected.progress.event_flags,
                    "menu load did not preserve the checkpoint"
                );
                let late = world
                    .resource::<loading::Resident>()
                    .late_reads
                    .load(Ordering::Relaxed);
                ensure!(late == 0, "menu requested an unprepared asset");
                std::fs::write(
                    self.output.join("result.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "checkpoint":state,"audio_device":false,"late_reads":late,"save_load":true,
                        "quicksave_rejected_in_menu":true
                    }))?,
                )?;
                self.capture(world, "loaded-field");
            }
            _ => {
                self.completed.store(true, Ordering::Release);
                world.write_message(AppExit::Success);
            }
        }
        self.step += 1;
        Ok(())
    }
    fn capture(&self, world: &mut World, name: &str) {
        probe::capture(
            world,
            self.output.join(format!("{name}.png")),
            &self.captured,
        );
    }
}
fn menu(world: &World) -> Option<&Menu> {
    world.resource::<new_game::Session>().field.menu.as_ref()
}
enum Action {
    Confirm,
    Cancel,
    Menu,
}
fn press(world: &mut World, action: Action) -> Result<()> {
    world
        .resource_mut::<new_game::Session>()
        .field
        .step(FieldInput {
            interact: matches!(action, Action::Confirm),
            cancel: matches!(action, Action::Cancel),
            menu: matches!(action, Action::Menu),
            ..Default::default()
        })
}
fn tap_direction(world: &mut World, direction: [f32; 2]) -> Result<()> {
    let field = &mut world.resource_mut::<new_game::Session>().field;
    field.step(FieldInput {
        direction,
        ..Default::default()
    })?;
    field.step(FieldInput::default())
}
