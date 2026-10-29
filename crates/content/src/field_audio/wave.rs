use super::Voice;
use crate::library::validate_path;
use anyhow::{Result, ensure};
use std::io::Cursor;

impl Voice {
    /// Infer disposable playback metadata from the actual audio bytes.
    pub fn from_wave(path: &str, sample_rate: Option<u32>, bytes: &[u8]) -> Result<Voice> {
        validate_path(path)?;
        let wave = hound::WavReader::new(Cursor::new(bytes))?;
        let spec = wave.spec();
        ensure!(
            spec.sample_format == hound::SampleFormat::Int && spec.bits_per_sample == 16,
            "speech requires PCM16 WAV: {path}"
        );
        let voice = Voice {
            path: path.into(),
            frames: wave.duration(),
            sample_rate: sample_rate.unwrap_or(spec.sample_rate),
            source_sample_rate: spec.sample_rate,
            channels: spec.channels,
        };
        voice.validate()?;
        Ok(voice)
    }
}
