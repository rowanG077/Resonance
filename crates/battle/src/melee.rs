//! Actor hit streams and per-target admission.
use crate::{ActorId, HitRule};
use anyhow::{Result, ensure};
use std::sync::Arc;

/// An upright cylinder in actor-local units, facing along local +Z.
/// The action owns its dimensions; models and equipment poses never alter it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeleeVolume {
    pub offset: [f32; 3],
    pub radius: f32,
    pub half_height: f32,
}

#[derive(Debug, Clone)]
pub struct MeleeDefinition {
    pub hit: HitRule,
    pub volume: MeleeVolume,
    /// Optional weapon ribbon sampled while this contact window is active.
    pub trail: Option<u8>,
}

impl MeleeDefinition {
    pub(crate) fn validate(&self) -> Result<()> {
        self.hit.reaction.validate()?;
        ensure!(
            self.volume.offset.iter().all(|v| v.is_finite())
                && self.volume.radius.is_finite()
                && self.volume.radius >= 0.
                && self.volume.half_height.is_finite()
                && self.volume.half_height >= 0.,
            "invalid melee volume"
        );
        Ok(())
    }
}

pub(crate) struct Window {
    pub definition: Arc<MeleeDefinition>,
    pub remaining: u16,
    pub struck: Vec<ActorId>,
}
