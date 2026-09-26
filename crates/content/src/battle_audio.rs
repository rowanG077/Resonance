//! Prepared battle mixer resources, independent of their original storage banks.
use crate::{
    field_audio::FieldAudio,
    field_preload::{File, Role},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PATH: &str = "battle/audio.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Audio {
    pub assets: FieldAudio,
    /// Original CRI pan lookup, indexed by the quantized -15..=15 pan.
    pub voice_pan: Vec<[f32; 2]>,
    /// Projected screen center, distance divisor, centered pan, and pan bounds.
    pub effect_spatial: [f32; 5],
    pub voice_spatial: [f32; 5],
    pub files: BTreeMap<String, File>,
}

impl Audio {
    pub fn validate(&self) -> Result<()> {
        self.assets.validate()?;
        ensure!(
            self.voice_pan.len() == 31
                && self
                    .voice_pan
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "invalid battle voice pan table"
        );
        for [origin, divisor, center, low, high] in [self.effect_spatial, self.voice_spatial] {
            ensure!(
                [origin, divisor, center, low, high]
                    .iter()
                    .all(|v| v.is_finite())
                    && origin > 0.0
                    && divisor > 0.0
                    && low >= 0.0
                    && low <= center
                    && center <= high
                    && high <= 127.0,
                "invalid battle spatial controls"
            );
        }
        for (path, file) in &self.files {
            crate::validate_asset_path(path)?;
            ensure!(
                file.bytes > 0
                    && file.sha256.len() == 64
                    && file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    && !file.roles.is_empty()
                    && !file.roles.contains(&Role::Movie),
                "invalid battle audio dependency {path}"
            );
        }
        for asset in self
            .assets
            .music
            .values()
            .chain(self.assets.sounds.values())
        {
            ensure!(
                self.files
                    .get(&asset.path)
                    .is_some_and(|file| file.sha256 == asset.sha256
                        && file.roles.contains(&Role::AudioPackage)),
                "missing battle audio package {}",
                asset.path
            );
        }
        for (&id, voice) in &self.assets.voices {
            ensure!(
                id < 0x8000 && voice.sample_rate == voice.source_sample_rate,
                "battle stream must retain its native playback rate"
            );
            ensure!(
                self.files
                    .get(&voice.asset.path)
                    .is_some_and(|file| file.sha256 == voice.asset.sha256
                        && file.roles.contains(&Role::Voice)),
                "missing battle spoken line {}",
                voice.asset.path
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_audio::{Asset, Voice};
    fn audio() -> Audio {
        let asset = Asset {
            path: "audio/line.wav".into(),
            sha256: "a".repeat(64),
        };
        Audio {
            assets: FieldAudio {
                version: FieldAudio::VERSION,
                music: BTreeMap::new(),
                sounds: BTreeMap::new(),
                voices: [(
                    1,
                    Voice {
                        asset: asset.clone(),
                        frames: 2,
                        sample_rate: 22050,
                        source_sample_rate: 22050,
                        channels: 1,
                        source_name: "voice.adx".into(),
                        source_sha256: "b".repeat(64),
                    },
                )]
                .into(),
                voice_gains: (0..128).map(|i| i as f32 / 127.).collect(),
            },
            voice_pan: vec![[1.; 2]; 31],
            effect_spatial: [320., 5., 64., 0., 127.],
            voice_spatial: [320., 5., 64., 24., 104.],
            files: [(
                asset.path,
                File {
                    sha256: asset.sha256,
                    bytes: 48,
                    roles: [Role::Voice].into(),
                },
            )]
            .into(),
        }
    }
    #[test]
    fn battle_voice_inventory_binds_digest_role_and_native_clock() {
        let mut value = audio();
        value.validate().unwrap();
        value.assets.voices.get_mut(&1).unwrap().sample_rate = 32028;
        assert!(value.validate().is_err());
        value = audio();
        value.files.clear();
        assert!(value.validate().is_err());
        value = audio();
        value.files.values_mut().next().unwrap().sha256 = "c".repeat(64);
        assert!(value.validate().is_err());
        value = audio();
        value.files.values_mut().next().unwrap().roles = [Role::Data].into();
        assert!(value.validate().is_err());
    }
}
