//! Named actor speech and explicit audio references.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    Cue(u16),
    Stream(u16),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voices {
    pub hurt: [Option<Sound>; 2],
    pub defeat: Option<Sound>,
    pub critical: Option<Sound>,
    pub guard: Option<Sound>,
    pub ally_defeated: [Option<Sound>; 2],
    pub technique_command: Option<Sound>,
    pub item_use: Option<Sound>,
    pub entry: [Option<Sound>; 3],
    pub major_enemy: Option<Sound>,
    pub outnumbered: Option<Sound>,
    pub weaker_enemies: Option<Sound>,
    pub stronger_enemies: Option<Sound>,
    pub repeated_encounter: Option<Sound>,
    pub victory: Vec<Sound>,
    pub interrupted_cast: Option<Sound>,
    pub escape_request: Option<Sound>,
    pub escape_success: Option<Sound>,
    pub escape_cancel: Option<Sound>,
    pub taunt: Option<Sound>,
    pub contact_recovery: Option<Sound>,
}
