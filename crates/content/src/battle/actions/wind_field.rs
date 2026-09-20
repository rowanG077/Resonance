//! Cyclone and Air Blade share a stored lifecycle and keep distinct original anchors.
use super::{HitElement, HitRule, StoredSpellPresentation};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u16)]
pub enum WindField {
    Cyclone = 210,
    AirBlade = 211,
}
impl WindField {
    pub fn effect(self, id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(self as u16 - 200),
            id,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WindFieldOrigin {
    TargetGround {
        height: f32,
        nudge: f32,
        threshold: f32,
    },
    CasterAhead {
        distance: f32,
        height: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindFieldRecipe {
    pub kind: WindField,
    pub lifetime: u16,
    pub projectile_tick: u16,
    pub rule: HitRule,
    pub origin: WindFieldOrigin,
    pub effect_scale: f32,
    pub presentation: StoredSpellPresentation,
}
impl WindFieldRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.projectile_tick < self.lifetime
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && matches!(self.rule.element, HitElement::Element(Element::Wind)),
            "invalid stored Wind field"
        );
        ensure!(
            match (self.kind, self.origin) {
                (
                    WindField::Cyclone,
                    WindFieldOrigin::TargetGround {
                        height,
                        nudge,
                        threshold,
                    },
                ) =>
                    height.is_finite()
                        && nudge.is_finite()
                        && nudge >= 0.
                        && threshold.is_finite()
                        && threshold > 0.,
                (WindField::AirBlade, WindFieldOrigin::CasterAhead { distance, height }) =>
                    distance.is_finite() && distance >= 0. && height.is_finite(),
                _ => false,
            },
            "stored Wind origin differs from its controller"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(self.kind as u16 - 200))?;
        Ok(())
    }
}
