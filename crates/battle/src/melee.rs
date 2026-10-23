//! Actor hit streams and per-target admission.
use crate::{ActorId, HitRule};
use anyhow::{Result, ensure};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeleeAnchor {
    pub resource: crate::ResourceId,
    pub bone: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MeleeVolume {
    Fixed {
        offset: [f32; 3],
        radius: f32,
        half_height: f32,
    },
    BodyCenter {
        shape: crate::HitShape,
        radius: f32,
        half_height: f32,
    },
    Attached {
        anchors: Vec<MeleeAnchor>,
        shape: crate::HitShape,
        radius: f32,
        half_height: f32,
    },
}

#[derive(Debug, Clone)]
pub struct MeleeDefinition {
    pub hit: HitRule,
    pub volume: MeleeVolume,
    /// Optional weapon ribbon sampled while this contact window is active.
    pub trail: Option<crate::WeaponSlot>,
}

impl MeleeDefinition {
    pub(crate) fn validate(&self) -> Result<()> {
        self.hit.reaction.validate()?;
        ensure!(
            !self.hit.reaction.capture || matches!(&self.volume, MeleeVolume::Attached { .. }),
            "capture requires a melee attachment"
        );
        let (radius, half_height) = match &self.volume {
            MeleeVolume::Fixed {
                offset,
                radius,
                half_height,
            } => {
                ensure!(offset.iter().all(|v| v.is_finite()), "invalid melee offset");
                (*radius, *half_height)
            }
            MeleeVolume::Attached {
                shape,
                radius,
                half_height,
                ..
            }
            | MeleeVolume::BodyCenter {
                shape,
                radius,
                half_height,
                ..
            } => {
                if let MeleeVolume::Attached { anchors, .. } = &self.volume {
                    ensure!(!anchors.is_empty(), "empty melee anchors");
                }
                if let crate::HitShape::Ring { width } = shape {
                    ensure!(width.is_finite(), "invalid melee ring");
                }
                (*radius, *half_height)
            }
        };
        ensure!(
            radius.is_finite() && radius >= 0. && half_height.is_finite() && half_height >= 0.,
            "invalid melee volume"
        );
        Ok(())
    }
}

pub(crate) struct Window {
    pub definition: Arc<MeleeDefinition>,
    pub remaining: u16,
    pub positions: Vec<[f32; 3]>,
    /// Each victim can be contacted once until the attack opens a new window.
    pub struck: Vec<ActorId>,
}
