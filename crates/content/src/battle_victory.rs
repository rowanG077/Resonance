//! Victory motion packages and regional group-voice descriptors.
//! Selection and native playback are owned by the game and battle runtimes.
use crate::field_preload::File;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PATH: &str = "battle/victory.json";
pub const ORDINARY_GROUPS: [u8; 42] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42,
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Performances<O = Vec<Ordinary>, G = Vec<Group>, F = BTreeMap<String, File>> {
    pub module_sha256: String,
    pub archive_sha256: String,
    pub group_archive_sha256: String,
    #[serde(default)]
    pub ordinary: O,
    #[serde(default)]
    pub groups: G,
    #[serde(default)]
    pub files: F,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ordinary {
    pub character: u8,
    pub selector: u8,
    pub source_sha256: String,
    pub body_sha256: String,
    /// Sparse motion clip for this performance.
    pub motion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: u8,
    /// Character leading the group and providing its voice.
    pub character: u8,
    pub voice: Option<crate::battle_voice::Sound>,
    pub participants: Vec<u8>,
    /// Restrict selection to this battle leader, independently of the speaker.
    pub required_leader: Option<u8>,
    pub condition: Condition,
}

/// Narrative context for a dialogue. Participant availability is checked separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    Always,
    SheenaAffinity,
    RaineFallen,
    SheenaFallen,
    RaineAndSheenaFallen,
    GenisAffinity,
    EnemyScanned,
    LloydOutmatched,
    ZelosAffinity,
    Flawless,
    PreseaDistantBeforeRecovery,
    PreseaCloseAfterRecovery,
    GenisAndKratosDistantFlawless,
    GenisAndZelosDistantFlawless,
    ChildhoodFriendsInjured,
    ColetteAffinity,
    LloydPoisoned,
    ColetteHighParticipationFlawless,
    ColetteFallen,
    GenisJealous,
    PreseaSheenaRainePoisoned,
    GenisInjured,
    RegalRecovered,
    KratosFallen,
    GenisFallenBeforePreseaRecovery,
    PreseaDistantAfterRecoveryFlawless,
    LloydManualPartyAutomatic,
}

impl Group {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..64).contains(&self.id) && (1..=4).contains(&self.participants.len()),
            "invalid victory group identity or participant count"
        );
        ensure!(
            self.participants.iter().enumerate().all(|(index, id)| {
                (1..=9).contains(id) && !self.participants[..index].contains(id)
            }) && self.participants.contains(&self.character),
            "invalid victory group participants"
        );
        ensure!(
            self.required_leader
                .is_none_or(|id| self.participants.contains(&id)),
            "victory selection leader is not a participant"
        );
        Ok(())
    }
}
