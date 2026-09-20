//! One ordered radius search selects the recipient of two independent ice contacts.
use super::{HitElement, HitRule, StoredSpellPresentation, lightning::GroundSpellOrigin};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbsoluteRecipe {
    pub lifetime: u16,
    pub origin: GroundSpellOrigin,
    pub presentation: StoredSpellPresentation,
    pub effect_scale: f32,
    /// Strict horizontal distance from the captured ground point.
    pub radius: f32,
    pub select_tick: u16,
    pub second_tick: u16,
    pub rules: [HitRule; 2],
}
impl AbsoluteRecipe {
    pub const fn effect(id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(30),
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
                && self.origin.height.is_finite()
                && self.origin.nudge.is_finite()
                && self.origin.nudge >= 0.
                && self.origin.direction_threshold.is_finite()
                && self.origin.direction_threshold > 0.
                && self.radius.is_finite()
                && self.radius > 0.
                && self.select_tick < self.second_tick
                && self.second_tick < self.lifetime,
            "invalid Absolute origin, presentation or schedule"
        );
        for rule in self.rules {
            ensure!(
                rule.element == HitElement::Element(Element::Ice),
                "invalid Absolute element"
            );
            rule.impact_program_from(EffectBank::Techniques, Some(30))?;
        }
        Ok(())
    }
}
