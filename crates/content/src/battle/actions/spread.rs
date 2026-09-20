//! A stored water spell with a fixed target origin and one delayed contact volume.
use super::{HitRule, StoredSpellPresentation};
use crate::battle::effects::{EffectBank, EffectId};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadRecipe {
    pub lifetime: u16,
    pub projectile_tick: u16,
    pub rule: HitRule,
    pub projectile: EffectId,
    pub effect: EffectId,
    pub presentation: StoredSpellPresentation,
    pub target_height: f32,
    /// Offset toward the caster, applied once when the stored spell initializes.
    pub target_nudge_distance: f32,
    pub target_nudge_threshold: f32,
    pub effect_scale: f32,
}

impl SpreadRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.projectile_tick < self.lifetime
                && self.projectile.bank == EffectBank::Magic(1)
                && self.effect.bank == self.projectile.bank
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && self.target_height.is_finite()
                && self.target_nudge_distance.is_finite()
                && self.target_nudge_distance >= 0.
                && self.target_nudge_threshold.is_finite()
                && self.target_nudge_threshold > 0.
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.,
            "invalid Spread presentation or projectile schedule"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(1))?;
        Ok(())
    }
}
