//! Stored Earth spells retain one ground point for independently authored contacts.
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
pub enum EarthField {
    Stalagmite = 213,
    GroundDasher = 214,
    Grave = 215,
}

impl EarthField {
    pub fn effect(self, id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(self as u16 - 200),
            id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarthFieldRecipe {
    pub kind: EarthField,
    pub lifetime: u16,
    pub effect_scale: f32,
    pub presentation: StoredSpellPresentation,
    pub target_height: f32,
    pub target_nudge_distance: f32,
    pub target_nudge_threshold: f32,
    pub pulses: Vec<EarthFieldPulse>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EarthFieldPulse {
    pub tick: u16,
    pub projectile: EffectId,
    pub rule: HitRule,
}

impl EarthFieldRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && !self.pulses.is_empty()
                && self.pulses.windows(2).all(|p| p[0].tick < p[1].tick)
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && self.target_height.is_finite()
                && self.target_nudge_distance.is_finite()
                && self.target_nudge_distance >= 0.
                && self.target_nudge_threshold.is_finite()
                && self.target_nudge_threshold > 0.
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.,
            "invalid stored Earth presentation or schedule"
        );
        for pulse in &self.pulses {
            // Projectile row0 is an authored contact in Stalagmite and Grave.
            ensure!(
                pulse.tick < self.lifetime
                    && pulse.projectile.bank == self.kind.effect(0).bank
                    && matches!(pulse.rule.element, HitElement::Element(Element::Earth)),
                "invalid stored Earth contact"
            );
            pulse
                .rule
                .impact_program_from(EffectBank::Techniques, Some(self.kind as u16 - 200))?;
        }
        Ok(())
    }
}
