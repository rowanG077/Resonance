//! A retained ground effect with two independently allocated contacts.
use super::{HitElement, HitRule, lightning::GroundSpellOrigin};
use crate::{
    battle::effects::{EffectBank, EffectId, ProjectileRecipe},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IcicleRecipe {
    pub lifetime: u16,
    pub origin: GroundSpellOrigin,
    pub effect: EffectId,
    pub effect_scale: f32,
    pub contact: ProjectileRecipe,
    pub pulses: [IciclePulse; 2],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct IciclePulse {
    pub tick: u16,
    pub reaction: u8,
    pub rule: HitRule,
}

impl IcicleRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.effect
                == EffectId {
                    bank: EffectBank::Techniques,
                    id: 32
                }
                && self.contact.id
                    == Some(EffectId {
                        bank: EffectBank::Techniques,
                        id: 1
                    })
                && self.origin.height.is_finite()
                && self.origin.nudge.is_finite()
                && self.origin.nudge >= 0.
                && self.origin.direction_threshold.is_finite()
                && self.origin.direction_threshold > 0.
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self.pulses[0].tick < self.pulses[1].tick
                && self.pulses[1].tick < self.lifetime,
            "invalid Icicle binding, origin or contact schedule"
        );
        self.contact.validate()?;
        for pulse in &self.pulses {
            ensure!(
                matches!(pulse.rule.element, HitElement::Element(Element::Ice)),
                "invalid Icicle element"
            );
            pulse
                .rule
                .impact_program_from(EffectBank::Techniques, None)?;
        }
        Ok(())
    }
}
