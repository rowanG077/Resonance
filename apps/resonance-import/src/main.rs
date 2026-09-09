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
    /// Build a conservative preload manifest from an already cooked field.
    CookFieldPreload {
        #[arg(long, default_value = "local/cooked")]
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
        #[arg(long, default_value = "local/cooked")]
        output: PathBuf,
        #[arg(long)]
        coefficients: PathBuf,
        #[arg(long, default_value = "vgmstream-cli")]
        voice_decoder: PathBuf,
    },
    /// Cook the New Game story movie with the same verified offline pipeline.
    CookStoryIntro {
        #[command(flatten)]
        paths: MediaPaths,
        #[arg(long, default_value = "hvqm4-video")]
        video_decoder: PathBuf,
        #[arg(long, default_value = "vgmstream-cli")]
        audio_decoder: PathBuf,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2))]
        audio_stream: u8,
    },
    /// Cook the Iselia classroom environment and original event data in Rust.
    CookClassroom {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/cooked")]
        output: PathBuf,
        #[arg(long, default_value = "ktx")]
        ktx: PathBuf,
    },
    /// Inspect a field archive, its original scenario, messages, and native calls.
    InspectField {
        #[arg(long)]
        source: PathBuf,
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
        #[arg(long)]
        extracted: PathBuf,
        #[arg(long, default_value = "local/cooked")]
        output: PathBuf,
        #[arg(long, default_value = "ktx")]
        ktx: PathBuf,
    },
    /// Decode the original startup logos in Rust and store KTX2 textures.
    CookBoot {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/cooked")]
        output: PathBuf,
        #[arg(long, default_value = "ktx")]
        ktx: PathBuf,
    },
    /// Cook typed score/instrument data and decoded WAV samples in Rust.
    CookTitleAudio {
        #[arg(long, default_value = "local/extracted/disc1")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/cooked")]
        output: PathBuf,
        #[arg(long)]
        coefficients: PathBuf,
    },
    /// Record only cooked music data through the shared renderer; no playback.
    RenderCookedTitleAudio {
        #[arg(long, default_value = "local/cooked")]
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
        #[arg(long, default_value = "local/cooked")]
        output: PathBuf,
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
    /// Convert the original opening to lossless Matroska for oracle comparison.
    CookIntro {
        #[command(flatten)]
        paths: MediaPaths,
        #[arg(long, default_value = "hvqm4-video")]
        video_decoder: PathBuf,
        #[arg(long, default_value = "vgmstream-cli")]
        audio_decoder: PathBuf,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2))]
        audio_stream: u8,
    },
}

#[derive(ClapArgs)]
struct MediaPaths {
    #[arg(long, default_value = "local/extracted/disc1")]
    extracted: PathBuf,
    #[arg(long, default_value = "local/cooked")]
    output: PathBuf,
    #[arg(long, default_value = "ffmpeg")]
    ffmpeg: PathBuf,
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Action::CookClassroomAudio {
            extracted,
            output,
            coefficients,
            voice_decoder,
        } => resonance_import::media::cook_classroom_audio(
            &extracted,
            &output,
            &coefficients,
            &voice_decoder,
        ),
        Action::CookStoryIntro {
            paths,
            video_decoder,
            audio_decoder,
            audio_stream,
        } => resonance_import::media::cook_movie(
            &paths.extracted,
            &paths.output,
            &video_decoder,
            &audio_decoder,
            &paths.ffmpeg,
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
        Action::CookClassroom {
            extracted,
            output,
            ktx,
        } => resonance_import::field::cook_classroom(&extracted, &output, &ktx),
        Action::InspectField { source, output } => {
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
        Action::CookBoot {
            extracted,
            output,
            ktx,
        } => resonance_import::cook_boot(&extracted, &output, &ktx),
        Action::CookTitle {
            extracted,
            output,
            ktx,
        } => resonance_import::cook_title(&extracted, &output, &ktx),
        Action::CookTitleAudio {
            extracted,
            output,
            coefficients,
        } => resonance_import::media::cook_title_audio(&extracted, &output, &coefficients),
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
        Action::CookTitleSounds { extracted, output } => {
            resonance_import::media::cook_title_sounds(&extracted, &output)
        }
        Action::RenderSoundBuses {
            extracted,
            bank,
            id,
            output,
        } => resonance_import::media::render_sound_buses(&extracted, &bank, id, &output),
        Action::CookIntro {
            paths,
            video_decoder,
            audio_decoder,
            audio_stream,
        } => resonance_import::media::cook_intro(
            &paths.extracted,
            &paths.output,
            &video_decoder,
            &audio_decoder,
            &paths.ffmpeg,
            audio_stream,
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
