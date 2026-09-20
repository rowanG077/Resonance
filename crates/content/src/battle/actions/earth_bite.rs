//! A raised lightning contact precedes two ground-level earth contacts.
use super::{HitElement, StoredSpellPresentation, lightning::GroundSpellOrigin};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use super::earth_field::EarthFieldPulse;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarthBiteRecipe {
    pub lifetime: u16,
    pub origin: GroundSpellOrigin,
    pub presentation: StoredSpellPresentation,
    pub effect_scale: f32,
    pub pulses: [EarthFieldPulse; 3],
    /// Absolute world height for the later two contacts; the retained origin stays unchanged.
    pub second_height: f32,
}
impl EarthBiteRecipe {
    pub const fn effect(id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(31),
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
                && self.second_height.is_finite()
                && self.pulses.windows(2).all(|p| p[0].tick < p[1].tick),
            "invalid Earth Bite origin, presentation or schedule"
        );
        for (index, pulse) in self.pulses.iter().enumerate() {
            ensure!(
                pulse.tick < self.lifetime
                    && pulse.projectile == Self::effect(if index == 0 { 1 } else { 2 })
                    && pulse.rule.element
                        == HitElement::Element(if index == 0 {
                            Element::Lightning
                        } else {
                            Element::Earth
                        }),
                "invalid Earth Bite contact"
            );
            pulse
                .rule
                .impact_program_from(EffectBank::Techniques, Some(31))?;
        }
        Ok(())
    }
}
