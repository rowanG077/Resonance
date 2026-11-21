//! Asset preparation does not consume scene or presentation time.
use super::*;

#[cfg(test)]
mod tests;

/// Latched once the startup scene's assets and pipelines have been prepared.
/// The live player and recorder use the same gate before starting either audio
/// or fixed updates. GPU readback preparation is therefore not part of a replay.
#[derive(Resource, Default)]
pub(super) struct Ready(pub bool);

pub(super) fn prepare(
    mut ready: ResMut<Ready>,
    startup: crate::loading::startup::Status,
    field: Res<FieldAssets>,
    renderer: Res<RenderReady>,
    art: Res<Art>,
    server: Res<AssetServer>,
    boot: Res<boot::Playback>,
) {
    ready.0 |= startup.ready()
        && field.ready
        && boot.ready(&server)
        && renderer.0.load(Ordering::Acquire)
        && art
            .images
            .iter()
            .all(|image| server.is_loaded_with_dependencies(image.handle.id()));
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct ClockGate<'w> {
    ready: Res<'w, Ready>,
    movie: Res<'w, movie::Playback>,
    boot: Res<'w, boot::Playback>,
    loading: Option<Res<'w, super::loading::Pending>>,
    resident: Option<Res<'w, super::loading::Resident>>,
    battle: Option<Res<'w, super::battle::Owner>>,
    game_over: Option<Res<'w, super::game_over::Active>>,
}

impl ClockGate<'_> {
    pub(crate) fn running(&self, session: Option<&super::new_game::Session>) -> bool {
        let battle = self
            .battle
            .as_ref()
            .is_some_and(|battle| battle.presenting());
        self.loading.is_none()
            && (battle
                || session.is_none()
                || self
                    .resident
                    .as_ref()
                    .is_none_or(|r| r.active.load(Ordering::Acquire)))
            && session
                .is_none_or(|s| !s.events().battle_pending() || self.game_over.is_some() || battle)
            && session.is_none_or(|s| s.events().world.field_transition.is_none())
            && (session.is_some() || self.ready.0)
            && (self.boot.active() || !self.movie.active || self.movie.is_presenting())
    }
}

pub(super) fn advance_clock(
    mut clock: ResMut<Clock>,
    gate: ClockGate,
    mut session: Option<ResMut<super::new_game::Session>>,
) {
    // Presentation age continues across movies and title entries;
    // pure loading waits do not advance it.
    if gate.running(session.as_deref()) {
        clock.0.advance();
        if gate.movie.active
            && let Some(session) = &mut session
        {
            session.advance_play_time();
        }
    }
}
