//! A stored wind spell centered on a captured target position.
use super::{HitRule, StoredSpellPresentation};
use crate::battle::effects::{EffectBank, EffectId};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirThrustRecipe {
    pub lifetime: u16,
    pub projectile_tick: u16,
    pub rule: HitRule,
    pub projectile: EffectId,
    pub effect: EffectId,
    pub presentation: StoredSpellPresentation,
    /// Clamp the captured target center to this world height during initialization.
    pub target_min_height: f32,
    pub effect_scale: f32,
}

impl AirThrustRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.projectile_tick < self.lifetime
                && self.projectile.bank == EffectBank::Magic(9)
                && self.effect.bank == self.projectile.bank
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && self.target_min_height.is_finite()
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.,
            "invalid Air Thrust presentation or projectile schedule"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(9))?;
        Ok(())
    }
}
