//! Parameters for the native Defense System coordination controller.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PATH: &str = "battle/defense-system.json";

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition<Enemy = u8> {
    pub core: FixedMember<Enemy>,
    pub repair: FixedMember<Enemy>,
    pub orbit: Enemy,
    pub orbit_count: u8,
    pub arm: Enemy,
    /// Exact GLB joint names in formation arm order.
    pub arm_joints: [String; 4],
    pub deployment: Deployment,
    pub repair_cycle: RepairCycle,
    pub repair_sound: u16,
    pub repair_sound_priority: u8,
    pub messages: Messages,
    /// Maintained camera restriction in degrees; not an extracted original table.
    pub camera_yaw_limits: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedMember<Enemy = u8> {
    pub enemy: Enemy,
    pub position: [f32; 3],
    pub heading: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    #[serde(with = "crate::battle_motion")]
    pub clip: u16,
    pub ticks: u16,
    pub hp_percent: u8,
    pub rate: f32,
    pub blend: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairCycle {
    pub self_ticks: u16,
    pub repeat_ticks: u16,
    pub orbit_ticks: u16,
    pub self_percent: u8,
    pub orbit_percent: u8,
    /// Divide the core's initial defense before multiplying while repair is unavailable.
    pub defense_divisor: u8,
    pub defense_multiplier: u8,
}

impl RepairCycle {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.self_ticks > 0
                && self.orbit_ticks > 0
                && (1..=100).contains(&self.self_percent)
                && (1..=100).contains(&self.orbit_percent)
                && self.defense_divisor > 0
                && self.defense_multiplier <= self.defense_divisor,
            "invalid Defense System repair cycle"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Messages {
    pub automatic: String,
    pub orbit: String,
    pub deployment: String,
    pub duration: u16,
}

impl Messages {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.duration > 0
                && [&self.automatic, &self.orbit, &self.deployment]
                    .into_iter()
                    .all(|text| !text.trim().is_empty() && !text.contains('\0')),
            "invalid Defense System messages"
        );
        Ok(())
    }
}

impl<Enemy> Definition<Enemy> {
    /// Cooking names the four members; preparation resolves them back to IDs.
    /// Both use the same controller parameters and serialized structure.
    pub(crate) fn map_enemies<Resolved>(
        self,
        mut resolve: impl FnMut(Enemy) -> Result<Resolved>,
    ) -> Result<Definition<Resolved>> {
        let mut fixed = |member: FixedMember<Enemy>| -> Result<FixedMember<Resolved>> {
            Ok(FixedMember {
                enemy: resolve(member.enemy)?,
                position: member.position,
                heading: member.heading,
            })
        };
        Ok(Definition {
            core: fixed(self.core)?,
            repair: fixed(self.repair)?,
            orbit: resolve(self.orbit)?,
            orbit_count: self.orbit_count,
            arm: resolve(self.arm)?,
            arm_joints: self.arm_joints,
            deployment: self.deployment,
            repair_cycle: self.repair_cycle,
            repair_sound: self.repair_sound,
            repair_sound_priority: self.repair_sound_priority,
            messages: self.messages,
            camera_yaw_limits: self.camera_yaw_limits,
        })
    }
}

impl Definition {
    pub fn contains(&self, enemy: u8) -> bool {
        [self.core.enemy, self.repair.enemy, self.orbit, self.arm].contains(&enemy)
    }

    pub fn fixed_member(&self, enemy: u8) -> Option<FixedMember> {
        [self.core, self.repair]
            .into_iter()
            .find(|member| member.enemy == enemy)
    }

    pub fn validate(&self) -> Result<()> {
        let ids = [self.core.enemy, self.repair.enemy, self.orbit, self.arm];
        ensure!(
            ids.into_iter().collect::<BTreeSet<_>>().len() == ids.len()
                && ids
                    .into_iter()
                    .all(|id| usize::from(id) < crate::monster::MONSTER_COUNT),
            "invalid Defense System member identities"
        );
        ensure!(
            (1..=2).contains(&self.orbit_count),
            "Defense System exceeds formation capacity"
        );
        ensure!(
            self.arm_joints.iter().collect::<BTreeSet<_>>().len() == 4
                && self
                    .arm_joints
                    .iter()
                    .all(|joint| !joint.is_empty() && !joint.chars().any(char::is_control)),
            "invalid Defense System attachment slots"
        );
        for member in [self.core, self.repair] {
            ensure!(
                member.position.into_iter().all(f32::is_finite) && member.heading.is_finite(),
                "invalid Defense System fixed placement"
            );
        }
        let deployment = self.deployment;
        self.repair_cycle.validate()?;
        self.messages.validate()?;
        ensure!(
            self.repair_sound != 0,
            "missing Defense System repair sound"
        );
        ensure!(
            deployment.ticks > 0
                && (1..=100).contains(&deployment.hp_percent)
                && deployment.rate.is_finite()
                && deployment.rate > 0.,
            "invalid Defense System deployment"
        );
        let [start, end] = self.camera_yaw_limits;
        ensure!(
            start.is_finite() && end.is_finite() && start >= 0. && start < end && end <= 360.,
            "invalid Defense System camera limits"
        );
        Ok(())
    }
}
