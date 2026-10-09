//! Cooked music, cue, and spoken-line references for a field route.
pub mod archive;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Sounds emitted by native field services rather than scenario instructions.
/// The cooker includes this catalogue in every field; service dispatch rejects
/// undeclared IDs so adding a native cue cannot bypass resource preparation.
#[repr(i16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceCue {
    Navigate = 1,
    Confirm = 2,
    Cancel = 3,
    Error = 4,
    TreasureReward = 6,
    TreasureOpen = 28,
    TreasureBag = 29,
    Door = 30,
    RingPrepare = 31,
    RingFire = 32,
    RingElectric = 41,
    RingLightning = 42,
    RingIce = 43,
    RingShatter = 191,
    RingEarthquake = 272,
    RingDarkness = 236,
    RingSound = 129,
    RingBubble = 289,
    RingWind = 87,
    RingWater = 110,
    RingBombPlace = 107,
    RingBombBlast = 194,
    RingBombDebris = 330,
    RingShrink = 287,
    RingMana = 270,
    RingRadar = 263,
    RingSplash = 414,
    MenuOpen = 33,
    Page = 38,
    Recovery = 104,
    Remedy = 132,
}
impl ServiceCue {
    pub const ALL: &[Self] = &[
        Self::Navigate,
        Self::Confirm,
        Self::Cancel,
        Self::Error,
        Self::TreasureReward,
        Self::TreasureOpen,
        Self::TreasureBag,
        Self::Door,
        Self::RingPrepare,
        Self::RingFire,
        Self::RingElectric,
        Self::RingLightning,
        Self::RingIce,
        Self::RingShatter,
        Self::RingEarthquake,
        Self::RingDarkness,
        Self::RingSound,
        Self::RingBubble,
        Self::RingWind,
        Self::RingWater,
        Self::RingBombPlace,
        Self::RingBombBlast,
        Self::RingBombDebris,
        Self::RingShrink,
        Self::RingMana,
        Self::RingRadar,
        Self::RingSplash,
        Self::MenuOpen,
        Self::Page,
        Self::Recovery,
        Self::Remedy,
    ];

    pub fn from_id(id: i16) -> Option<Self> {
        Self::ALL.iter().copied().find(|cue| *cue as i16 == id)
    }
}

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
    /// Playback clock selected by the caller, without resampling shared PCM.
    pub sample_rate: u32,
    /// Native sample rate stored in the shared WAV header.
    pub source_sample_rate: u32,
    pub channels: u16,
    pub source_name: String,
    pub source_sha256: String,
}
impl Voice {
    pub fn validate(&self) -> Result<()> {
        self.asset.validate()?;
        ensure!(
            (1..=2).contains(&self.channels)
                && (8000..=96000).contains(&self.sample_rate)
                && (8000..=96000).contains(&self.source_sample_rate)
                && (1..=32_000_000).contains(&self.frames)
                && !self.source_name.is_empty()
                && self.source_name.len() <= 32
                && !self.source_name.contains(['/', '\\'])
                && self.source_sha256.len() == 64
                && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid voice format or length"
        );
        Ok(())
    }
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
}
impl FieldAudio {
    pub const VERSION: u32 = 3;

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
            voice.validate()?;
        }
        Ok(())
    }
}
