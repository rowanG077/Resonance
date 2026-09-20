//! Prepared skit availability and notification text, independent of save data.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const TILE_SIZE: u32 = 8;
pub const MAX_PORTRAITS: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkitCatalog {
    pub version: u32,
    pub skits: Vec<SkitDefinition>,
    /// Cooked VM scenario and message resources keyed by skit ID.
    #[serde(default)]
    pub resources: BTreeMap<u16, SkitResourcePaths>,
    #[serde(default)]
    pub portraits: BTreeMap<u32, PortraitAsset>,
    pub portrait_recipes: Vec<PortraitRecipe>,
    #[serde(default)]
    pub media: BTreeMap<u32, SkitMedia>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkitResourcePaths {
    pub script: String,
    pub messages: String,
}

/// Original images stay independent; each portrait maintains its own tile canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortraitAsset {
    pub size: [u32; 2],
    pub images: Vec<PortraitImage>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortraitImage {
    pub texture: String,
    pub size: [u32; 2],
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortraitRecipe {
    pub tracks: [Vec<PortraitFrame>; 3],
    pub repeat: [bool; 3],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortraitFrame {
    pub ticks: i16,
    pub image: Option<u16>,
    pub position: [i16; 2],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortraitTile {
    pub image: u16,
    pub block: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkitMedia {
    /// Silent timing tracks need no audio payload. Cooking verifies their PCM.
    pub voice: Option<crate::field_audio::Voice>,
    pub frames: u32,
    pub sample_rate: u32,
    pub source_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkitDefinition {
    pub id: u16,
    pub title: String,
    /// Inclusive story-counter range; None means any story progress.
    pub story: Option<[i32; 2]>,
    pub party_mask: u16,
    pub location: SkitLocation,
    pub condition: SkitCondition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkitLocation {
    Anywhere,
    Map(u16),
    Field,
    Overworld(Option<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkitCondition {
    None,
    Maps([u16; 2]),
    Unimplemented,
}

impl SkitCatalog {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 2, "unsupported skit catalog");
        ensure!(self.skits.len() <= 512, "too many skit definitions");
        let mut previous = 0;
        for skit in &self.skits {
            ensure!(
                skit.id > previous
                    && ((1..120).contains(&skit.id) || (600..859).contains(&skit.id))
                    && !skit.title.is_empty()
                    && skit.title.chars().count() <= 80
                    && !skit.title.chars().any(char::is_control)
                    && skit.story.is_none_or(|[start, end]| start <= end)
                    && skit.party_mask & !0x3fe == 0
                    && !matches!(skit.condition, SkitCondition::Maps([start, end]) if start > end)
                    && !matches!(skit.location, SkitLocation::Overworld(Some(2..))),
                "invalid skit definition {}",
                skit.id
            );
            previous = skit.id;
        }
        for (&id, resource) in &self.resources {
            crate::validate_asset_path(&resource.script)?;
            crate::validate_asset_path(&resource.messages)?;
            ensure!(
                self.skits.iter().any(|skit| skit.id == id)
                    && resource.script.starts_with("game/skits/")
                    && resource.messages.starts_with("game/skits/"),
                "invalid skit resource {id}"
            );
        }
        for portrait in self.portraits.values() {
            ensure!(
                portrait.size.iter().all(|&v| v > 0 && v <= 1024)
                    && portrait
                        .images
                        .first()
                        .is_some_and(|image| image.size == portrait.size),
                "invalid skit portrait dimensions"
            );
            for image in &portrait.images {
                crate::validate_asset_path(&image.texture)?;
                ensure!(
                    image.size.iter().all(|&size| size > 0 && size <= 1024),
                    "invalid portrait image dimensions"
                );
            }
        }
        ensure!(
            self.media
                .values()
                .all(|m| m.frames > 0 && (8000..=96000).contains(&m.sample_rate)),
            "invalid skit media clock"
        );
        Ok(())
    }
}
