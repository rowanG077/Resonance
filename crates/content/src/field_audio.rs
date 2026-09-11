//! Cooked music, cue, and spoken-line references for a field route.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub path: String,
    pub sha256: String,
}
impl Asset {
    pub fn validate(&self) -> Result<()> {
        crate::validate_asset_path(&self.path)?;
        ensure!(
            self.sha256.len() == 64 && self.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid audio digest"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voice {
    #[serde(flatten)]
    pub asset: Asset,
    pub frames: u32,
    pub sample_rate: u32,
    pub source_sample_rate: u32,
    pub channels: u16,
    pub source_name: String,
    pub source_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldAudio {
    pub version: u32,
    pub music: BTreeMap<i16, Asset>,
    pub sounds: BTreeMap<i16, Asset>,
    pub voices: BTreeMap<u32, Voice>,
    /// Linear PCM gain for each saved dialogue-volume setting (0..=127).
    #[serde(default)]
    pub voice_gains: Vec<f32>,
    pub recipe: serde_json::Value,
}
impl FieldAudio {
    pub const VERSION: u32 = 2;

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == Self::VERSION
                && self.music.len() <= 256
                && self.sounds.len() <= 1024
                && self.voices.len() <= 4096,
            "invalid field audio manifest; recook field audio"
        );
        ensure!(
            self.voice_gains.len() == 128
                && self.voice_gains[0] == 0.
                && self.voice_gains[127] == 1.
                && self
                    .voice_gains
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                && self.voice_gains.windows(2).all(|v| v[0] <= v[1]),
            "invalid dialogue volume curve; recook field audio"
        );
        for asset in self.music.values().chain(self.sounds.values()) {
            asset.validate()?;
        }
        for voice in self.voices.values() {
            voice.asset.validate()?;
            ensure!(
                (1..=2).contains(&voice.channels)
                    && (8000..=96000).contains(&voice.sample_rate)
                    && (8000..=96000).contains(&voice.source_sample_rate)
                    && (1..=32_000_000).contains(&voice.frames)
                    && !voice.source_name.is_empty()
                    && voice.source_name.len() <= 32
                    && !voice.source_name.contains(['/', '\\'])
                    && voice.source_sha256.len() == 64
                    && voice.source_sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid voice format or length"
            );
        }
        Ok(())
    }
}
