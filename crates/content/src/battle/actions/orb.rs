//! Two pulses sample a retained target's current effect anchor.
use super::{HitElement, HitRule, StoredSpellPresentation};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum OrbSpell {
    Photon = 251,
    DarkSphere = 278,
}

impl OrbSpell {
    pub const fn effect(self, id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(self as u16 - 200),
            id,
        }
    }
    pub const fn element(self) -> Element {
        match self {
            Self::Photon => Element::Light,
            Self::DarkSphere => Element::Darkness,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OrbPulse {
    pub tick: u16,
    pub rule: HitRule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrbRecipe {
    pub kind: OrbSpell,
    pub lifetime: u16,
    pub presentation: StoredSpellPresentation,
    pub effect_scale: f32,
    pub pulses: [OrbPulse; 2],
}

impl OrbRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.pulses[0].tick < self.pulses[1].tick
                && self.pulses[1].tick < self.lifetime
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation),
            "invalid orb spell presentation or pulse schedule"
        );
        for pulse in self.pulses {
            ensure!(
                matches!(pulse.rule.element, HitElement::Element(element) if element == self.kind.element()),
                "orb pulse element differs from its spell"
            );
            pulse
                .rule
                .impact_program_from(EffectBank::Techniques, Some(self.kind as u16 - 200))?;
        }
        Ok(())
    }
}
