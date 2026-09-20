//! A four-point ring launches converging lances, followed by a central finisher.
use super::{HitElement, HitRule, StoredSpellPresentation, lightning::GroundSpellOrigin};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum LanceSpell {
    HolyLance = 253,
    BloodyLance = 283,
}
impl LanceSpell {
    pub const fn effect(self, id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(self as u16 - 200),
            id,
        }
    }
    pub const fn element(self) -> Element {
        match self {
            Self::HolyLance => Element::Light,
            Self::BloodyLance => Element::Darkness,
        }
    }
    pub const fn menu(self) -> u16 {
        match self {
            Self::HolyLance => 115,
            Self::BloodyLance => 250,
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LanceRing {
    /// Radians around the captured casting anchor, before its heading rotation.
    pub angle: f32,
    pub marker_tick: u16,
    pub projectile_tick: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanceRecipe {
    pub kind: LanceSpell,
    pub lifetime: u16,
    pub presentation: StoredSpellPresentation,
    pub effect_scale: f32,
    pub origin: GroundSpellOrigin,
    pub ring: [LanceRing; 4],
    pub ring_radius: f32,
    /// Y offset added to the captured anchor for the four elevated projectiles.
    pub ring_projectile_height: f32,
    pub final_tick: u16,
    /// Absolute world Y for the finisher's visual and projectile respectively.
    pub final_effect_height: f32,
    pub final_projectile_height: f32,
    /// Absolute world Y of the retained projectile aim point at the anchor's X/Z.
    pub target_height: f32,
    pub afterglow_tick: u16,
    /// Y offset added to the captured anchor for the last visual.
    pub afterglow_height: f32,
    pub rules: [HitRule; 2],
}
impl LanceRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.origin.height.is_finite()
                && self.origin.nudge.is_finite()
                && self.origin.nudge >= 0.
                && self.origin.direction_threshold.is_finite()
                && self.origin.direction_threshold > 0.
                && self.final_tick < self.afterglow_tick
                && self.afterglow_tick < self.lifetime
                && self
                    .ring
                    .windows(2)
                    .all(|r| r[0].marker_tick < r[1].marker_tick
                        && r[0].projectile_tick < r[1].projectile_tick)
                && self.ring.iter().all(|r| r.angle.is_finite()
                    && r.marker_tick < r.projectile_tick
                    && r.projectile_tick < self.final_tick)
                && [self.effect_scale, self.ring_radius]
                    .into_iter()
                    .all(|v| v.is_finite() && v > 0.)
                && [
                    self.ring_projectile_height,
                    self.final_effect_height,
                    self.final_projectile_height,
                    self.target_height,
                    self.afterglow_height
                ]
                .into_iter()
                .all(|v| v.is_finite() && v >= 0.)
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation),
            "invalid lance presentation or emission schedule"
        );
        for rule in self.rules {
            ensure!(
                matches!(rule.element,HitElement::Element(element) if element==self.kind.element()),
                "lance rule element differs from its spell"
            );
            rule.impact_program_from(EffectBank::Techniques, Some(self.kind as u16 - 200))?;
        }
        Ok(())
    }
}
