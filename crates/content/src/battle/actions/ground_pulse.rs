//! One persistent contact follows the visual at a captured ground point.
use super::{HitElement, HitRule, StoredSpellPresentation, lightning::GroundSpellOrigin};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u16)]
pub enum GroundPulseSpell {
    RagingMist = 223,
    DreadedWave = 224,
    GravityWell = 228,
    Atlas = 229,
}

impl GroundPulseSpell {
    pub const fn effect(self, id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(self as u16 - 200),
            id,
        }
    }

    pub const fn element(self) -> Option<Element> {
        match self {
            Self::RagingMist => Some(Element::Fire),
            Self::DreadedWave => Some(Element::Earth),
            Self::GravityWell => None,
            Self::Atlas => Some(Element::Wind),
        }
    }

    pub const fn menu(self) -> u16 {
        match self {
            Self::RagingMist => 85,
            Self::DreadedWave => 86,
            Self::GravityWell => 90,
            Self::Atlas => 91,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundPulseRecipe {
    pub kind: GroundPulseSpell,
    pub lifetime: u16,
    pub pulse_tick: u16,
    pub effect_scale: f32,
    pub presentation: StoredSpellPresentation,
    pub origin: GroundSpellOrigin,
    pub rule: HitRule,
}

impl GroundPulseRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 45
                && self.pulse_tick < self.lifetime
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
                && (0. ..90.).contains(&self.presentation.camera_elevation),
            "invalid ground pulse origin, presentation or schedule"
        );
        ensure!(
            self.rule.element
                == self
                    .kind
                    .element()
                    .map_or(HitElement::Neutral, HitElement::Element),
            "invalid ground pulse element"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(self.kind as u16 - 200))?;
        Ok(())
    }
}
