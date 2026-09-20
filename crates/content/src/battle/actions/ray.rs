//! Nine light columns share a captured ground point and heading.
use super::{HitElement, HitRule, StoredSpellPresentation, lightning::GroundSpellOrigin};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RayBurst {
    pub tick: u16,
    /// Local XYZ offset, rotated by the retained heading before adding the ground point.
    pub offset: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RayRecipe {
    pub lifetime: u16,
    pub origin: GroundSpellOrigin,
    pub presentation: StoredSpellPresentation,
    /// Radians added once to the captured caster heading, not an elevation offset.
    pub heading_offset: f32,
    pub effect_scale: f32,
    pub bursts: [RayBurst; 9],
    pub rule: HitRule,
}

impl RayRecipe {
    pub const fn effect(id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(52),
            id,
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.origin.height.is_finite()
                && self.origin.nudge.is_finite()
                && self.origin.nudge >= 0.
                && self.origin.direction_threshold.is_finite()
                && self.origin.direction_threshold > 0.
                && self.heading_offset.is_finite()
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self
                    .bursts
                    .windows(2)
                    .all(|pair| pair[0].tick < pair[1].tick)
                && self.bursts.iter().all(|burst| burst.tick < self.lifetime
                    && burst.offset.into_iter().all(f32::is_finite))
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && matches!(self.rule.element, HitElement::Element(Element::Light)),
            "invalid Ray origin, presentation or bursts"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(52))?;
        Ok(())
    }
}
