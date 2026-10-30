//! Source rigs shared by combat preparation and the existing model publications.
use crate::animation::Skeleton;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub mod document;

/// Cooked attachment descriptors, keyed by equipment item or carried-prop identity.
pub type Weapons = BTreeMap<u16, Attachment>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Attachment {
    Weapon(Weapon),
    Shield(Weapon),
    Nonvisual,
}

#[derive(Debug, Clone, Serialize)]
pub struct Weapon {
    pub parts: BTreeMap<u8, ModelPart>,
    pub trails: BTreeMap<u8, Trail>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uv_animation: BTreeMap<u8, AttachmentAnimation>,
}

mod controls;
pub use controls::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPart {
    pub rig: Rig,
    /// Independent visual skeletons keyed by scene resource. Other layers share the contact rig.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layer_skeletons: BTreeMap<u16, Skeleton>,
    pub layers: Vec<crate::model_preview::PreviewPart>,
}

/// A costume's body and independent battle motion bank. Party
/// statistics and actor templates have their existing session/table owners.
#[derive(Debug, Clone, Serialize)]
pub struct Party {
    /// Explicit empty entries in the authored body animation table.
    #[serde(default)]
    pub null_motions: BTreeSet<u16>,
    pub body: Rig,
    pub parts: Vec<crate::ScenePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wings: Option<crate::ScenePart>,
}

/// Optional body and carried artwork. Enemy gameplay has its own definition.
#[derive(Debug, Clone, Serialize)]
pub struct Enemy {
    /// A separately animated body component, sharing the actor's action clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_body: Option<SecondaryBody>,
    /// Formation-selected vertical atlas rows on the primary body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_variant: Option<crate::AtlasChannel>,
    /// Persistent models declared independently of carried weapons and effect inventory.
    pub attached_models: Vec<AttachedModel>,
    /// Native carried model slots, independently visible and sampled.
    pub attachments: BTreeMap<u8, ModelPart>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uv_animation: BTreeMap<u8, AttachmentAnimation>,
    /// Additional source packages selected by the encounter for weapon slot zero.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weapon_variants: BTreeMap<u8, WeaponVariant>,
    pub trails: BTreeMap<u8, Trail>,
    pub body: Rig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachedModel {
    pub model_slot: u8,
    pub motion_slot: Option<u8>,
    pub placement: AttachmentPlacement,
    pub visible: bool,
    pub additive: bool,
    pub inherit_tint: bool,
    pub wing_layers: bool,
    pub transform: crate::animation::Transform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentPlacement {
    Bone(u8),
    Independent,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecondaryBody {
    pub model_slot: u8,
    pub motion_offset: u16,
    pub model: ModelPart,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeaponVariant {
    pub model: ModelPart,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trail: Option<Trail>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<AttachmentAnimation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rig {
    pub skeleton: Skeleton,
    /// KK slots point into this body's skeleton.
    pub attachments: BTreeMap<u8, u16>,
}

impl Rig {
    /// Resolve editable attachment targets against the current model nodes.
    pub fn from_glb(bytes: &[u8], targets: &BTreeMap<u8, String>) -> anyhow::Result<Self> {
        use anyhow::{Context, ensure};
        let skeleton = Skeleton::from_glb(bytes)?;
        let mut attachments = BTreeMap::new();
        for (&slot, name) in targets {
            let mut matching = skeleton
                .bones
                .iter()
                .enumerate()
                .filter(|(_, bone)| &bone.name == name);
            let (index, _) = matching
                .next()
                .with_context(|| format!("attachment {slot} refers to absent joint {name}"))?;
            ensure!(
                matching.next().is_none(),
                "attachment {slot} refers to ambiguous joint {name}"
            );
            attachments.insert(slot, u16::try_from(index)?);
        }
        Ok(Self {
            skeleton,
            attachments,
        })
    }

    /// Publication keeps only gameplay slots and their readable joint targets.
    pub fn attachment_names(&self) -> anyhow::Result<BTreeMap<u8, String>> {
        use anyhow::Context;
        self.attachments
            .iter()
            .map(|(&slot, &bone)| {
                Ok((
                    slot,
                    self.skeleton
                        .bones
                        .get(usize::from(bone))
                        .context("attachment bone is absent")?
                        .name
                        .clone(),
                ))
            })
            .collect()
    }
}
