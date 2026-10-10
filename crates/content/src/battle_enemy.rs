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
    pub profile: Profile,
    pub actions: EnemyActions,
    pub target_strategy: Option<TargetPolicy>,
    pub entry_row: EntryRow,
    pub guard_recovery_bonus: u8,
}
