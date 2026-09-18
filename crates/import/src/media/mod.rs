//! Offline media recipes. Rust owns parsing, validation, caching, and conversion;
//! pure Rust codecs run without opening an audio output device.
mod cooked_music;
mod field_audio;
pub(crate) mod voice_library;
pub(crate) use field_audio::FieldAudioCooker;
mod adx;
pub(crate) mod library;
mod movie;
mod music;
mod music_library;
mod music_score;
mod music_voice;
mod pitched_sample;
mod sound_buses;
pub(crate) mod sound_library;
mod sounds;

pub(crate) use field_audio::{VoiceFormat, decode_voice_to, music_path, sound_score};
pub(crate) use music::song_reverb_change;
pub(crate) use music_voice::tables as synthesis_tables;

pub(crate) use cooked_music::prepare_title_audio;
pub use cooked_music::render_cooked_title_audio;
pub(crate) use movie::bind_all_movies;
pub(crate) use movie::cook_directory as cook_movie_directory;
pub(crate) use movie::cook_movie_file;
pub(crate) use movie::is_movie;
pub use music_score::inspect_title_audio;
pub use music_voice::{MusicVoiceOptions, render_music_voice, render_title_audio_preview};
pub use pitched_sample::{PitchedSampleOptions, render_pitched_sample};
pub use sound_buses::{render_sound_buses, render_sound_sequence};
pub(crate) use sounds::prepare_title_sounds;

use crate::read::u32 as be_u32;
use anyhow::{Context, Result, ensure};
use resonance_asset_writer::wav::write_pcm16;
use resonance_audio::package::SampleAsset;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

const SAMPLE_RATE: u32 = 32_000;
// MusyX synthesizes 160-sample nominal 5 ms blocks. GameCube's DAC runs
// slightly faster: 108 MHz / (1124 * 3), recorded by Dolphin as 32,028 Hz.
// Preserve the sample sequence and label its playback clock; do not resample.
const PLAYBACK_RATE: u32 = 32_028;

pub(crate) struct Workspace {
    pub(crate) extracted: PathBuf,
    pub(crate) output: PathBuf,
    _session: Arc<OutputSession>,
}

impl Workspace {
    pub(crate) fn open(extracted: &Path, output: &Path) -> Result<Self> {
        OutputSession::open(output)?.workspace(extracted)
    }
}

/// One exclusive output owner, shared by every source in the same cook.
pub(crate) struct OutputSession {
    output: PathBuf,
    lock: PathBuf,
}

impl OutputSession {
    pub(crate) fn workspace(self: &Arc<Self>, extracted: &Path) -> Result<Workspace> {
        let extracted = extracted
            .canonicalize()
            .context("missing extracted disc directory")?;
        crate::disc_number(&extracted)?;
        Ok(Workspace {
            extracted,
            output: self.output.clone(),
            _session: Arc::clone(self),
        })
    }

    pub(crate) fn open(output: &Path) -> Result<Arc<Self>> {
        fs::create_dir_all(output)?;
        let output = output.canonicalize()?;
        let lock = output.join(".cook-media.lock");
        let mut file = fs::File::create_new(&lock).with_context(|| format!(
            "another media cook owns {}; remove a stale lock only after its process has stopped", lock.display()))?;
        use std::io::Write;
        if let Err(error) = writeln!(file, "{}", std::process::id()) {
            let _ = fs::remove_file(&lock);
            return Err(error.into());
        }
        Ok(Arc::new(Self { output, lock }))
    }
}

impl Drop for OutputSession {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock);
    }
}

pub(crate) fn hash_file(path: &Path) -> Result<String> {
    hash_reader(fs::File::open(path).with_context(|| format!("open {}", path.display()))?)
}

pub(crate) fn hash_reader(mut file: impl Read) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn json_file(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut data = serde_json::to_vec_pretty(value)?;
    data.push(b'\n');
    crate::write_atomic(path, &data)
}

