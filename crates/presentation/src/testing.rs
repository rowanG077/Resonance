//! Live testing controls. Event skipping executes scripts and keeps their progress.
use bevy::prelude::*;

const SKIP_UPDATES_PER_FRAME: usize = 64;

#[derive(Resource, Default)]
pub(super) struct Controls {
    pub paused: bool,
    pub skipping: bool,
    pub double_speed: bool,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Controls>()
        .add_systems(
            PreUpdate,
            controls
                .after(bevy::input::InputSystems)
                .after(super::dungeons::controls)
                .before(super::field_view::gather_controls),
        )
        .add_systems(Update, advance.before(super::new_game::skip_test_battles));
}

fn controls(world: &mut World) {
    for (index, key) in [KeyCode::F6, KeyCode::F7, KeyCode::F8]
        .into_iter()
        .enumerate()
    {
        if world.resource::<ButtonInput<KeyCode>>().just_pressed(key) {
            press(world, index);
        }
    }
}

pub(super) fn press(world: &mut World, index: usize) {
    let Some(session) = world.get_resource::<super::new_game::Session>() else {
        return;
    };
    let can_skip = session.is_field() && session.field().can_skip_event();
    world.resource_scope(|world, mut controls: Mut<Controls>| {
        let mut time = world.resource_mut::<Time<Virtual>>();
        match index {
            0 => {
                controls.double_speed = !controls.double_speed;
                time.set_relative_speed(if controls.double_speed { 2. } else { 1. });
            }
            1 => {
                controls.paused = !controls.paused;
                if controls.paused {
                    time.pause();
                } else {
                    time.unpause();
                }
            }
            2 => controls.skipping = !controls.skipping && can_skip,
            _ => unreachable!(),
        }
        info!(
            "Testing: {}x, {}, skip={}",
            if controls.double_speed { 2 } else { 1 },
            if controls.paused { "paused" } else { "playing" },
            controls.skipping
        );
    });
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
    let skipping = world.resource::<Controls>().skipping;
    let result = (|| -> anyhow::Result<()> {
        if let Some(audio) = world.get_resource::<super::field_audio::Control>() {
            audio.set_skipping(skipping)?;
        }
        if !skipping
            || world.resource::<Time<Real>>().delta().is_zero()
            || world
                .get_resource::<super::dungeons::Menu>()
                .is_some_and(|menu| menu.blocked())
        {
            return Ok(());
        }
        let Some(session) = world.get_resource::<super::new_game::Session>() else {
            world.resource_mut::<Controls>().skipping = false;
            return Ok(());
        };
        if session.overworld().is_some() {
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
        // Bound work per rendered frame, including when virtual time is paused.
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
