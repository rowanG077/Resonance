//! Source rigs shared by combat preparation and the existing model publications.
use crate::animation::Skeleton;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const WEAPONS_PATH: &str = "battle/weapons.json";

/// Cooked attachment descriptors, keyed by equipment item or carried-prop identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weapons {
    pub source_sha256: String,
    pub table_sha256: String,
    pub owner_motion_sha256: String,
    pub records: BTreeMap<u16, Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attachment {
    Weapon(Weapon),
    Shield(Weapon),
    Nonvisual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weapon {
    pub source_sha256: String,
    pub parts: BTreeMap<u8, ModelPart>,
    pub trails: BTreeMap<u8, TrailMaterial>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uv_animation: BTreeMap<u8, AttachmentAnimation>,
    pub files: BTreeMap<String, crate::field_preload::File>,
}

/// Per-part texture channels and periodic effects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentAnimation {
    pub channels: [AttachmentUvChannel; 2],
    pub frame_order: Vec<u8>,
    pub effect_interval: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AttachmentUvChannel {
    pub count: u8,
    pub texture: i8,
    pub mode: AttachmentUvMode,
    pub frames_or_step: i8,
    pub period: i8,
    pub initial_scroll: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentUvMode {
    Disabled,
    Frames,
    ScrollV,
    Reserved,
    Sequence,
    ScrollU,
    Inactive(i8),
}

/// Blade ribbon material; timer/history stay in combat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrailMaterial {
    pub texture: Option<TrailTexture>,
    pub palette: u8,
    pub additive: bool,
    pub color: [u8; 3],
    pub uv: [i16; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum TrailTexture {
    Common { slot: u8 },
    Enemy { slot: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPart {
    pub rig: Rig,
    /// Independent visual skeletons keyed by scene resource. Other layers share the contact rig.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layer_skeletons: BTreeMap<u16, Skeleton>,
    pub layers: Vec<crate::model_preview::PreviewPart>,
}

pub fn enemy_path(id: u8) -> String {
    format!("battle/enemies/{id:03}.json")
}

pub fn enemy_effects_path(id: u8) -> String {
    format!("battle/enemies/{id:03}/effects.json")
}

pub fn party_path(character: u8) -> String {
    party_costume_path(character, 0)
}

pub fn party_costume_path(character: u8, costume: u8) -> String {
    if costume == 0 {
        format!("battle/party/{character:02}.json")
    } else {
        format!("battle/party/{character:02}-{costume}.json")
    }
}

/// A costume's body and independent battle motion bank. Party
/// statistics and actor templates have their existing session/table owners.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Party {
    pub body_sha256: String,
    pub motion_sha256: String,
    /// Explicit empty entries in the authored body animation table.
    #[serde(default)]
    pub null_motions: BTreeSet<u16>,
    pub body: Rig,
    pub parts: Vec<crate::ScenePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wings: Option<crate::ScenePart>,
    pub files: BTreeMap<String, crate::field_preload::File>,
}

/// Optional body and carried artwork. Enemy gameplay has its own definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Enemy {
    pub source_sha256: String,
    /// A separately animated body component, sharing the actor's action clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_body: Option<SecondaryBody>,
    /// Formation-selected vertical atlas rows on the primary body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_variant: Option<crate::battle_profile::TextureChannel>,
    /// Native carried model slots, independently visible and sampled.
    pub attachments: BTreeMap<u8, ModelPart>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uv_animation: BTreeMap<u8, AttachmentAnimation>,
    /// Additional source packages selected by the encounter for weapon slot zero.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weapon_variants: BTreeMap<u8, WeaponVariant>,
    pub trails: BTreeMap<u8, TrailMaterial>,
    pub body: Rig,
    /// Shared meshes, textures and sparse clips, loaded only for selected enemies.
    pub files: BTreeMap<String, crate::field_preload::File>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondaryBody {
    pub model_slot: u8,
    pub motion_offset: u16,
    pub model: ModelPart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeaponVariant {
    pub model: ModelPart,
    pub trail: TrailMaterial,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<AttachmentAnimation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rig {
    pub skeleton: Skeleton,
    /// KK slots point into this body's skeleton.
    pub attachments: BTreeMap<u8, u16>,
}
