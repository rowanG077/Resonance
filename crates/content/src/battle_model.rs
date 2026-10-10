//! Source rigs shared by combat preparation and the existing model publications.
use crate::animation::Skeleton;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    pub files: BTreeMap<String, crate::field_preload::File>,
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

/// Optional body and carried artwork. Enemy gameplay has its own definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Enemy {
    pub source_sha256: String,
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
    /// KK slots point into this body's skeleton.
    pub attachments: BTreeMap<u8, u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_json_roundtrip_preserves_numeric_part_and_rig_keys() -> anyhow::Result<()> {
        let attachment = Attachment::Weapon(Weapon {
            source_sha256: "source".into(),
            parts: BTreeMap::from([(
                0,
                ModelPart {
                    rig: Rig {
                        skeleton: Skeleton { bones: vec![] },
                        attachments: BTreeMap::from([(1, 3)]),
                    },
                    layer_skeletons: BTreeMap::new(),
                    layers: vec![],
                },
            )]),
            trails: BTreeMap::new(),
            files: BTreeMap::new(),
        });
        let restored: Attachment = serde_json::from_slice(&serde_json::to_vec(&attachment)?)?;
        let Attachment::Weapon(model) = restored else {
            panic!("weapon variant changed")
        };
        assert_eq!(model.parts[&0].rig.attachments[&1], 3);
        Ok(())
    }
}
