//! Editable title artwork, background selection and menu audio references.
use crate::{BootAssets, library::Snapshot, preparation};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PATH: &str = "ui/title/title.json";
pub const BOOT: &str = "ui/boot/sequence.json";
pub mod boot;
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub day_field: u32,
    pub night_field: u32,
    pub artwork: Vec<Artwork>,
    pub music: String,
    pub cues: String,
}

/// One centered title image, positioned from the top of the 640 × 480 canvas.
/// Choices bind to menu behavior, independently of image order or source banks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artwork {
    pub image: String,
    pub top: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<Choice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_image: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    NewGame,
    Continue,
    Customize,
}

impl Choice {
    pub fn row(self) -> usize {
        match self {
            Self::NewGame => 0,
            Self::Continue => 1,
            Self::Customize => 2,
        }
    }
}

impl Artwork {
    pub fn images(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.image.as_str()).chain(self.selected_image.as_deref())
    }
}

impl Definition {
    pub fn validate(&self) -> Result<()> {
        crate::library::validate_path(&self.music)?;
        crate::library::validate_path(&self.cues)?;
        for artwork in &self.artwork {
            ensure!(artwork.top.is_finite(), "invalid title image position");
            ensure!(
                artwork.selected_image.is_none() || artwork.choice.is_some(),
                "a selected title image needs a menu choice"
            );
            for path in artwork.images() {
                crate::library::validate_path(path)?;
            }
        }
        Ok(())
    }
}

/// Cue names are the menu controller's interface; these are direct paths to
/// editable programs, not another ID catalogue or a dependency inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cues<Entry = String> {
    pub version: u32,
    /// Override the common field/menu acoustics from audio/mix.json.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::field_audio::reverb::optional"
    )]
    pub reverbs: Option<[[f32; 5]; 2]>,
    pub cues: BTreeMap<String, Entry>,
}

pub fn capture(
    snapshot: &mut Snapshot,
    boot: bool,
    cancelled: &impl Fn() -> bool,
) -> Result<(Definition, Option<BootAssets>)> {
    ensure!(!cancelled(), "title preparation cancelled");
    let definition: Definition = snapshot.json(PATH, preparation::JSON_LIMIT)?;
    definition.validate()?;
    for path in definition.artwork.iter().flat_map(Artwork::images) {
        ensure!(!cancelled(), "title preparation cancelled");
        snapshot.read(path, preparation::ASSET_LIMIT)?;
    }
    let boot = if boot {
        let boot: BootAssets = snapshot.json(BOOT, preparation::JSON_LIMIT)?;
        boot.duration()?;
        for logo in &boot.0 {
            ensure!(!cancelled(), "title preparation cancelled");
            snapshot.read(&logo.image, preparation::ASSET_LIMIT)?;
        }
        Some(boot)
    } else {
        None
    };
    ensure!(!cancelled(), "title preparation cancelled");
    Ok((definition, boot))
}

#[cfg(test)]
mod tests;
