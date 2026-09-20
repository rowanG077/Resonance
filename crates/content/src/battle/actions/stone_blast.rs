//! Stone Blast keeps an ordinary ground anchor and three independent contact allocations.
use super::{HitElement, HitRule, VolleySchedule, lightning::GroundSpellOrigin};
use crate::{
    battle::effects::{EffectBank, EffectId, ProjectileRecipe},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoneBlastRecipe {
    pub lifetime: u16,
    pub origin: GroundSpellOrigin,
    pub effect: EffectId,
    pub effect_scale: f32,
    pub pulses: VolleySchedule,
    pub contact: ProjectileRecipe,
    pub rule: HitRule,
}

impl StoneBlastRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.effect
                == EffectId {
                    bank: EffectBank::Techniques,
                    id: 21
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
                && matches!(self.rule.element, HitElement::Element(Element::Earth)),
            "invalid Stone Blast source binding or origin"
        );
        self.pulses.validate(self.lifetime)?;
        self.contact.validate()?;
        self.rule
            .impact_program_from(EffectBank::Techniques, None)?;
        Ok(())
    }
}
