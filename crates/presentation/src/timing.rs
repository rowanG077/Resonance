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
    field: Res<FieldAssets>,
    renderer: Res<RenderReady>,
    art: Res<Art>,
    server: Res<AssetServer>,
    boot: Res<boot::Playback>,
) {
    ready.0 |= field.ready
        && boot.ready(&server)
        && renderer.0.load(Ordering::Acquire)
        && art
            .images
            .iter()
            .all(|h| server.is_loaded_with_dependencies(h.id()));
}

#[allow(clippy::too_many_arguments)] // Shared clock with title, movie and field readiness gates.
pub(super) fn advance_clock(
    mut clock: ResMut<Clock>,
    ready: Res<Ready>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
    loading: Option<Res<super::loading::Pending>>,
    quickload: Option<Res<super::saves::Quickload>>,
    resident: Option<Res<super::loading::Resident>>,
    mut session: Option<ResMut<super::new_game::Session>>,
    game_over: Option<Res<super::session_screen::GameOver>>,
) {
    // Presentation age continues across movies and title entries;
    // pure loading waits do not advance it.
    if loading.is_none()
        && quickload.is_none()
        && (session.is_none()
            || resident
                .as_ref()
                .is_none_or(|r| r.active.load(Ordering::Acquire)))
        && session
            .as_ref()
            .is_none_or(|session| !session.events().battle_pending() || game_over.is_some())
        && session
            .as_ref()
            .is_none_or(|s| s.events().world.field_transition.is_none())
        && (session.is_some() || ready.0)
        && (boot.active() || !movie.active || movie.is_presenting())
    {
        clock.0.advance();
        if movie.active
            && let Some(session) = &mut session
        {
            session.advance_play_time();
        }
    }
}
