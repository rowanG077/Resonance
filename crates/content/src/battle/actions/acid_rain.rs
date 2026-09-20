//! A global rain scene applies a retained defense effect to the opposing roster.
use super::StoredSpellPresentation;
use crate::battle::{
    conditions::StatDebuff,
    effects::{EffectBank, EffectId},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AcidRainRecipe {
    pub lifetime: u16,
    pub application_tick: u16,
    pub effect: EffectId,
    pub origin: [f32; 3],
    pub effect_scale: f32,
    pub stat: StatDebuff,
    /// Preserve the signed authored amount; the name does not determine its sign.
    pub amount: i16,
    /// Zero selects the shared status duration.
    pub duration: i16,
    pub retained: bool,
    pub tint: [u8; 4],
    pub presentation: StoredSpellPresentation,
}

impl AcidRainRecipe {
    pub const fn effect(id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(65),
            id,
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.application_tick < self.lifetime
                && self.effect == Self::effect(1)
                && self.origin.iter().all(|value| value.is_finite())
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self.stat == StatDebuff::DefenseDown
                && self.duration >= 0
                && self.tint[3] == 255
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation),
            "invalid Acid Rain scene or status application"
        );
        Ok(())
    }
}
