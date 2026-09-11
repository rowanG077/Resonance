//! Prepared skit availability and notification text, independent of save data.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkitCatalog {
    pub version: u32,
    pub skits: Vec<SkitDefinition>,
    /// Cooked VM scenario and message resources keyed by skit ID.
    #[serde(default)]
    pub resources: BTreeMap<u16, SkitResourcePaths>,
    #[serde(default)]
    pub portraits: BTreeMap<u32, PortraitAsset>,
    #[serde(default)]
    pub media: BTreeMap<u32, SkitMedia>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkitResourcePaths {
    pub script: String,
    pub messages: String,
}

/// Precomposed expression frames: patches replace pixels, including alpha.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortraitAsset {
    pub layout_sha256: String,
    pub texture: String,
    pub size: [u32; 2],
    pub atlas_size: [u32; 2],
    pub tracks: [Vec<PortraitFrame>; 3],
    pub repeat: [bool; 3],
    pub variants: Vec<PortraitVariant>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortraitFrame {
    pub ticks: u16,
    pub image: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortraitVariant {
    pub images: [u16; 3],
    pub rect: [u32; 4],
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
        ensure!(self.version == 1, "unsupported skit catalog");
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
            crate::validate_asset_path(&portrait.texture)?;
            ensure!(
                portrait.size.iter().all(|&v| v > 0 && v <= 1024)
                    && portrait.atlas_size.iter().all(|&v| v > 0 && v <= 8192)
                    && !portrait.variants.is_empty()
                    && portrait.variants.len() <= 512,
                "invalid skit portrait dimensions"
            );
            ensure!(
                portrait
                    .tracks
                    .iter()
                    .all(|track| track.len() <= 64 && track.iter().all(|frame| frame.ticks < 253)),
                "invalid portrait timeline"
            );
            for variant in &portrait.variants {
                let [x, y, w, h] = variant.rect;
                ensure!(
                    [w, h] == portrait.size
                        && x + w <= portrait.atlas_size[0]
                        && y + h <= portrait.atlas_size[1],
                    "portrait frame outside atlas"
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
