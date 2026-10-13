//! Victory motion packages and regional group-voice descriptors.
//! Selection and native playback are owned by the game and battle runtimes.
use crate::field_preload::File;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PATH: &str = "battle/victory.json";
pub const GROUP_COUNT: u8 = 54;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Performances<O = Vec<Ordinary>, G = Vec<Group>, F = BTreeMap<String, File>> {
    pub module_sha256: String,
    pub archive_sha256: String,
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
    /// Authored result updates, independent of the skeletal clip's frame rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<Vec<(u16, Event)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Pose {
        start: f32,
        end: Option<f32>,
        rate: f32,
        blend: u8,
        repeat: bool,
    },
    Expression([u8; 4]),
    /// Resume the actor's ordinary idle after a completed celebration.
    Idle {
        blend: u8,
    },
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
