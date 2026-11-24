//! Live testing controls. Event skipping executes scripts and keeps their progress.
use crate::audio_output::DevicePause;
use crate::field_ui::pause as overlay;
use crate::timing::live_gameplay;
use bevy::prelude::*;
use resonance_content::input::StandardAction;
use resonance_playback::Speed;
use std::time::Duration;

#[cfg(test)]
mod tests;

const SKIP_UPDATES_PER_FRAME: usize = 64;

/// While virtual time is paused, grant rumble only for a manual tick.
pub(super) fn rumble_lease(time: &Time<Virtual>, normal: Duration) -> Duration {
    if time.is_paused() {
        normal.min(time.delta())
    } else {
        normal
    }
}

#[derive(Resource, Default)]
pub(super) struct Controls {
    pub paused: bool,
    pub skipping: bool,
    pub speed: Speed,
    step: Option<TickStep>,
}

struct TickStep {
    held_since: Option<Duration>,
    pending: u32,
    resume_after_pending: bool,
}

impl Controls {
    pub(super) fn stepping(&self) -> bool {
        self.step.is_some()
    }

    fn tick_step(
        &mut self,
        input: &crate::input::HostInput,
        now: Duration,
        timestep: Duration,
        device: Option<&DevicePause>,
        time: &mut Time<Virtual>,
        current: &mut Time,
    ) {
        use crate::input::InspectionKey::*;
        let key = input.tick_step;
        let mut presses = match key {
            Pressed(count) | Released(count) => count,
            _ => 0,
        };
        // A release (possibly followed by another press in this input batch)
        // can be the first update to observe the previous hold's deadline.
        if let Some(step) = &mut self.step {
            if input.tick_hold_lost {
                step.held_since = None;
            }
            step.resume_after_pending |= match key {
                Up => false,
                Unavailable => true,
                _ => step
                    .held_since
                    .is_some_and(|start| now - start >= Duration::from_secs(3)),
            };
            if step.resume_after_pending {
                if step.pending == 0 {
                    self.resume(time);
                } else if presses > 0 {
                    // The first press after the hold freezes again. Earlier
                    // taps still get their own ticks.
                    presses -= 1;
                    step.resume_after_pending = false;
                }
            }
        }
        let Some(step) = &mut self.step else {
            if presses == 0 {
                return;
            }
            if time.is_paused() && !self.paused && !device.is_some_and(|pause| pause.resume) {
                return;
            }
            self.step = Some(TickStep {
                held_since: matches!(key, Pressed(_)).then_some(now),
                pending: presses - 1,
                resume_after_pending: false,
            });
            self.paused = true;
            self.skipping = false;
            // First has already advanced virtual time for this render.
            let mut previous = Time::new_with(*time.context());
            previous.set_wrap_period(time.wrap_period());
            previous.advance_to(time.elapsed() - time.delta());
            previous.pause();
            previous.advance_by(Duration::ZERO);
            *time = previous;
            *current = time.as_generic();
            return;
        };
        match key {
            Pressed(_) => step.held_since = Some(now),
            Released(_) | Up | Unavailable => step.held_since = None,
            Held => {}
        }
        step.pending = step.pending.saturating_add(presses);
        if device.is_none() && step.pending > 0 {
            // Present each tap separately, even when several arrived together.
            step.pending -= 1;
            time.advance_by(timestep);
            *current = time.as_generic();
        }
    }

