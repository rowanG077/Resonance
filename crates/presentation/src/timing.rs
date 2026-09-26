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
    images: Res<Assets<Image>>,
    server: Res<AssetServer>,
    boot: Res<boot::Playback>,
) {
    ready.0 |= field.ready
        && boot.ready(&server)
        && renderer.0.load(Ordering::Acquire)
        && art.images.iter().all(|handle| images.contains(handle));
}

#[allow(clippy::too_many_arguments)] // Shared clock with title, movie and field readiness gates.
pub(super) fn advance_clock(
    options: Res<RunOptions>,
    mut clock: ResMut<Clock>,
    ready: Res<Ready>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
    recording: Option<Res<playthrough::Recording>>,
    loading: Option<Res<super::loading::Pending>>,
    resident: Option<Res<super::loading::Resident>>,
    mut session: Option<ResMut<super::new_game::Session>>,
    pause: Option<Res<super::PresentationPause>>,
    battle: Option<Res<super::battle::Owner>>,
    game_over: Option<Res<super::game_over::Active>>,
) {
    // Presentation age continues across movies and title entries;
    // pure loading waits do not advance it.
    if options.capture.is_none()
        && loading.is_none()
        && (battle.as_ref().is_some_and(|battle| battle.presenting())
            || session.is_none()
            || resident
                .as_ref()
                .is_none_or(|r| r.active.load(Ordering::Acquire)))
        && session.as_ref().is_none_or(|session| {
            !session.field.events.battle_pending()
                || game_over.is_some()
                || battle.as_ref().is_some_and(|battle| battle.presenting())
        })
        && session
            .as_ref()
            .is_none_or(|s| s.field.events.world.field_transition.is_none())
        && ready.0
        && pause.is_none_or(|p| !p.0)
        && recording.is_none_or(|r| r.started)
        && (boot.active() || !movie.active || movie.is_presenting())
    {
        clock.0.advance();
        if movie.active
            && let Some(session) = &mut session
        {
            session.field.play_time.advance();
        }
    }
}
