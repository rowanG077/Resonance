//! Enemy gameplay inputs, available independently of body and attachment artwork.
use crate::{
    battle_action::{EnemyActions, TargetPolicy},
    battle_profile::Profile,
};
use serde::{Deserialize, Serialize};

pub fn path(id: u8) -> String {
    format!("battle/enemies/{id:03}/definition.json")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryRow {
    Random,
    Front,
    Middle,
    Back,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definition {
    pub source_sha256: String,
    /// Empty visual slots in the authored body motion table.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub null_motions: std::collections::BTreeSet<u16>,
    pub profile: Profile,
    /// Every decoded projectile row, including unsupported rows, in authored order.
    pub projectiles: Vec<Projectile>,
    pub actions: EnemyActions,
    pub target_strategy: Option<TargetPolicy>,
    pub entry_row: EntryRow,
    pub guard_recovery_bonus: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<Presentation>,
}

/// Optional opening shot and the final enemy's defeat hold.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Presentation {
    pub opening: Option<crate::battle_voice::TimedVoice>,
    pub defeat: crate::battle_voice::TimedVoice,
}

/// An enemy projectile recipe and its contact response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projectile {
    pub definition: crate::battle_projectile::Projectile,
    pub recoil: u8,
}
