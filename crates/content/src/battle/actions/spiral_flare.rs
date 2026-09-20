//! A travelling fire column is launched from a captured point ahead of its caster.
use super::{HitElement, HitRule, StoredSpellPresentation};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpiralFlareRecipe {
    pub lifetime: u16,
    pub pulse_tick: u16,
    pub effect_scale: f32,
    pub presentation: StoredSpellPresentation,
    /// Offset along the captured caster attack direction, from its attack origin.
    pub forward_distance: f32,
    /// Absolute world height of the captured launch point.
    pub height: f32,
    pub rule: HitRule,
}

impl SpiralFlareRecipe {
    pub const fn effect(id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(26),
            id,
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.pulse_tick < self.lifetime
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self.forward_distance.is_finite()
                && self.forward_distance >= 0.
                && self.height.is_finite()
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && self.rule.element == HitElement::Element(Element::Fire),
            "invalid Spiral Flare origin, presentation or schedule"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(26))?;
        Ok(())
    }
}
