//! Offline media recipes. Rust owns parsing, validation, caching, and conversion;
//! pure Rust codecs run without opening an audio output device.
mod cooked_music;
mod field_audio;
pub use field_audio::cook_field_audio;
mod movie;
mod music;
mod music_score;
mod music_voice;
mod pitched_sample;
mod sound_buses;
mod sounds;

pub use cooked_music::{cook_title_audio, render_cooked_title_audio};
pub use movie::{MovieSource, cook_movie};
pub use music_score::inspect_title_audio;
pub use music_voice::{MusicVoiceOptions, render_music_voice, render_title_audio_preview};
pub use pitched_sample::{PitchedSampleOptions, render_pitched_sample};
pub use sound_buses::{render_sound_buses, render_sound_sequence};
pub use sounds::cook_title_sounds;

use crate::read::u32 as be_u32;
use anyhow::{Context, Result, ensure};
use resonance_audio::{data::Resources, package::SampleAsset};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const SAMPLE_RATE: u32 = 32_000;
// MusyX synthesizes 160-sample nominal 5 ms blocks. GameCube's DAC runs
// slightly faster: 108 MHz / (1124 * 3), recorded by Dolphin as 32,028 Hz.
// Preserve the sample sequence and label its playback clock; do not resample.
const PLAYBACK_RATE: u32 = 32_028;

pub(crate) struct Tool {
    pub(crate) path: PathBuf,
    pub(crate) hash: String,
}

impl Tool {
    pub(crate) fn resolve(name: &Path) -> Result<Self> {
        let path = if name.components().count() > 1 || name.is_absolute() {
            name.to_path_buf()
        } else {
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .flat_map(|directory| {
                    let path = directory.join(name);
                    #[cfg(windows)]
                    {
                        vec![path.clone(), path.with_extension("exe")]
                    }
                    #[cfg(not(windows))]
                    {
                        vec![path]
                    }
                })
                .find(|path| path.is_file())
                .with_context(|| format!("{} is missing; enter nix develop", name.display()))?
        };
        let path = path.canonicalize()?;
        let hash = hash_file(&path)?;
        Ok(Self { path, hash })
    }
}

pub(crate) struct Workspace {
    extracted: PathBuf,
    output: PathBuf,
    lock: PathBuf,
}

impl Workspace {
    pub(crate) fn open(extracted: &Path, output: &Path) -> Result<Self> {
        let extracted = extracted
            .canonicalize()
            .context("missing extracted disc directory")?;
        let boot = fs::read(extracted.join("sys/boot.bin"))?;
        ensure!(
            boot.get(..6) == Some(b"GQSEAF") && boot.get(6) == Some(&0) && boot.get(7) == Some(&0),
            "expected GQSEAF revision 0 disc 1"
        );
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
        Ok(Self {
            extracted,
            output,
            lock,
        })
    }

    fn intermediate(&self, relative: &str) -> Result<PathBuf> {
        let path = self.output.join("intermediate").join(relative);
        fs::create_dir_all(&path)?;
        Ok(path)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock);
    }
}

pub(crate) fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
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

/// Write only the supplied path; callers validate temporary files before publishing them.
fn write_pcm16(
    path: &Path,
    channels: u16,
    sample_rate: u32,
    samples: impl IntoIterator<Item = i16>,
) -> Result<()> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

fn write_sample_assets(
    output: &Path,
    resources: &Resources,
    name: impl Fn(u16) -> String,
) -> Result<BTreeMap<u16, SampleAsset>> {
    resources
        .samples
        .iter()
        .map(|(&id, sample)| {
            let path = name(id);
            let target = output.join(&path);
            let temporary = target.with_extension("partial.wav");
            write_pcm16(
                &temporary,
                1,
                u32::from(sample.rate),
                sample.pcm.iter().chain(&sample.loop_pcm).copied(),
            )?;
            fs::rename(temporary, &target)?;
            Ok((
                id,
                SampleAsset {
                    path,
                    sha256: hash_file(&target)?,
                    key: sample.key,
                    rate: sample.rate,
                    first_frames: sample.pcm.len() as u32,
                    loop_start: sample.loop_start,
                    loop_length: sample.loop_length,
                },
            ))
        })
        .collect()
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

// Bump whenever AHX output semantics change; invalidates voice and skit caches.
pub(crate) const AHX_DECODER: &str = "ahx-mpg123-neon64-pcm16-v1";
