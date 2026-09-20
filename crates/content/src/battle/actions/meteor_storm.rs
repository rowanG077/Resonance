//! Fourteen neutral meteors use world-relative positions and one retained heading.
use super::{HitElement, HitRule, StoredSpellPresentation};
use crate::battle::effects::{EffectBank, EffectId};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use super::ray::RayBurst;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeteorStormRecipe {
    pub lifetime: u16,
    pub presentation: StoredSpellPresentation,
    pub effect_scale: f32,
    /// Radians added once to the captured caster heading.
    pub heading_offset: f32,
    pub bursts: [RayBurst; 14],
    pub rule: HitRule,
}
impl MeteorStormRecipe {
    pub const fn effect(id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(33),
            id,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && self.heading_offset.is_finite()
                && self.bursts.windows(2).all(|p| p[0].tick < p[1].tick)
                && self
                    .bursts
                    .iter()
                    .all(|b| b.tick < self.lifetime && b.offset.into_iter().all(f32::is_finite))
                && self.rule.element == HitElement::Neutral,
            "invalid Meteor Storm presentation or bursts"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(33))?;
        Ok(())
    }
}
