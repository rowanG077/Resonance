//! A resident healing scene has one model per roster slot and one group recovery pulse.
use super::StoredSpellPresentation;
use crate::battle::effects::{EffectBank, EffectId};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NurseRecipe {
    pub lifetime: u16,
    pub recovery_tick: u16,
    pub percent: u16,
    pub first_heading: f32,
    pub heading_step: f32,
    pub presentation: StoredSpellPresentation,
}

impl NurseRecipe {
    pub const MODELS: usize = 4;

    pub const fn effect(id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(37),
            id,
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.recovery_tick < self.lifetime
                && (1..=100).contains(&self.percent)
                && self.first_heading.is_finite()
                && self.heading_step.is_finite()
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation),
            "invalid Nurse recovery scene"
        );
        Ok(())
    }
}
