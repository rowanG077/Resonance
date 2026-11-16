//! Live testing controls. Event skipping executes scripts and keeps their progress.
use crate::field_ui::pause as overlay;
use bevy::prelude::*;
use resonance_content::input::StandardAction;
use resonance_playback::Speed;

#[cfg(test)]
mod tests;

const SKIP_UPDATES_PER_FRAME: usize = 64;

#[derive(Resource, Default)]
pub(super) struct Controls {
    pub paused: bool,
    pub skipping: bool,
    pub speed: Speed,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Controls>()
        .add_systems(PreUpdate, controls.in_set(crate::input::HostActions))
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

pub(super) fn controls(
    input: Res<crate::input::HostInput>,
    options: Option<Res<super::RunOptions>>,
    session: Option<Res<super::new_game::Session>>,
    mut controls: ResMut<Controls>,
    mut time: ResMut<Time<Virtual>>,
    mut commands: Commands,
) {
    use StandardAction::*;
    // Slow motion is a live inspection control, not part of deterministic
    // captures. It also works at the title, without a session.
    if input.slow_motion && !options.is_some_and(|o| o.headless() || o.replay.is_some()) {
        slow_motion(&mut controls, &mut time);
        commands.queue(super::performance::show);
    }
    let Some(session) = session else {
        return;
    };
    let can_skip = session.is_field() && session.field().can_skip_event();
    for action in [SpeedToggle, Pause, SkipEvent] {
        if input.pressed(action) {
            toggle(&mut controls, &mut time, action, can_skip);
        }
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
            controls.paused = !controls.paused;
            if controls.paused {
                time.pause();
            } else {
                time.unpause();
            }
        }
        StandardAction::SkipEvent => controls.skipping = !controls.skipping && can_skip,
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

pub(super) fn audio(world: &mut World) {
    if let Some(controls) = world.get_resource::<Controls>() {
        let (paused, speed) = (controls.paused, controls.speed);
        for sink in world.query::<&super::audio_output::Sink>().iter(world) {
            sink.0.set_transport(paused, speed);
        }
    }
}

fn advance(world: &mut World) {
    let controls = world.resource::<Controls>();
    let skipping = controls.skipping && !controls.paused;
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
