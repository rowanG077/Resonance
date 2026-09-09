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
    options: Res<RunOptions>,
    mut clock: ResMut<Clock>,
    ready: Res<Ready>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
    recording: Option<Res<playthrough::Recording>>,
    loading: Option<Res<super::loading::Pending>>,
    resident: Option<Res<super::loading::Resident>>,
    session: Option<Res<super::new_game::Session>>,
) {
    // Presentation age continues across movies and title entries;
    // pure loading waits do not advance it.
    if options.capture.is_none()
        && loading.is_none()
        && (session.is_none() || resident.is_none_or(|r| r.active.load(Ordering::Acquire)))
        && ready.0
        && recording.is_none_or(|r| r.started)
        && (boot.active() || !movie.active || movie.is_presenting())
    {
        clock.0.advance();
    }
}
