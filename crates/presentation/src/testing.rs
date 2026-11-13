//! Live testing controls. Event skipping executes scripts and keeps their progress.
use crate::field_ui::pause as overlay;
use bevy::prelude::*;
use resonance_content::input::StandardAction;

const SKIP_UPDATES_PER_FRAME: usize = 64;

#[derive(Resource, Default)]
pub(super) struct Controls {
    pub paused: bool,
    pub skipping: bool,
    pub double_speed: bool,
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
        .add_systems(Last, overlay::composite)
        .add_systems(
            Update,
            (
                advance.before(super::new_game::skip_test_battles),
                overlay::update,
            ),
        );
}

pub(super) fn controls(
    input: Res<crate::input::HostInput>,
    session: Option<Res<super::new_game::Session>>,
    mut controls: ResMut<Controls>,
    mut time: ResMut<Time<Virtual>>,
) {
    use StandardAction::*;
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
            controls.double_speed = !controls.double_speed;
            time.set_relative_speed(if controls.double_speed { 2. } else { 1. });
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
    info!(
        "Testing: {}x, {}, skip={}",
        if controls.double_speed { 2 } else { 1 },
        if controls.paused { "paused" } else { "playing" },
        controls.skipping
    );
}

pub(super) fn audio(world: &mut World) {
    if let Some(controls) = world.get_resource::<Controls>() {
        let (paused, double_speed) = (controls.paused, controls.double_speed);
        for sink in world.query::<&super::audio_output::Sink>().iter(world) {
            sink.0.set_transport(paused, double_speed);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_resumes_virtual_time_without_losing_the_selected_speed() {
        use StandardAction::*;
        let mut controls = Controls::default();
        let mut time = Time::<Virtual>::default();
        toggle(&mut controls, &mut time, SpeedToggle, false);
        assert_eq!(time.relative_speed(), 2.);
        toggle(&mut controls, &mut time, Pause, false);
        assert!(controls.paused && time.is_paused());
        toggle(&mut controls, &mut time, SpeedToggle, false);
        assert!(time.is_paused());
        assert_eq!(time.relative_speed(), 1.);
        toggle(&mut controls, &mut time, Pause, false);
        assert!(!controls.paused && !time.is_paused());
        toggle(&mut controls, &mut time, SkipEvent, false);
        assert!(!controls.skipping);
        toggle(&mut controls, &mut time, SkipEvent, true);
        assert!(controls.skipping);
    }
}
