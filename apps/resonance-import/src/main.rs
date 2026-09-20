use clap::{Args as ClapArgs, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Extract your Symphonia disc and convert assets for Resonance")]
struct Args {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Cook every physical asset from the extracted discs, including unused assets.
    CookAll {
        /// Parallel asset workers; codecs run within this worker limit.
        #[arg(long, default_value_t = std::thread::available_parallelism().map_or(1, |n| n.get().min(resonance_import::all_assets::MAX_WORKERS)) as u8, value_parser = clap::value_parser!(u8).range(1..=resonance_import::all_assets::MAX_WORKERS as i64))]
        jobs: u8,
        #[arg(long, num_args=1.., default_values=["local/extracted/disc1", "local/extracted/disc2"])]
        extracted: Vec<PathBuf>,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
        #[arg(long)]
        coefficients: PathBuf,
    },
    /// Cook localized character, item and title names without changing save definitions.
    CookText {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Prepare figurine catalogue records using shared models, textures and clips.
    CookFigurines {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
        #[arg(long, num_args = 1..)]
        figurine: Vec<u16>,
    },
    /// Prepare enemy catalogue records using shared models, textures and clips.
    CookMonsters {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
        #[arg(long, num_args = 1..)]
        monster: Vec<u8>,
    },
    /// Cook menu descriptions, statistics, frames, and character illustrations.
    CookMenu {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Compare every cooked shop inventory and price with the original disc data.
    ValidateShops {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        cooked: PathBuf,
        /// Write the full inventory and pricing report to this JSON file.
        #[arg(long)]
        json: Option<PathBuf>,
    },
    /// Refresh shared field sprites and dependent preload manifests.
    CookEffects {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Prepare skit scripts and bind shared portraits and media without opening audio.
    CookSkits {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Build a conservative preload manifest from an already cooked field.
    CookFieldPreload {
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
        /// Field metadata path relative to --output.
        #[arg(long)]
        field: String,
        /// Audio metadata paths relative to --output; repeat for every bank.
        #[arg(long)]
        audio: Vec<String>,
        /// Movie metadata paths relative to --output; repeat for every movie.
        #[arg(long)]
        movie: Vec<String>,
    },
    /// Cook the classroom's original scores, cues, and spoken lines without playback.
    CookClassroomAudio {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
        #[arg(long)]
        coefficients: PathBuf,
    },
    /// Cook a field's declared music, cues, and spoken lines without playback.
    CookFieldAudio {
        #[arg(long)]
        map: u32,
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
        #[arg(long)]
        coefficients: PathBuf,
        /// Additional extracted disc containing voices absent from the primary disc.
        #[arg(long)]
        additional_disc: Option<PathBuf>,
    },
    /// Bind the New Game story movie and selected audio from the shared library.
    CookStoryIntro {
        #[command(flatten)]
        paths: MediaPaths,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u16).range(1..=256))]
        audio_stream: u16,
    },
    /// Cook the Iselia classroom environment and original event data in Rust.
    CookClassroom {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Cook a field's geometry, collision, scenario and model packages by disc ID.
    CookField {
        #[arg(long)]
        map: u32,
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Inspect a field archive, its original scenario, messages, and native calls.
    InspectField {
        #[arg(long, required_unless_present = "map", conflicts_with = "map")]
        source: Option<PathBuf>,
        /// Indexed field resource in the original executable.
        #[arg(long, required_unless_present = "source")]
        map: Option<u32>,
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Diagnose cue overlaps through shared effects, without playback.
    RenderSoundSequence {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        frames: u32,
        /// Source frame and sound ID, e.g. --event 0:1 --event 16640:1.
        #[arg(long = "event", required = true, value_parser = parse_sound_event)]
        events: Vec<(u32, u16)>,
    },
    /// Render an original-score diagnostic window including loops, without playback.
    RenderTitleAudioPreview {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long)]
        coefficients: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 160000)]
        frames: u32,
        /// Include the title's 2000-ms master / 100-ms sequence startup fades;
        /// specify how many milliseconds the master fade precedes song startup.
        #[arg(long)]
        master_fade_lead_ms: Option<u16>,
    },
    /// Diagnose one instrument macro through note-off and release, without playback.
    RenderMusicVoice {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long)]
        coefficients: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        macro_id: u16,
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..128))]
        key: u8,
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u8).range(0..128))]
        velocity: u8,
        #[arg(long, default_value_t = 1500)]
        hold_ms: u32,
        #[arg(long, default_value_t = 5000)]
        max_ms: u32,
    },
    /// Diagnose pitched instrument PCM; writes a WAV without opening an audio device.
    RenderPitchedSample {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/extracted/disc1/files/S/inst.snd")]
        bank: PathBuf,
        /// Explicit 4096-byte big-endian DSP interpolation coefficients.
        #[arg(long)]
        coefficients: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        id: u16,
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..128))]
        key: Option<u8>,
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        cents: i8,
        #[arg(long, default_value_t = 32000, value_parser = clap::value_parser!(u32).range(1..=320000))]
        frames: u32,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(0..4))]
        coefficient_set: u8,
    },
    /// Inspect the original title score and instrument setup in Rust, without playback.
    InspectTitleAudio {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Extract {
        #[arg(long)]
        disc: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    CookTitle {
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2))]
        disc: u8,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Decode the original startup logos in Rust and store KTX2 textures.
    CookBoot {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Prepare title music from the complete cooked library.
    CookTitleAudio {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Record only cooked music data through the shared renderer; no playback.
    RenderCookedTitleAudio {
        #[arg(long, default_value = "local/all-assets")]
        assets: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 1280000)]
        frames: u32,
        #[arg(long, default_value_t = 1185)]
        master_fade_lead_ms: u16,
    },
    /// Render navigation and confirmation cues without speaker playback.
    CookTitleSounds {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
        #[arg(long)]
        coefficients: PathBuf,
    },
    /// Diagnose Rust voice buses and their studio mix; writes WAVs without playback.
    RenderSoundBuses {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/extracted/disc1/files/S/se.snd")]
        bank: PathBuf,
        #[arg(long)]
        id: u16,
        #[arg(long)]
        output: PathBuf,
    },
    /// Bind the opening movie and selected audio from the shared library.
    CookIntro {
        #[command(flatten)]
        paths: MediaPaths,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u16).range(1..=256))]
        audio_stream: u16,
    },
}

