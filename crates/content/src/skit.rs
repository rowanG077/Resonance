//! Prepared skit availability and notification text, independent of save data.
use crate::overworld::PartyCharacter;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const TILE_SIZE: u32 = 8;
pub const MAX_PORTRAITS: usize = 32;
pub mod document;
pub mod fields;
pub use fields::{FieldRange, FieldRef};

#[derive(Debug, Clone, Serialize)]
pub struct SkitCatalog {
    pub skits: Vec<SkitDefinition>,
    /// Catalogue order for the Katz' Village replay service.
    pub preview_order: Vec<u16>,
    /// Cooked VM scenario and message resources keyed by skit ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resources: BTreeMap<u16, SkitResourcePaths>,
    #[serde(default)]
    pub portraits: BTreeMap<u32, PortraitAsset>,
    pub portrait_recipes: BTreeMap<u16, PortraitRecipe>,
    #[serde(default)]
    pub media: BTreeMap<u32, SkitMedia>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkitResourcePaths {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub script: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
    /// Authored permission, additionally constrained by the playback caller.
    #[serde(default = "allow_skip", skip_serializing_if = "is_allowed")]
    pub allow_skip: bool,
    /// Event-only skits have titles but never produce an ambient notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

fn allow_skip() -> bool {
    true
}
fn is_allowed(value: &bool) -> bool {
    *value
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortraitRecipe {
    pub tracks: [Vec<PortraitFrame>; 3],
    pub repeat: [bool; 3],
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize)]
pub struct SkitMedia {
    /// Silent timing tracks need no audio payload. Cooking verifies their PCM.
    pub voice: Option<crate::field_audio::Voice>,
    pub frames: u32,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkitDefinition {
    pub id: u16,
    pub title: String,
    pub trigger: SkitTrigger,
    /// Inclusive story-counter range; None means any story progress.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub story: Option<[i32; 2]>,
    /// Every listed character must be in the active formation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub party: Vec<PartyCharacter>,
    #[serde(default, skip_serializing_if = "SkitLocation::is_anywhere")]
    pub location: SkitLocation,
    #[serde(default, skip_serializing_if = "SkitCondition::is_unconditional")]
    pub condition: SkitCondition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkitTrigger {
    Entry,
    Exploration,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkitLocation {
    #[default]
    Anywhere,
    #[serde(rename = "field")]
    Map(FieldRef),
    #[serde(rename = "any_field")]
    Field,
    Overworld,
    Sylvarant,
    TetheAlla,
}

impl SkitLocation {
    fn is_anywhere(&self) -> bool {
        matches!(self, Self::Anywhere)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkitCondition {
    #[default]
    None,
    /// A catalogue entry that never announces itself; explicit playback still works.
    Never,
    #[serde(rename = "fields")]
    Maps(FieldRange),
    All(Vec<Self>),
    Any(Vec<Self>),
    Not(Box<Self>),
    Flag(u16),
    Viewed(u16),
    Member(PartyCharacter),
    Item(u16),
    Terrain(crate::overworld::Terrain),
    Range {
        value: SkitValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<i64>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkitValue {
    /// Saved story counter used by the prompt's story range too.
    StoryProgress,
    /// Saved world side: zero is Sylvarant, one is Tethe'alla.
    WorldSide,
    #[serde(rename = "global_word")]
    Global(u8),
    Gald,
    AffinityRank(PartyCharacter),
    Level(PartyCharacter),
    BattleParticipation(PartyCharacter),
    Battles,
    HeadgearCategory(PartyCharacter),
    Title(PartyCharacter),
    RingMode,
    /// Nominal 60 Hz clocks, expressed in whole seconds.
    ExplorationSeconds,
    PlayedSeconds,
    WorldArea,
}

impl SkitCondition {
    fn is_unconditional(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::All(conditions) | Self::Any(conditions) => {
                ensure!(!conditions.is_empty(), "empty skit condition group");
                for condition in conditions {
                    condition.validate()?;
                }
            }
            Self::Not(condition) => condition.validate()?,
            Self::Maps(range) => ensure!(
                range.from.id()? <= range.through.id()?,
                "inverted skit field range"
            ),
            Self::Viewed(id) => ensure!((1..=860).contains(id), "invalid viewed skit"),
            Self::Range { value, min, max } => {
                ensure!(min.is_some() || max.is_some(), "unbounded skit condition");
                ensure!(
                    min.zip(*max).is_none_or(|(a, b)| a <= b),
                    "inverted skit range"
                );
                match value {
                    SkitValue::AffinityRank(id) => {
                        ensure!(
                            *id != PartyCharacter::Lloyd,
                            "Lloyd is excluded from affinity ranking"
                        )
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl SkitCatalog {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.skits.len() <= 512, "too many skit definitions");
        ensure!(
            self.preview_order
                .iter()
                .all(|id| self.resources.contains_key(id)),
            "skit replay catalogue references an unavailable script"
        );
        let mut ids = BTreeSet::new();
        for skit in &self.skits {
            ensure!(
                ids.insert(skit.id)
                    && (1..=860).contains(&skit.id)
                    && !skit.title.is_empty()
                    && skit.title.chars().count() <= 80
                    && !skit.title.chars().any(char::is_control)
                    && skit.story.is_none_or(|[start, end]| start <= end)
                    && skit
                        .party
                        .iter()
                        .enumerate()
                        .all(|(i, member)| !skit.party[..i].contains(member)),
                "invalid skit definition {}",
                skit.id
            );
            skit.condition.validate()?;
        }
        for (&id, resource) in &self.resources {
            crate::validate_asset_path(&resource.script)?;
            if let Some(path) = &resource.messages {
                crate::validate_asset_path(path)?;
            }
            ensure!((1..=860).contains(&id), "invalid skit resource {id}");
            ensure!(
                resource.title.as_ref().is_none_or(
                    |title| title.chars().count() <= 80 && !title.chars().any(char::is_control)
                ),
                "invalid skit resource title {id}"
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
