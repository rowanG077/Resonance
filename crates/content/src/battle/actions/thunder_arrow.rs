//! Thunder Arrow keeps one contact anchor and three separate surrounding visual programs.
use super::{HitElement, HitRule, StoredSpellPresentation, lightning::GroundSpellOrigin};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThunderArrowRecipe {
    pub lifetime: u16,
    pub origin: GroundSpellOrigin,
    pub presentation: StoredSpellPresentation,
    /// Radians added once to the caster's heading during initialization.
    pub heading_offset: f32,
    pub effect_scale: f32,
    pub satellite_tick: u16,
    pub satellite_count: u8,
    pub satellite_step: f32,
    pub satellite_radius: f32,
    pub satellite_heading: f32,
    pub projectile_tick: u16,
    pub rule: HitRule,
}

impl ThunderArrowRecipe {
    pub const fn effect(id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(27),
            id,
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.satellite_tick < self.projectile_tick
                && self.projectile_tick < self.lifetime
                && self.lifetime > 45
                && self.satellite_count > 0
                && self.origin.height.is_finite()
                && self.origin.nudge.is_finite()
                && self.origin.nudge >= 0.
                && self.origin.direction_threshold.is_finite()
                && self.origin.direction_threshold > 0.
                && self.heading_offset.is_finite()
                && self.satellite_heading.is_finite()
                && self.satellite_step.is_finite()
                && self.satellite_step > 0.
                && self.satellite_radius.is_finite()
                && self.satellite_radius > 0.
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && matches!(self.rule.element, HitElement::Element(Element::Lightning)),
            "invalid Thunder Arrow origin, presentation or callbacks"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(27))?;
        Ok(())
    }
}
