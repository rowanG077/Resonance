//! Every enemy action and its authored dependencies, including unresolved recovery.
use super::{
    action_program::CommandProgram,
    actions::{AnimationProgram, HitWindow},
    effects::EffectId,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyInventory {
    pub packages: Vec<EnemyPackageInventory>,
    pub issues: Vec<RecoveryIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyPackageInventory {
    pub monster: u8,
    pub variant_count: u8,
    pub actions: Vec<EnemyActionInventory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyActionInventory {
    pub id: u8,
    pub entry_route: EnemyEntryRoute,
    pub duration: u16,
    /// Authored cost override; zero falls back to the native's first menu record.
    pub tp: u8,
    /// Native arte identity; this requires a separately recovered arte implementation.
    pub native_technique: Option<u16>,
    pub cast_voices: [u16; 2],
    pub effect: Option<EffectId>,
    pub recovery_animation: Option<u8>,
    pub hit_recovery_animation: Option<u8>,
    pub source: EnemyActionSource,
    /// None always has a corresponding contextual recovery issue.
    pub commands: Option<CommandProgram>,
    pub recovery_commands: Option<CommandProgram>,
    pub animations: Option<AnimationProgram>,
    pub hits: Option<Vec<HitWindow>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyEntryRoute {
    Attack,
    Technique,
    MoveAwayFromTarget,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnemyActionSource {
    pub offset: u32,
    pub selection_flags: u32,
    pub animation_index: u16,
    pub command_index: u16,
    pub hit_index: u16,
    pub recovery_command_index: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryIssue {
    pub monster: u8,
    pub action: Option<u8>,
    pub section: RecoverySection,
    pub source_offset: u32,
    pub reason: String,
    pub disposition: RecoveryDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "route", rename_all = "snake_case")]
pub enum RecoveryDisposition {
    RequiredProgram,
    UnusedByEntryRoute(EnemyEntryRoute),
}

impl RecoveryIssue {
    pub fn is_required(&self) -> bool {
        self.disposition == RecoveryDisposition::RequiredProgram
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoverySection {
    Package,
    Commands,
    RecoveryCommands,
    Animations,
    Hits,
}

impl EnemyInventory {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.packages.len() == crate::monster::MONSTER_COUNT,
            "enemy inventory does not cover every monster slot"
        );
        let mut monsters = BTreeSet::new();
        for package in &self.packages {
            ensure!(
                usize::from(package.monster) < crate::monster::MONSTER_COUNT
                    && monsters.insert(package.monster),
                "invalid or duplicate enemy inventory slot"
            );
            if package.variant_count == 0 || package.actions.is_empty() {
                ensure!(
                    self.issues
                        .iter()
                        .any(|issue| issue.monster == package.monster
                            && issue.section == RecoverySection::Package),
                    "enemy inventory silently lost a package"
                );
            }
            for (index, action) in package.actions.iter().enumerate() {
                ensure!(
                    usize::from(action.id) == index,
                    "enemy action identities are not contiguous"
                );
                for (missing, section) in [
                    (action.commands.is_none(), RecoverySection::Commands),
                    (action.animations.is_none(), RecoverySection::Animations),
                    (action.hits.is_none(), RecoverySection::Hits),
                    (
                        action.source.recovery_command_index.is_some()
                            && action.recovery_commands.is_none(),
                        RecoverySection::RecoveryCommands,
                    ),
                ] {
                    ensure!(
                        !missing
                            || self
                                .issues
                                .iter()
                                .any(|issue| issue.monster == package.monster
                                    && issue.action == Some(action.id)
                                    && issue.section == section),
                        "enemy inventory silently lost an action program"
                    );
                }
            }
        }
        Ok(())
    }
}
