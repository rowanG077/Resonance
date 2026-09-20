//! A retained casting ring launches six separately aimed ice projectiles.
use super::{HitElement, HitRule, StoredSpellPresentation};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeLancerRecipe {
    pub lifetime: u16,
    pub presentation: StoredSpellPresentation,
    pub effect_scale: f32,
    pub forward_distance: f32,
    pub minimum_height: f32,
    pub radius: f32,
    /// Original radians-per-sector, retained independently of the yaw conversion.
    pub sector_angle: f32,
    pub order: [u8; 6],
    pub first_tick: u16,
    pub interval: u16,
    pub forward_threshold: f32,
    pub direction_threshold: f32,
    pub vertical_limit: f32,
    pub sound: u16,
    pub rule: HitRule,
}

impl FreezeLancerRecipe {
    pub const fn effect(id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(22),
            id,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let mut order = self.order;
        order.sort_unstable();
        ensure!(
            order == [0, 1, 2, 3, 4, 5]
                && self.interval > 0
                && u32::from(self.first_tick) + 5 * u32::from(self.interval)
                    < u32::from(self.lifetime)
                && self.lifetime > 45
                && [
                    self.effect_scale,
                    self.radius,
                    self.sector_angle,
                    self.direction_threshold
                ]
                .iter()
                .all(|v| v.is_finite() && *v > 0.)
                && self.forward_distance.is_finite()
                && self.forward_distance >= 0.
                && self.minimum_height.is_finite()
                && (0. ..1.).contains(&self.forward_threshold)
                && (0. ..1.).contains(&self.vertical_limit)
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && self.sound != 0
                && matches!(self.rule.element, HitElement::Element(Element::Ice)),
            "invalid Freeze Lancer ring, aiming or callback schedule"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(22))?;
        Ok(())
    }
}
