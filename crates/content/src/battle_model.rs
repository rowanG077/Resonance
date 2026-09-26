//! Source rigs shared by combat preparation and the existing model publications.
use crate::{animation::Skeleton, battle_profile::Profile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WEAPONS_PATH: &str = "battle/weapons.json";

/// Original resource slots, including holes. Item selection is resolved at load time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weapons {
    pub source_sha256: String,
    pub table_sha256: String,
    pub owner_motion_sha256: String,
    pub records: Vec<Option<Weapon>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weapon {
    pub source_sha256: String,
    pub parts: BTreeMap<u8, ModelPart>,
    pub trails: BTreeMap<u8, TrailMaterial>,
    pub files: BTreeMap<String, crate::field_preload::File>,
}

/// Original member-zero blade ribbon material; timer/history stay in combat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrailMaterial {
    pub texture: i8,
    pub palette: u8,
    pub flags: u8,
    pub color: [u8; 3],
    pub uv: [i16; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPart {
    pub rig: Rig,
    pub layers: Vec<crate::model_preview::PreviewPart>,
}

pub fn enemy_path(id: u8) -> String {
    format!("battle/enemies/{id:03}.json")
}

pub fn enemy_projectiles_path(id: u8) -> String {
    format!("battle/enemies/{id:03}/projectiles.json")
}

pub fn enemy_effects_path(id: u8) -> String {
    format!("battle/enemies/{id:03}/effects.json")
}

pub fn party_path(character: u8) -> String {
    format!("battle/party/{character:02}.json")
}

/// The ordinary costume's body and independent battle motion bank. Party
/// statistics and actor templates have their existing session/table owners.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Party {
    pub body_sha256: String,
    pub motion_sha256: String,
    pub body: Rig,
    pub parts: Vec<crate::ScenePart>,
    pub files: BTreeMap<String, crate::field_preload::File>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Enemy {
    pub source_sha256: String,
    /// Battle resource's statistics-header name; independent of Monster Book text.
    pub name: String,
    /// Original encoded name byte length divided by two (44E1C).
    pub hidden_name_units: u16,
    pub profile: Profile,
    pub actions: crate::battle_action::EnemyActions,
    /// Enemy statistics byte 0, used by 1AB2C/36A94.
    pub target_strategy: u8,
    /// Enemy statistics byte 2; zero uses the actor-kind default in 1A9AC.
    pub guard_preference: u8,
    /// Native carried model slots, independently visible and sampled.
    pub attachments: BTreeMap<u8, ModelPart>,
    pub trails: BTreeMap<u8, TrailMaterial>,
    pub body: Rig,
    /// Shared meshes, textures and sparse clips, loaded only for selected enemies.
    pub files: BTreeMap<String, crate::field_preload::File>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rig {
    pub skeleton: Skeleton,
    /// Original transform selectors, retained until the model loader admits them.
    pub transform_kinds: Vec<u8>,
    pub volumes: Vec<Volume>,
    /// Bones admitted by the original 1BD18 / 1B1DC target-bounds classifier.
    pub target_bones: Vec<u16>,
    /// Original actor AT groups; empty groups remain explicit.
    pub attack_groups: BTreeMap<u8, Vec<u16>>,
    /// KK slots point into this body's skeleton.
    pub attachments: BTreeMap<u8, u16>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Volume {
    pub bone: u16,
    pub radius: f32,
    pub hurt: bool,
    pub body: bool,
}
