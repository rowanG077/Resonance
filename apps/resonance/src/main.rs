use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Resonance — Tales of Symphonia reimplementation")]
#[command(group(clap::ArgGroup::new("checkpoint").args(["tick", "movie_frame", "boot_frame", "load"]).multiple(false)))]
struct Args {
    #[arg(long, default_value = "local/cooked")]
    assets: PathBuf,
    /// Directory for normal saves and development quicksave slots.
    #[arg(long)]
    save_directory: Option<PathBuf>,
    /// Development slot used by F5 (save) and F9 (load).
    #[arg(long)]
    quick_slot: Option<String>,
    /// Start from a free-exploration save, without replaying the opening.
    #[arg(long, conflicts_with_all = ["record_music", "record_playthrough", "replay"])]
    load: Option<PathBuf>,
    /// Fixed render resolution for this session (default 640x480); restart to change it.
    #[arg(long, conflicts_with_all = ["capture", "record_music", "record_playthrough", "replay"])]
    resolution: Option<resonance_presentation::Resolution>,
    /// Record the player's music/cue source to WAV without any window/audio device.
    #[arg(long, conflicts_with_all = ["capture", "tick", "movie_frame", "replay"])]
    record_music: Option<PathBuf>,
    /// Record rendered movie/title checkpoints and their mixed audio, without devices.
    #[arg(long, conflicts_with_all = ["record_music", "capture", "tick", "movie_frame", "reveal", "selected"])]
    record_playthrough: Option<PathBuf>,
    /// Title updates to record after the opening finishes.
    #[arg(long, requires = "record_playthrough", default_value_t = 1000)]
    record_title_ticks: u32,
    #[arg(long, requires = "record_music", default_value_t = 1280000)]
    audio_frames: u32,
    /// Add a cue request to recording, e.g. --audio-cue 487360:navigate.
    #[arg(long, requires = "record_music")]
    audio_cue: Vec<resonance_presentation::CueEvent>,
    /// Render one deterministic title tick to a PNG without a window or audio device.
    #[arg(long, requires = "capture")]
    tick: Option<u32>,
    /// Original counter at title entry for a checkpoint or title-only recording.
    #[arg(long, conflicts_with = "record_music")]
    presentation_start: Option<u32>,
    #[arg(long, requires = "checkpoint")]
    capture: Option<PathBuf>,
    /// Render an opening-movie frame without starting audio playback.
    #[arg(long, requires = "capture")]
    movie_frame: Option<u32>,
    /// Render one authored startup-logo tick without playback.
    #[arg(long, requires = "capture", conflicts_with_all = ["skip_intro", "replay", "presentation_start", "record_music", "record_playthrough"])]
    boot_frame: Option<u32>,
    /// Start directly at the title scene (also implied by title checkpoints/replays).
    #[arg(long, conflicts_with = "movie_frame")]
    skip_intro: bool,
    /// Reveal the menu immediately (development checkpoint).
    #[arg(long)]
    reveal: bool,
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=2))]
    selected: u8,
    /// Disable speaker output. Capture mode disables the audio device entirely.
    #[arg(long)]
    silent: bool,
    /// Show wall-clock FPS, frame-time percentiles and low FPS (F3 toggles).
    #[arg(long, conflicts_with_all = ["record_music", "capture", "record_playthrough"])]
    perf_overlay: bool,
    /// Stream frame timings and rolling summaries to a new JSONL file.
    #[arg(long, conflicts_with = "record_music")]
    perf_dump: Option<PathBuf>,
    /// Play a native title input fixture (update numbers, not Dolphin polls).
    #[arg(long, conflicts_with_all = ["reveal", "selected"])]
    replay: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if let Some(output) = &args.record_music {
        return resonance_presentation::record_title_music(
            &args.assets,
            output,
            args.audio_frames,
            !args.skip_intro,
            &args.audio_cue,
        );
    }
    resonance_presentation::run_with_display(
        resonance_presentation::RunOptions {
            saves: resonance_presentation::SaveOptions {
                directory: args.save_directory,
                quick_slot: args.quick_slot,
                load: args.load,
            },
            assets: args.assets,
            tick: args.tick,
            presentation_start: args.presentation_start,
            capture: args.capture,
            reveal: args.reveal,
            selected: args.selected as usize,
            silent: args.silent,
            replay: args.replay,
            movie_frame: args.movie_frame,
            boot_frame: args.boot_frame,
            skip_intro: args.skip_intro,
            record_playthrough: args.record_playthrough,
            record_title_ticks: args.record_title_ticks,
        },
        resonance_presentation::PerformanceOptions {
            overlay: args.perf_overlay,
            dump: args.perf_dump,
        },
        args.resolution.unwrap_or_default(),
    )
}
