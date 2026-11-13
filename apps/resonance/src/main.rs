use clap::Parser;
use std::path::PathBuf;

mod crash_dumps;
mod logging;

#[derive(Parser)]
#[command(version, about = "Resonance — Tales of Symphonia reimplementation")]
#[command(group(clap::ArgGroup::new("checkpoint").args(["tick", "movie_frame", "boot_frame", "load", "test_overworld"]).multiple(false)))]
struct Args {
    #[arg(long, default_value = "local/all-assets")]
    assets: PathBuf,
    /// Automatic numbered mods, loaded before explicit --mod directories.
    #[arg(long, default_value = "local/mods")]
    mods_directory: PathBuf,
    /// Skip automatic mods (explicit --mod directories still apply).
    #[arg(long)]
    no_default_mods: bool,
    /// Enable a mod directory. Repeat in priority order; the last mod wins.
    #[arg(long = "mod", value_name = "DIRECTORY")]
    mods: Vec<PathBuf>,
    /// Editable SymphoniaScript project with fields.json entry bindings.
    #[arg(long)]
    scripts: Option<PathBuf>,
    /// Directory for player saves.
    #[arg(long)]
    save_directory: Option<PathBuf>,
    /// Start from a free-exploration save, without replaying the opening.
    #[arg(long, conflicts_with_all = ["record_music", "record_playthrough"])]
    load: Option<PathBuf>,
    /// Temporary playground: start above Sylvarant on unlocked Rheairds.
    #[arg(long, conflicts_with_all = ["record_music", "record_playthrough"])]
    test_overworld: bool,
    /// Resolve encounters as victories for exploration testing.
    #[arg(long)]
    skip_battles: bool,
    /// Override the saved render resolution at startup (WIDTHxHEIGHT).
    #[arg(long, conflicts_with_all = ["capture", "record_music", "record_playthrough"])]
    resolution: Option<resonance_presentation::Resolution>,
    /// Record the player's music/cue source to WAV without any window/audio device.
    #[arg(long, conflicts_with_all = ["capture", "tick", "movie_frame"])]
    record_music: Option<PathBuf>,
    /// Record rendered movie/title checkpoints and their mixed audio, without devices.
    #[arg(long, conflicts_with_all = ["record_music", "capture", "tick", "movie_frame", "reveal"])]
    record_playthrough: Option<PathBuf>,
    /// Title updates to record after the opening finishes.
    #[arg(long, requires = "record_playthrough", default_value_t = 1000)]
    record_title_ticks: u32,
    #[arg(long, requires = "record_music", default_value_t = 1280000)]
    audio_frames: u32,
    /// Add a cue request to recording, e.g. --audio-cue 487360:navigate.
    #[arg(long, requires = "record_music")]
    audio_cue: Vec<resonance_presentation::CueEvent>,
    /// Render one title tick to a PNG without a window or audio device.
    #[arg(long, requires = "capture")]
    tick: Option<u32>,
    /// Presentation clock at title entry for deterministic recordings.
    #[arg(long, conflicts_with = "record_music")]
    presentation_start: Option<u32>,
    /// Play a native title input fixture.
    #[arg(long, conflicts_with = "reveal")]
    replay: Option<PathBuf>,
    #[arg(long, requires = "checkpoint")]
    capture: Option<PathBuf>,
    /// Play the opening movie to this frame and capture a held PNG without devices.
    #[arg(long, requires = "capture")]
    movie_frame: Option<u32>,
    /// Run the startup logos to this update and capture a held PNG without devices.
    #[arg(long, requires = "capture", conflicts_with_all = ["skip_intro", "record_music", "record_playthrough"])]
    boot_frame: Option<u32>,
    /// Start directly at the title scene (also implied by --tick).
    #[arg(long, conflicts_with_all = ["movie_frame", "record_music"])]
    skip_intro: bool,
    /// Reveal the menu immediately (development checkpoint).
    #[arg(long)]
    reveal: bool,
    /// Disable speaker output. Capture mode disables the audio device entirely.
    #[arg(long)]
    silent: bool,
    /// Stop on missing or unsupported content instead of logging and continuing.
    #[arg(long)]
    paranoid: bool,
    /// Keep text crash reports, but disable the native minidump monitor.
    #[arg(long)]
    no_crash_dumps: bool,
    /// Show wall-clock FPS, frame-time percentiles and low FPS (F3 toggles).
    #[arg(long, conflicts_with_all = ["record_music", "capture", "record_playthrough"])]
    perf_overlay: bool,
    /// Stream frame timings and rolling summaries to a new JSONL file.
    #[arg(long, conflicts_with = "record_music")]
    perf_dump: Option<PathBuf>,
}

impl Args {
    fn capture_target(&self) -> Option<resonance_presentation::CaptureAt> {
        use resonance_presentation::CaptureAt;
        self.tick
            .map(CaptureAt::TitleTick)
            .or_else(|| self.movie_frame.map(CaptureAt::MovieFrame))
            .or_else(|| self.boot_frame.map(CaptureAt::BootTick))
            .or_else(|| {
                (self.load.is_some() && self.capture.is_some()).then_some(CaptureAt::LoadedField)
            })
    }
}

