//! Stored fire spells bind one ground point and emit independently authored hit rules.
use super::{HitElement, HitRule, StoredSpellPresentation};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FireFieldRecipe {
    pub native_id: u16,
    pub effect: EffectId,
    pub lifetime: u16,
    pub effect_scale: f32,
    pub presentation: StoredSpellPresentation,
    pub target_height: f32,
    pub target_nudge_distance: f32,
    pub target_nudge_threshold: f32,
    pub pulses: Vec<FireFieldPulse>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FireFieldPulse {
    pub tick: u16,
    pub projectile: EffectId,
    pub rule: HitRule,
}

impl FireFieldRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (205..=207).contains(&self.native_id),
            "unsupported fire field native"
        );
        let bank = EffectBank::Magic(self.native_id - 200);
        ensure!(
            self.effect == EffectId { bank, id: 1 }
                && self.lifetime > 45
                && !self.pulses.is_empty()
                && self
                    .pulses
                    .windows(2)
                    .all(|pair| pair[0].tick < pair[1].tick)
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
            "invalid fire field presentation or schedule"
        );
        for pulse in &self.pulses {
            ensure!(
                pulse.tick < self.lifetime
                    && pulse.projectile.bank == bank
                    && pulse.projectile.id != 0
                    && matches!(pulse.rule.element, HitElement::Element(Element::Fire)),
                "invalid fire field contact"
            );
            pulse
                .rule
                .impact_program_from(EffectBank::Techniques, Some(self.native_id - 200))?;
        }
        Ok(())
    }
}
