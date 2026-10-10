//! Prepared battle scores and spoken PCM.
use crate::{
    diagnostics::Diagnostics,
    field_audio::{Asset, DecodedBank, FieldAudio},
    field_preload::{File, Role},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PATH: &str = "battle/audio.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Audio<Assets = FieldAudio, Entry = File> {
    pub assets: Assets,
    /// Projected screen center, distance divisor, centered pan, and pan bounds.
    pub effect_spatial: [f32; 5],
    pub voice_spatial: [f32; 5],
    pub files: BTreeMap<String, Entry>,
}

impl Audio<DecodedBank, serde_json::Value> {
    /// Admit each independent metadata row under the current session policy.
    pub fn checked(self, diagnostics: &Diagnostics) -> Result<Option<Audio>> {
        let mut assets = self.assets.checked(diagnostics)?.clone();
        if diagnostics
            .attempt("battle audio schema", assets.validate_header())?
            .is_none()
        {
            return Ok(None);
        }
        let mut files = BTreeMap::new();
        for (path, value) in self.files {
            let entry = (|| -> Result<File> {
                let file = serde_json::from_value(value)?;
                validate_dependency(&path, &file)?;
                Ok(file)
            })();
            if let Some(file) = diagnostics.attempt(
                "battle audio dependency",
                entry.map_err(|error| error.context(format!("battle audio dependency {path}"))),
            )? {
                files.insert(path, file);
            }
        }
        for entries in [&mut assets.music, &mut assets.sounds] {
            *entries = std::mem::take(entries)
                .into_iter()
                .filter_map(|(id, asset)| {
                    diagnostics
                        .attempt(
                            "battle audio package",
                            validate_binding(&files, &asset, Role::AudioPackage)
                                .map(|()| (id, asset)),
                        )
                        .transpose()
                })
                .collect::<Result<_>>()?;
        }
        assets.voices = assets
            .voices
            .into_iter()
            .filter_map(|(id, voice)| {
                diagnostics
                    .attempt(
                        "battle spoken line",
                        voice
                            .validate()
                            .and_then(|()| validate_binding(&files, &voice.asset, Role::Voice))
                            .map(|()| (id, voice)),
                    )
                    .transpose()
            })
            .collect::<Result<_>>()?;
        Ok(Some(Audio {
            assets,
            files,
            effect_spatial: self.effect_spatial,
            voice_spatial: self.voice_spatial,
        }))
    }
}

fn validate_dependency(path: &str, file: &File) -> Result<()> {
    file.validate(path)?;
    ensure!(
        file.bytes > 0 && !file.roles.contains(&Role::Movie),
        "invalid battle audio dependency {path}"
    );
    Ok(())
}

fn validate_binding(files: &BTreeMap<String, File>, asset: &Asset, role: Role) -> Result<()> {
    asset.validate()?;
    ensure!(
        files
            .get(&asset.path)
            .is_some_and(|file| file.sha256 == asset.sha256 && file.roles.contains(&role)),
        "missing battle audio binding {}",
        asset.path
    );
    Ok(())
}

impl Audio {
    pub fn validate(&self) -> Result<()> {
        self.assets.validate()?;
        for controls in [self.effect_spatial, self.voice_spatial] {
            spatial_controls(controls)?;
        }
        for (path, file) in &self.files {
            validate_dependency(path, file)?;
        }
        for asset in self
            .assets
            .music
            .values()
            .chain(self.assets.sounds.values())
        {
            validate_binding(&self.files, asset, Role::AudioPackage)?;
        }
        for voice in self.assets.voices.values() {
            validate_binding(&self.files, &voice.asset, Role::Voice)?;
        }
        Ok(())
    }
}

pub fn spatial_controls(controls: [f32; 5]) -> Result<[f32; 5]> {
    let [origin, divisor, center, low, high] = controls;
    ensure!(
        controls.iter().all(|v| v.is_finite())
            && origin > 0.
            && divisor > 0.
            && low >= 0.
            && low <= center
            && center <= high
            && high <= 127.,
        "invalid battle spatial controls"
    );
    Ok(controls)
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
                music_reverbs: crate::field_audio::MusicReverbs {
                    presets: [[0.5, 0.5, 1., 0.5, 0.]; 2],
                    selectors: vec![1; 112],
                },
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
                    },
                )]
                .into(),
                voice_gains: (0..128).map(|i| i as f32 / 127.).collect(),
            },
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
    fn battle_voice_inventory_binds_digest_and_role() {
        let mut value = audio();
        value.validate().unwrap();
        value.files.clear();
        assert!(value.validate().is_err());
        value = audio();
        value.files.get_mut("audio/line.wav").unwrap().sha256 = "c".repeat(64);
        assert!(value.validate().is_err());
        value = audio();
        value.files.get_mut("audio/line.wav").unwrap().roles = [Role::Data].into();
        assert!(value.validate().is_err());
    }
    #[test]
    fn prepared_pcm_needs_no_score_or_stream_tables() {
        let mut value = audio();
        let voice = value.assets.voices.get_mut(&1).unwrap();
        voice.channels = 2;
        voice.sample_rate = 48000;
        value.validate().unwrap();
        let round_trip: Audio =
            serde_json::from_value(serde_json::to_value(value).unwrap()).unwrap();
        round_trip.validate().unwrap();
    }
}
