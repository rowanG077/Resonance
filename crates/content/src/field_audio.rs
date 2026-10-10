//! Cooked music, cue, and spoken-line references for a field route.
pub mod archive;
use crate::diagnostics::Diagnostics;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;

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
}
impl Voice {
    pub fn validate(&self) -> Result<()> {
        self.asset.validate()?;
        ensure!(
            (1..=2).contains(&self.channels)
                && (8000..=96000).contains(&self.sample_rate)
                && (8000..=96000).contains(&self.source_sample_rate)
                && (1..=32_000_000).contains(&self.frames),
            "invalid voice format or length"
        );
        Ok(())
    }
}
/// Prepared music reverb choices. A zero selector leaves the effect unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicReverbs {
    pub presets: [[f32; 5]; 2],
    pub selectors: Vec<u8>,
}
impl MusicReverbs {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.selectors.iter().all(|&preset| preset <= 2),
            "invalid music reverb selector table"
        );
        for parameters in self.presets {
            Self::validate_preset(parameters)?;
        }
        Ok(())
    }

    fn validate_preset(parameters: [f32; 5]) -> Result<[f32; 5]> {
        for (value, (low, high)) in
            parameters
                .into_iter()
                .zip([(0., 1.), (0., 1.), (0.01, 10.), (0., 1.), (0., 0.1)])
        {
            ensure!(
                value.is_finite() && (low..=high).contains(&value),
                "invalid music reverb parameter"
            );
        }
        Ok(parameters)
    }

    pub fn for_song(&self, song: i16) -> Result<Option<[f32; 5]>> {
        let preset = usize::try_from(song)
            .ok()
            .and_then(|song| self.selectors.get(song))
            .copied();
        let Some(preset) = preset else {
            anyhow::bail!("music reverb song is outside the table");
        };
        if preset == 0 {
            return Ok(None);
        }
        ensure!((1..=2).contains(&preset), "invalid music reverb preset");
        Self::validate_preset(self.presets[usize::from(preset - 1)]).map(Some)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldAudio<Scores = BTreeMap<i16, Asset>, Voices = BTreeMap<u32, Voice>> {
    pub version: u32,
    pub music_reverbs: MusicReverbs,
    pub music: Scores,
    pub sounds: Scores,
    pub voices: Voices,
    /// Linear PCM gain for each saved dialogue-volume setting (0..=127).
    #[serde(default)]
    pub voice_gains: Vec<f32>,
}
impl FieldAudio {
    pub const VERSION: u32 = 4;

    pub fn validate(&self) -> Result<()> {
        self.validate_header()?;
        self.music_reverbs.validate()?;
        self.validated_voice_gains()?;
        for asset in self.music.values().chain(self.sounds.values()) {
            asset.validate()?;
        }
        for voice in self.voices.values() {
            voice.validate()?;
        }
        Ok(())
    }
}
impl<Scores, Voices> FieldAudio<Scores, Voices> {
    /// Bank-wide format; individual resources are validated independently.
    pub fn validate_header(&self) -> Result<()> {
        ensure!(
            self.version == FieldAudio::VERSION,
            "invalid field audio manifest; recook field audio"
        );
        Ok(())
    }

    pub fn validated_voice_gains(&self) -> Result<[f32; 128]> {
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
        Ok(self.voice_gains.as_slice().try_into().unwrap())
    }
}

/// Keep schema failures with the cached snapshot so every admission applies its policy.
#[derive(Debug, Clone)]
pub struct DecodedBank {
    contents: FieldAudio,
    errors: Vec<String>,
}
impl<'de> serde::Deserialize<'de> for DecodedBank {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> std::result::Result<Self, D::Error> {
        type Rows = BTreeMap<String, serde_json::Value>;
        let raw = <FieldAudio<Rows, Rows> as serde::Deserialize>::deserialize(decoder)?;
        let mut errors = Vec::new();
        Ok(Self {
            contents: FieldAudio {
                version: raw.version,
                music_reverbs: raw.music_reverbs,
                music: decode_entries(raw.music, "music", &mut errors),
                sounds: decode_entries(raw.sounds, "sound", &mut errors),
                voices: decode_entries(raw.voices, "voice", &mut errors),
                voice_gains: raw.voice_gains,
            },
            errors,
        })
    }
}
impl DecodedBank {
    pub fn checked(&self, diagnostics: &Diagnostics) -> Result<&FieldAudio> {
        for error in &self.errors {
            diagnostics.report("field audio entry", anyhow::anyhow!("{error}"))?;
        }
        Ok(&self.contents)
    }
}
fn decode_entries<K, V>(
    rows: BTreeMap<String, serde_json::Value>,
    kind: &str,
    errors: &mut Vec<String>,
) -> BTreeMap<K, V>
where
    K: Ord + std::str::FromStr + std::fmt::Display,
    K::Err: std::fmt::Display,
    V: serde::de::DeserializeOwned,
{
    rows.into_iter()
        .filter_map(|(key, value)| {
            let decoded = (|| -> Result<_> {
                let id = key
                    .parse::<K>()
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                ensure!(key == id.to_string(), "noncanonical audio ID {key}");
                Ok((id, serde_json::from_value::<V>(value)?))
            })();
            match decoded {
                Ok(entry) => Some(entry),
                Err(error) => {
                    errors.push(format!("{kind} {key}: {error}"));
                    None
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_reverb_selection_validates_only_the_requested_preset() {
        let mut catalog = MusicReverbs {
            presets: [
                [0.8, 0.7, 3.6, 0.6, 1. / 32000.],
                [0.7, 0.7, 2.5, 0.6, 0.05],
            ],
            selectors: vec![0, 1, 2],
        };
        catalog.validate().unwrap();
        assert_eq!(catalog.for_song(0).unwrap(), None);
        assert_eq!(catalog.for_song(1).unwrap(), Some(catalog.presets[0]));
        assert_eq!(catalog.for_song(2).unwrap(), Some(catalog.presets[1]));
        assert!(catalog.for_song(-1).is_err());
        assert!(catalog.for_song(3).is_err());

        catalog.selectors.push(3);
        catalog.presets[1][0] = f32::NAN;
        assert!(catalog.validate().is_err());
        assert!(catalog.for_song(2).is_err());
        assert!(catalog.for_song(3).is_err());
        assert_eq!(catalog.for_song(0).unwrap(), None);
        assert_eq!(catalog.for_song(1).unwrap(), Some(catalog.presets[0]));
        catalog.presets[0][4] = 0.11;
        assert!(catalog.for_song(1).is_err());
    }
}