/// All-asset cooks share identical PCM across sound, program and music packages.
/// Each worker writes a private temporary. Only complete immutable WAVs become
/// visible at the shared content address; no decoded samples are cached here.
pub(crate) fn write_shared_sample(
    output: &Path,
    sample: &resonance_audio::sample::Sample,
) -> Result<SampleAsset> {
    ensure!(
        sample.key < 128
            && sample.rate > 0
            && !sample.pcm.is_empty()
            && sample.pcm.len().saturating_add(sample.loop_pcm.len()) <= 32_000_000,
        "invalid standalone instrument sample"
    );
    resonance_audio::resample::SampleCursor::new(sample)?;
    let directory = output.join("audio/samples");
    fs::create_dir_all(&directory)?;
    let temporary = crate::temporary_path(&directory.join("sample.wav"));
    write_pcm16(
        &temporary,
        1,
        u32::from(sample.rate),
        sample.pcm.iter().chain(&sample.loop_pcm).copied(),
    )?;
    let sha256 = hash_file(&temporary)?;
    let path = format!("audio/samples/{sha256}.wav");
    let target = output.join(&path);
    let result = crate::publication::publish(&target, &sha256, || {
        match fs::hard_link(&temporary, &target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                ensure!(hash_file(&target)? == sha256, "corrupt shared audio sample");
                Ok(())
            }
            Err(error) => Err(error).context("publish shared audio sample"),
        }
    });
    fs::remove_file(temporary)?;
    result?;
    Ok(SampleAsset {
        path,
        sha256,
        key: sample.key,
        rate: sample.rate,
        first_frames: u32::try_from(sample.pcm.len())?,
        loop_start: sample.loop_start,
        loop_length: sample.loop_length,
    })
}

fn valid_asset(output: &Path, asset: &Value) -> bool {
    let (Some(path), Some(hash)) = (asset["path"].as_str(), asset["sha256"].as_str()) else {
        return false;
    };
    resonance_content::validate_asset_path(path).is_ok()
        && hash_file(&output.join(path)).is_ok_and(|actual| actual == hash)
}

fn wav_frames(path: &Path, rate: u32, maximum: u32) -> Result<u32> {
    validate_wave(path, rate, maximum, false)
}

fn validate_wave(path: &Path, rate: u32, maximum: u32, allow_float: bool) -> Result<u32> {
    let mut wave = hound::WavReader::open(path)?;
    let spec = wave.spec();
    let frames = wave.duration();
    ensure!(
        spec.channels == 2
            && spec.sample_rate == rate
            && ((spec.bits_per_sample == 16 && spec.sample_format == hound::SampleFormat::Int)
                || (allow_float
                    && spec.bits_per_sample == 32
                    && spec.sample_format == hound::SampleFormat::Float))
            && (1..=maximum).contains(&frames),
        "unexpected PCM format or duration in {}",
        path.display()
    );
    let mut audible = false;
    if spec.sample_format == hound::SampleFormat::Float {
        for sample in wave.samples::<f32>() {
            let sample = sample?;
            ensure!(sample.is_finite(), "nonfinite rendered audio sample");
            audible |= sample != 0.0;
        }
    } else {
        for sample in wave.samples::<i16>() {
            audible |= sample? != 0;
        }
    }
    ensure!(audible, "rendered audio is silent: {}", path.display());
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_session_shares_discs_and_releases_only_its_last_owner() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let discs = [
            temporary.path().join("disc1"),
            temporary.path().join("disc2"),
        ];
        for (disc, header) in discs.iter().zip([b"GQSEAF\0\0", b"GQSEAF\x01\0"]) {
            fs::create_dir_all(disc.join("sys"))?;
            fs::write(disc.join("sys/boot.bin"), header)?;
        }
        let output = temporary.path().join("cooked");
        let lock = output.join(".cook-media.lock");
        let session = OutputSession::open(&output)?;
        let first = session.workspace(&discs[0])?;
        let second = session.workspace(&discs[1])?;
        assert_eq!(
            (
                crate::disc_number(&first.extracted)?,
                crate::disc_number(&second.extracted)?
            ),
            (1, 2)
        );
        assert_eq!(first.output, second.output);
        assert_ne!(first.extracted, second.extracted);
        let error = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    Workspace::open(&discs[1], &output)
                        .err()
                        .unwrap()
                        .to_string()
                })
                .join()
                .unwrap()
        });
        assert!(error.contains("another media cook owns"));
        assert!(session.workspace(&temporary.path().join("absent")).is_err());
        drop(session);
        drop(first);
        assert!(lock.exists());
        assert!(OutputSession::open(&output).is_err());
        drop(second);
        assert!(!lock.exists());

        // A failed constructor must release a newly acquired standalone session.
        assert!(Workspace::open(&temporary.path().join("absent"), &output).is_err());
        assert!(!lock.exists());
        let failed_cook = || -> Result<()> {
            let session = OutputSession::open(&output)?;
            let _first = session.workspace(&discs[0])?;
            let _second = session.workspace(&discs[1])?;
            anyhow::bail!("conversion failed")
        };
        assert!(failed_cook().is_err());
        assert!(!lock.exists());
        let next = Workspace::open(&discs[0], &output)?;
        assert!(lock.exists());
        drop(next);
        assert!(!lock.exists());
        Ok(())
    }
}
