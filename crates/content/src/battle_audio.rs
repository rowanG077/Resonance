//! Prepared battle scores and spoken PCM.
use crate::{
    diagnostics::Diagnostics,
    field_audio::{Asset, DecodedBank, FieldAudio},
    field_preload::{File, Role},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PATH: &str = "battle/audio.json";

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub sounds: BTreeSet<u16>,
    pub streams: BTreeSet<u16>,
    #[serde(default)]
    pub voices: BTreeSet<u32>,
    pub music: BTreeSet<u16>,
}

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
