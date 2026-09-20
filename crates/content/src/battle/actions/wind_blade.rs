//! Three stationary wind pulses share one retained target position.
use super::{HitRule, VolleySchedule};
use crate::battle::effects::{EffectBank, EffectId, ProjectileRecipe};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindBladeRecipe {
    pub lifetime: u16,
    pub effect_tick: u16,
    pub effect: EffectId,
    pub pulses: VolleySchedule,
    pub contact: ProjectileRecipe,
    pub rule: HitRule,
}

impl WindBladeRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.effect_tick < self.lifetime
                && self.effect.bank == EffectBank::Techniques
                && self.contact.id
                    == Some(EffectId {
                        bank: EffectBank::Techniques,
                        id: 1
                    }),
            "invalid Wind Blade effect or shared contact binding"
        );
        self.pulses.validate(self.lifetime)?;
        self.contact.validate()?;
        self.rule
            .impact_program_from(EffectBank::Techniques, None)?;
        Ok(())
    }
}
