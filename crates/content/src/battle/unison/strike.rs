//! Combined sword strikes keep their participant roles and native projectile callback.
use super::*;
use crate::battle::{
    actions::{HitRule, TechniquePhase},
    effects::{EffectBank, EffectId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strike {
    LightningTiger,
    FieryBeast,
    ThunderTiger,
}

impl Strike {
    pub const ALL: [Self; 3] = [Self::LightningTiger, Self::FieryBeast, Self::ThunderTiger];
    pub const fn native(self) -> u16 {
        match self {
            Self::LightningTiger => 313,
            Self::FieryBeast => 315,
            Self::ThunderTiger => 317,
        }
    }
    pub const fn combination(self) -> u8 {
        match self {
            Self::LightningTiger => 14,
            Self::FieryBeast => 16,
            Self::ThunderTiger => 18,
        }
    }
    pub const fn bank(self) -> EffectBank {
        EffectBank::Magic(self.native() - 200)
    }
    pub fn effect(self, id: u8) -> EffectId {
        EffectId {
            bank: self.bank(),
            id,
        }
    }
    pub fn programs(self) -> std::ops::Range<u8> {
        0..if self == Self::FieryBeast { 2 } else { 3 }
    }
    pub fn secondary(self, character: u8) -> bool {
        matches!(character, 6 | 9) || character == 3 && self != Self::ThunderTiger
    }
    pub fn selected(self, party: &[u8]) -> bool {
        party.contains(&1) && party.iter().any(|&c| self.secondary(c))
    }
    pub fn phase(self, role: usize, character: u8) -> Option<u8> {
        match role {
            0 if character == 1 => Some(0),
            1 if self.secondary(character) => Some(1),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrikeProgram {
    pub phases: [TechniquePhase; 2],
    pub direction: [f32; 3],
    pub distance: f32,
    pub color: [u8; 4],
    pub release_tick: u16,
    pub projectile_rule: HitRule,
    /// None keeps the primary actor's heading.
    pub projectile_heading: Option<f32>,
    /// None keeps the target's current height.
    pub projectile_height: Option<f32>,
    /// Lightning Tiger uses its actor's effect scale at release instead.
    pub initial_effect_scale: Option<f32>,
}

impl StrikeProgram {
    pub fn validate(&self, kind: Strike) -> Result<()> {
        ensure!(
            self.direction.iter().all(|v| v.is_finite())
                && self.direction[1] == 0.
                && self.distance.is_finite()
                && self.distance > 0.
                && self
                    .projectile_heading
                    .iter()
                    .chain(&self.projectile_height)
                    .all(|v| v.is_finite())
                && self
                    .initial_effect_scale
                    .is_none_or(|v| v.is_finite() && v > 0.),
            "invalid combined strike placement"
        );
        ensure!(
            self.release_tick
                == match kind {
                    Strike::LightningTiger => 35,
                    Strike::FieryBeast => 66,
                    Strike::ThunderTiger => 65,
                }
                && self.projectile_heading.is_some() == (kind != Strike::ThunderTiger)
                && self.projectile_height.is_some() == (kind == Strike::ThunderTiger)
                && self.initial_effect_scale.is_some() == (kind != Strike::LightningTiger),
            "invalid combined strike callback"
        );
        for (index, phase) in self.phases.iter().enumerate() {
            ensure!(
                usize::from(phase.variant) == index
                    && phase.action.duration > 0
                    && phase.action.tp == 0
                    && phase.buffer_until == 0
                    && phase.combo_at == 0
                    && phase.callback.is_none()
                    && phase.effect.is_none(),
                "invalid combined strike phase"
            );
            phase.action.validate(0)?;
        }
        ensure!(
            self.phases[1].action.hits.is_empty() && self.phases[1].action.commands.is_empty(),
            "combined strike secondary must have an empty native track"
        );
        ensure!(
            self.phases[0].action.duration > self.release_tick
                && self.phases[1].action.duration == 60,
            "invalid combined strike completion times"
        );
        self.projectile_rule
            .impact_program_from(EffectBank::Techniques, Some(kind.native() - 200))?;
        Ok(())
    }
}
