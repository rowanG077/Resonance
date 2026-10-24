//! Offline media conversion. Rust owns parsing, validation, and conversion;
//! pure Rust codecs run without opening an audio output device.
pub(crate) mod adx;
pub(crate) mod field_audio;
pub(crate) mod library;
mod movie;
pub(crate) mod music;
pub(crate) mod voice_library;
/// Original inline metadata for bounded descriptor-only preparation.
pub use music::music_reverbs;
pub(crate) mod music_library;
mod music_score;
mod music_voice;
mod pitched_sample;
mod sound_buses;
pub(crate) mod sound_library;
pub(crate) mod source;

pub(crate) use field_audio::{VoiceFormat, music_path, sound_score};
pub(crate) use music::song_reverb_change;
pub(crate) use music_voice::tables as synthesis_tables;

pub(crate) use movie::catalogue as movie_catalogue;
pub(crate) use movie::cook_directory as cook_movie_directory;
pub(crate) use movie::cook_movie_file;
pub(crate) use movie::input_identity as movie_input_identity;
pub(crate) use movie::is_movie;
pub use music_score::inspect_title_audio;
pub use music_voice::{MusicVoiceOptions, render_music_voice, render_title_audio_preview};
pub use pitched_sample::{PitchedSampleOptions, render_pitched_sample};
pub use sound_buses::{render_sound_buses, render_sound_sequence};

use crate::read::u32 as be_u32;
use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) const SAMPLE_RATE: u32 = 32_000;
// MusyX synthesizes 160-sample nominal 5 ms blocks. GameCube's DAC runs
// slightly faster: 108 MHz / (1124 * 3), recorded by Dolphin as 32,028 Hz.
// Preserve the sample sequence and label its playback clock; do not resample.
pub(crate) const PLAYBACK_RATE: u32 = 32_028;

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

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut data = serde_json::to_vec_pretty(value)?;
    data.push(b'\n');
    crate::write_atomic(path, &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn audio_tables() -> resonance_audio::music_voice::Tables {
        use resonance_audio::{dls, mix, modulation, music_voice, resample};
        music_voice::Tables {
            mix: mix::Tables {
                volume: [1.; 129],
                alternate_volume: [1.; 129],
                pan: [1.; 4],
                volume_16_scale: 1.,
                controller_14_scale: 1.,
                pan_16_scale: 1.,
                spatial: None,
            },
            dls: dls::Tables {
                attenuation: [0; 194],
            },
            modulation: modulation::Tables {
                sine: [0; 1024],
                tremolo: [1.; 5],
            },
            coefficients: resample::Coefficients([[[0; 4]; 128]; 4]),
        }
    }

    #[test]
    fn generated_interpolation_coefficients_are_stable() {
        // Guard quantization and platform-dependent table changes in CI.
        assert_eq!(
            crate::digest(&resonance_audio_cook::interpolation::coefficients()),
            "b5683269fba74a145778697b88961e17e857dd35f13c06af63524e71c7a1de4f"
        );
    }

    #[test]
    fn named_samples_preserve_existing_edits_and_publish_pcm() -> Result<()> {
        let output = tempfile::tempdir()?;
        let sample = resonance_audio::sample::Sample {
            key: 60,
            rate: 32000,
            loop_start: 0,
            loop_length: 0,
            pcm: vec![7, 11, 13],
            loop_pcm: Vec::new(),
        };
        let name = "audio/instruments/effects/example/sample-00001.wav";
        {
            let _session = crate::publication::Session::start(output.path())?;
            let mut publisher = crate::named::Publisher::new(output.path(), output.path())?;
            let asset = publisher.native_sample(&sample, name)?;
            assert_eq!(asset.path, name);
            assert_eq!(asset.key, sample.key);
            assert_eq!(publisher.native_sample(&sample, name)?.path, name);
            let mut wave = hound::WavReader::open(output.path().join(name))?;
            assert_eq!(wave.spec().sample_rate, u32::from(sample.rate));
            assert_eq!(
                wave.samples::<i16>().collect::<Result<Vec<_>, _>>()?,
                sample.pcm
            );
        }
        let edited = b"user edited this sample";
        fs::write(output.path().join(name), edited)?;
        let _session = crate::publication::Session::start(output.path())?;
        let mut publisher = crate::named::Publisher::new(output.path(), output.path())?;
        assert!(publisher.native_sample(&sample, name).is_err());
        assert_eq!(fs::read(output.path().join(name))?, edited);
        Ok(())
    }

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
