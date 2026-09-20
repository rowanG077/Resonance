//! A ground-anchored vortex and its independently timed persistent contact.
use super::{HitElement, HitRule, StoredSpellPresentation, lightning::GroundSpellOrigin};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceTornadoRecipe {
    pub lifetime: u16,
    pub origin: GroundSpellOrigin,
    pub presentation: StoredSpellPresentation,
    pub effect_scale: f32,
    pub projectile_tick: u16,
    pub rule: HitRule,
}

impl IceTornadoRecipe {
    pub const EFFECT: EffectId = EffectId {
        bank: EffectBank::Magic(21),
        id: 1,
    };

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.projectile_tick < self.lifetime
                && self.lifetime > 45
                && self.origin.height.is_finite()
                && self.origin.nudge.is_finite()
                && self.origin.nudge >= 0.
                && self.origin.direction_threshold.is_finite()
                && self.origin.direction_threshold > 0.
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && matches!(self.rule.element, HitElement::Element(Element::Ice)),
            "invalid Ice Tornado origin, presentation or contact schedule"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(21))?;
        Ok(())
    }
}
