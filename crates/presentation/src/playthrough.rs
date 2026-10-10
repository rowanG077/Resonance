//! Startup recording uses the same scenario runner as field and battle validation.
use super::*;
use resonance_playback::Decodable;

pub(super) fn capture(app: App, at: CaptureAt, path: &std::path::Path) -> Result<()> {
    use saves::{CheckpointReplay, ScenarioEvent as Event, ScenarioStep as Step};
    let (event, updates) = match at {
        CaptureAt::LoadedField => (Event::FieldReady, 600),
        CaptureAt::TitleTick(tick) => (Event::TitleTick { tick }, tick.saturating_add(1)),
        CaptureAt::BootTick(tick) => {
            anyhow::ensure!(
                tick < resonance_game::boot::LOGO_TICKS,
                "logo tick exceeds startup duration"
            );
            (Event::Boot { tick }, tick + 1)
        }
        CaptureAt::MovieFrame(frame) => {
            let movie = app.world().resource::<movie::Playback>();
            let updates = if let Some(asset) = &movie.asset {
                anyhow::ensure!(frame < asset.frames, "movie frame exceeds duration");
                let seconds = f64::from(frame + 1) * f64::from(asset.frame_micros) / 1_000_000.;
                (seconds * resonance_game::clock::UPDATE_HZ).ceil() as u32 + 120
            } else {
                // Startup already reported missing media. The shared runner
                // records the unavailable event and an invalid held image.
                1
            };
            (Event::Movie { resource: 0, frame }, updates)
        }
    };
    saves::capture_image(
        app,
        path,
        &CheckpointReplay::new(vec![Step::wait(event, updates), Step::capture("capture")]),
    )
}

pub(super) fn record(app: App, output: PathBuf) -> Result<()> {
    use saves::{CheckpointReplay, ScenarioEvent as Event, ScenarioStep as Step};
    let ticks = app.world().resource::<RunOptions>().record_title_ticks;
    anyhow::ensure!(
        (1..=7200).contains(&ticks),
        "record-title-ticks must be 1..7200"
    );
    anyhow::ensure!(!output.exists(), "recording directory already exists");
    let mut steps = Vec::new();
    if app.world().resource::<boot::Playback>().active() {
        let duration = resonance_game::boot::LOGO_TICKS;
        for tick in [0, duration / 2, duration - 1] {
            steps.push(Step::wait(Event::Boot { tick }, duration));
            steps.push(Step::capture(format!("boot-{tick}")));
        }
    }
    let mut title_wait = 600;
    if let Some(movie) = &app.world().resource::<movie::Playback>().asset {
        let updates_for = |frames: u32| {
            let seconds = f64::from(frames) * f64::from(movie.frame_micros) / 1_000_000.;
            (seconds * resonance_game::clock::UPDATE_HZ).ceil() as u32
        };
        let mut previous = 0;
        // Sample while the movie still owns presentation. Its final decoded
        // frame can retire in the same update as natural audio completion.
        for frame in [0, movie.frames / 2, movie.frames / 4 * 3]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
        {
            let max_updates = updates_for(frame - previous) + 120;
            steps.push(Step::wait(Event::Movie { resource: 0, frame }, max_updates));
            steps.push(Step::capture(format!("movie-{frame}")));
            previous = frame;
        }
        title_wait += updates_for(movie.frames - previous);
    }
    for tick in [0, ticks] {
        steps.push(Step::wait(Event::TitleTick { tick }, tick + title_wait));
        steps.push(Step::capture(format!("title-{tick}")));
        title_wait = 600;
    }
    saves::record_app(app, &output, &CheckpointReplay::new(steps), false)
}

pub(super) fn check_exit(app: &App) -> Result<()> {
    anyhow::ensure!(
        app.should_exit().is_none(),
        "recording application exited before completion"
    );
    Ok(())
}

pub(super) fn attach<T: Asset + Decodable + Clone>(
    world: &mut World,
    mixer: &resonance_playback::Control,
) -> Result<()> {
    super::audio_output::attach::<T>(world, mixer)
}