fn main() -> anyhow::Result<()> {
    // SAFETY: This is the first operation in main, before starting any threads
    // or installing plugins. Respect an explicit opt-out, but capture the
    // originating anyhow stack for ordinary user launches by default.
    if std::env::var_os("RUST_LIB_BACKTRACE").is_none() {
        unsafe { std::env::set_var("RUST_LIB_BACKTRACE", "1") };
    }
    if let Some(result) = crash_dumps::monitor_command() {
        return result;
    }
    if let Some(result) = crash_dumps::self_test_command() {
        return result;
    }
    let args = Args::parse();
    let reports = logging::init();
    let _dumps = if args.no_crash_dumps {
        None
    } else {
        crash_dumps::install(&reports)
    };
    let result = run(args);
    if let Err(error) = &result {
        bevy::log::error!("{error:#}");
        reports.error(error);
    } else {
        bevy::log::info!("Resonance exited normally");
    }
    result
}

fn run(mut args: Args) -> anyhow::Result<()> {
    if !args.no_default_mods {
        let mut mods = resonance_content::library::discover_mods(&args.mods_directory)?;
        // An explicit --mod keeps its own priority, even for an automatic mod.
        let explicit: Vec<_> = args.mods.iter().map(std::fs::canonicalize).collect();
        mods.retain(|path| {
            let path = std::fs::canonicalize(path).ok();
            !explicit
                .iter()
                .any(|other| other.as_ref().ok() == path.as_ref())
        });
        mods.append(&mut args.mods);
        args.mods = mods;
    }
    bevy::log::info!(
        assets = ?args.assets, mods = ?args.mods, scripts = ?args.scripts,
        load = ?args.load, saves = ?args.save_directory, resolution = ?args.resolution,
        silent = args.silent, paranoid = args.paranoid, capture = ?args.capture,
        "Launch options"
    );
    // Keep the fixture and default save slots alive for this run only. The
    // ordinary checkpoint loader then owns rendering, audio and flight input.
    let _playground = if args.test_overworld {
        let directory = tempfile::tempdir()?;
        let checkpoint = directory.path().join("rheairds.json");
        resonance_presentation::prepare_overworld_test_fixture(
            &args.assets,
            &args.mods,
            args.scripts.as_deref(),
            &checkpoint,
        )?;
        args.load = Some(checkpoint);
        args.save_directory
            .get_or_insert_with(|| directory.path().join("saves"));
        Some(directory)
    } else {
        None
    };
    if let Some(output) = &args.record_music {
        return resonance_presentation::record_title_music(
            &args.assets,
            &args.mods,
            output,
            args.audio_frames,
            !args.skip_intro,
            &args.audio_cue,
        );
    }
    let capture_at = args.capture_target();
    resonance_presentation::run_with_display(
        resonance_presentation::RunOptions {
            saves: resonance_presentation::SaveOptions {
                directory: args.save_directory,
                load: args.load,
            },
            assets: args.assets,
            mods: args.mods,
            script_root: args.scripts,
            capture_at,
            presentation_start: args.presentation_start,
            replay: args.replay,
            capture: args.capture,
            reveal: args.reveal,
            silent: args.silent,
            paranoid: args.paranoid,
            skip_intro: args.skip_intro,
            skip_battles: args.skip_battles || args.test_overworld,
            allow_incomplete_scripts: args.test_overworld,
            record_playthrough: args.record_playthrough,
            record_title_ticks: args.record_title_ticks,
        },
        resonance_presentation::PerformanceOptions {
            overlay: args.perf_overlay,
            dump: args.perf_dump,
        },
        args.resolution,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mods_keep_command_line_precedence_with_saves_and_captures() {
        let args = Args::try_parse_from([
            "resonance",
            "--mod",
            "mods/translation",
            "--mod",
            "mods/music",
            "--load",
            "save.json",
            "--capture",
            "field.png",
        ])
        .unwrap();
        assert_eq!(
            args.mods,
            [
                PathBuf::from("mods/translation"),
                PathBuf::from("mods/music")
            ]
        );
        assert!(matches!(
            args.capture_target(),
            Some(resonance_presentation::CaptureAt::LoadedField)
        ));
        for mode in ["--record-music", "--test-overworld"] {
            let mut arguments = vec!["resonance", "--mod", "mods/music", mode];
            if mode == "--record-music" {
                arguments.push("track.wav");
            }
            assert_eq!(
                Args::try_parse_from(arguments).unwrap().mods,
                [PathBuf::from("mods/music")]
            );
        }
    }

    #[test]
    fn loaded_save_capture_uses_the_field_runner_but_play_does_not() {
        let capture =
            Args::try_parse_from(["resonance", "--load", "save.json", "--capture", "field.png"])
                .unwrap();
        assert!(matches!(
            capture.capture_target(),
            Some(resonance_presentation::CaptureAt::LoadedField)
        ));
        let play = Args::try_parse_from(["resonance", "--load", "save.json"]).unwrap();
        assert!(play.capture_target().is_none());
    }

    #[test]
    fn diagnostics_are_tolerant_unless_paranoid_is_requested() {
        assert!(!Args::try_parse_from(["resonance"]).unwrap().paranoid);
        assert!(
            Args::try_parse_from(["resonance", "--paranoid"])
                .unwrap()
                .paranoid
        );
    }
}
