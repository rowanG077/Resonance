//! Stored water spells retain either the arena origin or the caster's forward point.
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
pub enum WaterSpell {
    TidalWave = 202,
    AquaLaser = 203,
}

impl WaterSpell {
    pub fn effect(self, id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(self as u16 - 200),
            id,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaterOrigin {
    Battlefield { position: [f32; 3] },
    CasterAhead { distance: f32, height: f32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaterRecipe {
    pub kind: WaterSpell,
    pub lifetime: u16,
    pub projectile_tick: u16,
    pub rule: HitRule,
    pub origin: WaterOrigin,
    pub effect_scale: f32,
    pub presentation: StoredSpellPresentation,
}

impl WaterRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.projectile_tick < self.lifetime
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation)
                && matches!(self.rule.element, HitElement::Element(Element::Water)),
            "invalid stored water spell"
        );
        ensure!(
            match (self.kind, self.origin) {
                (WaterSpell::TidalWave, WaterOrigin::Battlefield { position }) =>
                    position.iter().all(|n| n.is_finite()),
                (WaterSpell::AquaLaser, WaterOrigin::CasterAhead { distance, height }) =>
                    distance.is_finite() && distance >= 0. && height.is_finite(),
                _ => false,
            },
            "water spell origin does not match its controller"
        );
        self.rule
            .impact_program_from(EffectBank::Techniques, Some(self.kind as u16 - 200))?;
        Ok(())
    }
}