#[derive(ClapArgs)]
struct MediaPaths {
    #[arg(long, default_value = "local/extracted/disc1")]
    extracted: PathBuf,
    #[arg(long, default_value = "local/all-assets")]
    output: PathBuf,
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Action::CookAll {
            jobs,
            extracted,
            output,

            coefficients,
        } => {
            let report =
                resonance_import::all_assets::cook(&resonance_import::all_assets::Options {
                    jobs: usize::from(jobs),
                    discs: &extracted,
                    output: &output,
                    coefficients: &coefficients,
                })?;
            println!(
                "{} conversion units, {} reused resources, {} failures",
                report.cooked,
                report.duplicates,
                report.failures.len()
            );
            anyhow::ensure!(
                report.failures.is_empty(),
                "some assets could not be cooked; see {}",
                output.join("failures.json").display()
            );
            Ok(())
        }
        Action::CookText { extracted, output } => resonance_import::cook_text(&extracted, &output),
        Action::CookFigurines {
            extracted,
            output,
            figurine,
        } => resonance_import::figurines::cook(&extracted, &output, &figurine),
        Action::CookMonsters {
            extracted,
            output,
            monster,
        } => resonance_import::monsters::cook(&extracted, &output, &monster),
        Action::CookMenu { extracted, output } => {
            resonance_import::menu::cook_all(&extracted, &output)
        }
        Action::ValidateShops {
            extracted,
            cooked,
            json,
        } => {
            let report = resonance_import::menu::validate_shops(&extracted, &cooked)?;
            if let Some(path) = json {
                if let Some(parent) = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
            }
            println!(
                "Checked {} shops, {} stock entries, {} prices, and {} story variants.",
                report.shops.len(),
                report.stock_entries,
                report.price_checks,
                report.story_variants
            );
            Ok(())
        }
        Action::CookFieldAudio {
            map,
            extracted,
            output,
            coefficients,
            additional_disc,
        } => resonance_import::media::cook_field_audio(
            &extracted,
            &output,
            map,
            &coefficients,
            additional_disc.as_deref(),
        ),
        Action::CookSkits { extracted, output } => {
            resonance_import::skit::cook_all(&extracted, &output)
        }
        Action::CookEffects { extracted, output } => {
            resonance_import::cook_effects(&extracted, &output)
        }
        Action::CookClassroomAudio {
            extracted,
            output,
            coefficients,
        } => {
            resonance_import::media::cook_field_audio(&extracted, &output, 340, &coefficients, None)
        }
        Action::CookStoryIntro {
            paths,
            audio_stream,
        } => resonance_import::media::cook_movie(
            &paths.extracted,
            &paths.output,
            audio_stream,
            resonance_import::media::MovieSource::StoryIntroduction,
        ),
        Action::CookFieldPreload {
            output,
            field,
            audio,
            movie,
        } => resonance_import::field_preload::cook(
            &output,
            resonance_import::field_preload::Inputs {
                field,
                audio: audio.into_iter().collect(),
                movies: movie.into_iter().collect(),
            },
        )
        .map(|_| ()),
        Action::CookClassroom { extracted, output } => {
            resonance_import::field::cook_field(&extracted, 340, &output)
        }
        Action::CookField {
            map,
            extracted,
            output,
        } => resonance_import::field::cook_field(&extracted, map, &output),
        Action::InspectField {
            source,
            map,
            extracted,
            output,
        } => {
            let source = match source {
                Some(source) => source,
                None => resonance_import::field::source_for_id(&extracted, map.unwrap())?,
            };
            resonance_import::field::inspect(&source, &output)
        }
        Action::RenderSoundSequence {
            extracted,
            output,
            frames,
            events,
        } => resonance_import::media::render_sound_sequence(&extracted, &output, frames, &events),
        Action::RenderTitleAudioPreview {
            extracted,
            coefficients,
            output,
            frames,
            master_fade_lead_ms,
        } => resonance_import::media::render_title_audio_preview(
            &extracted,
            &coefficients,
            &output,
            frames,
            master_fade_lead_ms,
        ),
        Action::RenderMusicVoice {
            extracted,
            coefficients,
            output,
            macro_id,
            key,
            velocity,
            hold_ms,
            max_ms,
        } => resonance_import::media::render_music_voice(
            resonance_import::media::MusicVoiceOptions {
                extracted: &extracted,
                coefficients: &coefficients,
                output: &output,
                macro_id,
                key,
                velocity,
                hold_ms,
                max_ms,
            },
        ),
        Action::RenderPitchedSample {
            extracted,
            bank,
            coefficients,
            output,
            id,
            key,
            cents,
            frames,
            coefficient_set,
        } => resonance_import::media::render_pitched_sample(
            resonance_import::media::PitchedSampleOptions {
                extracted: &extracted,
                bank: &bank,
                coefficients: &coefficients,
                output: &output,
                id,
                key,
                cents,
                frames,
                coefficient_set,
            },
        ),
        Action::InspectTitleAudio { extracted, output } => {
            resonance_import::media::inspect_title_audio(&extracted, &output)
        }
        Action::Extract { disc, output } => resonance_import::extract(&disc, &output),
        Action::CookBoot { extracted, output } => resonance_import::cook_boot(&extracted, &output),
        Action::CookTitle { disc, output } => resonance_import::cook_title(&output, disc),
        Action::CookTitleAudio { extracted, output } => {
            resonance_import::media::cook_title_audio(&extracted, &output)
        }
        Action::RenderCookedTitleAudio {
            assets,
            output,
            frames,
            master_fade_lead_ms,
        } => resonance_import::media::render_cooked_title_audio(
            &assets,
            &output,
            frames,
            master_fade_lead_ms,
        ),
        Action::CookTitleSounds {
            extracted,
            output,
            coefficients,
        } => resonance_import::media::cook_title_sounds(&extracted, &output, &coefficients),
        Action::RenderSoundBuses {
            extracted,
            bank,
            id,
            output,
        } => resonance_import::media::render_sound_buses(&extracted, &bank, id, &output),
        Action::CookIntro {
            paths,
            audio_stream,
        } => resonance_import::media::cook_movie(
            &paths.extracted,
            &paths.output,
            audio_stream,
            resonance_import::media::MovieSource::Opening,
        ),
    }
}

fn parse_sound_event(value: &str) -> Result<(u32, u16), String> {
    let (frame, id) = value.split_once(':').ok_or("expected FRAME:SOUND_ID")?;
    Ok((
        frame.parse().map_err(|_| "invalid cue frame")?,
        id.parse().map_err(|_| "invalid sound ID")?,
    ))
}
