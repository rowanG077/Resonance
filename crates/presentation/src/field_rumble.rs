//! Translate the field's single GameCube motor to the connected controller.
use super::field_view::State;
use bevy::{
    input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest},
    prelude::*,
};
use resonance_events::rumble::Rumble;
use std::time::Duration;

#[derive(Default)]
pub(super) struct Playing {
    effect: Option<(Entity, Rumble)>,
    refresh: f64,
}

pub(super) fn update(
    state: State,
    pads: Query<Entity, With<Gamepad>>,
    time: Res<Time>,
    mut playing: Local<Playing>,
    mut requests: MessageWriter<GamepadRumbleRequest>,
) {
    let field = state.checkpoint.as_ref().map(|s| &s.0).or_else(|| {
        state
            .live
            .as_ref()
            .filter(|s| s.is_field() && s.ready_for_field && s.audio.is_none())
            .map(|s| s.field())
    });
    let desired = field
        .filter(|f| !f.menu_is_open() && f.active_skit.is_none())
        .and_then(|f| {
            let world = &f.events.world;
            let effect = world.rumble?;
            if world
                .party
                .as_ref()
                .is_some_and(|p| !p.settings.preferences.rumble)
                || effect.remaining(world.tick) == Some(0)
                || world.blocked_by_movie()
                || world.battle_request.is_some()
                || world.field_transition.is_some()
                || world.world_transition.is_some()
            {
                return None;
            }
            let mut pads: Vec<_> = pads.iter().collect();
            pads.sort();
            let pad = *pads.get(usize::from(effect.port))?;
            Some((pad, effect, effect.remaining(world.tick)))
        });
    let identity = desired.map(|(pad, effect, _)| (pad, effect));
    let now = time.elapsed_secs_f64();
    if playing.effect == identity && (identity.is_none() || now < playing.refresh) {
        return;
    }
    if let Some((gamepad, _)) = playing.effect {
        // The portable gamepad API has one stop operation for both coast and brake.
        requests.write(GamepadRumbleRequest::Stop { gamepad });
    }
    playing.effect = identity;
    if let Some((gamepad, _, remaining)) = desired {
        // Renew long effects while this scene owns them. Bounded device requests
        // also stop feedback if the renderer or controller disappears.
        const LEASE_SECONDS: f64 = 1.;
        let seconds = remaining.map_or(LEASE_SECONDS, |ticks| {
            (f64::from(ticks) / resonance_content::ANIMATION_HZ as f64).min(LEASE_SECONDS)
        });
        requests.write(GamepadRumbleRequest::Add {
            gamepad,
            intensity: GamepadRumbleIntensity::strong_motor(1.),
            duration: Duration::from_secs_f64(seconds),
        });
        playing.refresh = now + seconds * 0.5;
    }
}