    fn resume(&mut self, time: &mut Time<Virtual>) {
        self.step = None;
        self.paused = false;
        time.unpause();
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Controls>()
        .add_systems(
            PreUpdate,
            live_gameplay
                .pipe(controls)
                .in_set(crate::input::HostActions),
        )
        // A manual tick can hand the scene to a movie or the credits before audio attaches in Last.
        .add_systems(
            PostUpdate,
            release_step.run_if(stepping).run_if(not(live_gameplay)),
        )
        .add_systems(
            Startup,
            overlay::setup
                .after(crate::setup)
                .before(crate::display::initialize),
        )
        .add_systems(
            Update,
            (
                advance.before(super::new_game::skip_test_battles),
                // Publish the overlay while its camera still has its logical
                // activity, before the native renderer claims it in PostUpdate.
                (overlay::update, overlay::composite).chain(),
            ),
        );
}

fn stepping(controls: Res<Controls>) -> bool {
    controls.stepping()
}

fn release_step(
    mut controls: ResMut<Controls>,
    mut time: ResMut<Time<Virtual>>,
    device: Option<ResMut<DevicePause>>,
) {
    controls.resume(&mut time);
    if let Some(mut device) = device {
        device.resume = true;
        time.pause();
    }
}

pub(super) fn controls(
    In(gameplay): In<bool>,
    input: Res<crate::input::HostInput>,
    options: Option<Res<super::RunOptions>>,
    session: Option<Res<super::new_game::Session>>,
    (mut controls, device): (ResMut<Controls>, Option<ResMut<DevicePause>>),
    (mut time, mut current): (ResMut<Time<Virtual>>, ResMut<Time>),
    (real, fixed): (Res<Time<Real>>, Res<Time<Fixed>>),
    mut commands: Commands,
) {
    use StandardAction::*;
    let was_paused = controls.paused;
    if !options.is_some_and(|o| o.headless() || o.replay.is_some()) {
        if gameplay && !input.pressed(Pause) {
            controls.tick_step(
                &input,
                real.elapsed(),
                fixed.timestep(),
                device.as_deref(),
                &mut time,
                &mut current,
            );
        } else if !gameplay && controls.stepping() {
            controls.resume(&mut time);
        }
        if input.slow_motion {
            slow_motion(&mut controls, &mut time);
            commands.queue(super::performance::show);
        }
    }
    if let Some(session) = session {
        let can_skip = session.is_field() && session.field().can_skip_event();
        for action in [SpeedToggle, Pause, SkipEvent] {
            if input.pressed(action) {
                toggle(&mut controls, &mut time, action, can_skip);
            }
        }
    }
    if let Some(mut device) = device {
        if was_paused != controls.paused {
            device.resume = !controls.paused;
        }
        time.pause();
    }
}

fn toggle(
    controls: &mut Controls,
    time: &mut Time<Virtual>,
    action: StandardAction,
    can_skip: bool,
) {
    match action {
        StandardAction::SpeedToggle => {
            controls.speed = if controls.speed == Speed::Double {
                Speed::Normal
            } else {
                Speed::Double
            };
            time.set_relative_speed_f64(controls.speed.factor());
        }
        StandardAction::Pause => {
            if controls.paused {
                controls.resume(time);
            } else {
                controls.paused = true;
                time.pause();
            }
        }
        StandardAction::SkipEvent => {
            controls.skipping = !controls.skipping && can_skip && !controls.stepping()
        }
        _ => unreachable!(),
    }
    log(controls);
}

/// Cycles the live game speed through half, a fifth and a tenth back to normal.
fn slow_motion(controls: &mut Controls, time: &mut Time<Virtual>) {
    controls.speed = match controls.speed {
        Speed::Normal | Speed::Double => Speed::Half,
        Speed::Half => Speed::Fifth,
        Speed::Fifth => Speed::Tenth,
        Speed::Tenth => Speed::Normal,
    };
    time.set_relative_speed_f64(controls.speed.factor());
    log(controls);
}

fn log(controls: &Controls) {
    info!(
        "Testing: {}, {}, skip={}",
        controls.speed.label(),
        if controls.paused { "paused" } else { "playing" },
        controls.skipping
    );
}

pub(super) fn reset_session(world: &mut World) {
    if let Some(mut controls) = world.get_resource_mut::<Controls>() {
        *controls = default();
        if let Some(mut device) = world.get_resource_mut::<DevicePause>() {
            device.resume = true;
        } else {
            world.resource_mut::<Time<Virtual>>().unpause();
        }
        world.resource_mut::<Time<Virtual>>().set_relative_speed(1.);
    }
}

pub(super) fn audio(world: &mut World) {
    if let Some(controls) = world.get_resource::<Controls>() {
        let (paused, speed) = (controls.paused, controls.speed);
        let stepping = controls.stepping();
        let frames = if stepping {
            let fixed = world.resource::<Time<Fixed>>();
            let delta = world.resource::<Time<Virtual>>().delta();
            let sample = |t: Duration| {
                t.as_nanos() * u128::from(resonance_playback::SOURCE_RATE) / 1_000_000_000
            };
            (sample(fixed.elapsed()) - sample(fixed.elapsed() - delta)) as u64
        } else {
            0
        };
        for sink in world.query::<&super::audio_output::Sink>().iter(world) {
            if stepping {
                sink.0.step_transport(frames);
            } else {
                sink.0.set_transport(paused, speed);
            }
        }
    }
}

fn advance(world: &mut World) {
    let controls = world.resource::<Controls>();
    let skipping =
        controls.skipping && !controls.paused && !world.contains_resource::<DevicePause>();
    let result = (|| -> anyhow::Result<()> {
        if let Some(audio) = world.get_resource::<super::field_audio::Control>() {
            audio.set_skipping(skipping)?;
        }
        if !skipping || world.resource::<Time<Real>>().delta().is_zero() {
            return Ok(());
        }
        let Some(session) = world.get_resource::<super::new_game::Session>() else {
            world.resource_mut::<Controls>().skipping = false;
            return Ok(());
        };
        if session.overworld().is_some()
            || session.events().battle_pending()
            || world.contains_resource::<super::battle::Owner>()
        {
            world.resource_mut::<Controls>().skipping = false;
            return Ok(());
        }
        if !session.ready_for_field
            || session.audio.is_some()
            || !super::field_view::ready(world)
            || !world
                .resource::<super::loading::Resident>()
                .active
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(());
        }
        // Bound work per rendered frame.
        for _ in 0..SKIP_UPDATES_PER_FRAME {
            let mut session = world.resource_mut::<super::new_game::Session>();
            if session.field().events.world.field_transition.is_some()
                || session.field().events.world.world_transition.is_some()
            {
                break;
            }
            if session.field_mut().skip_event_step()? {
                world.resource_mut::<Controls>().skipping = false;
                break;
            }
            world.resource_mut::<super::Clock>().0.advance();
        }
        super::field_audio::update(world);
        Ok(())
    })();
    if let Err(error) = result {
        error!("Event skip failed: {error:#}");
        world.resource_mut::<Controls>().skipping = false;
        world.write_message(AppExit::error());
    }
}
