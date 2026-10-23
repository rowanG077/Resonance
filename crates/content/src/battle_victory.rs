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
    pub selection: SelectionRule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SelectionRule {
    Encounter { priority: u8, formations: Vec<u16> },
    Ordinary { priority: u8, gate: RandomGate },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RandomGate {
    pub mask: u16,
    pub accepted: u16,
}

impl RandomGate {
    pub fn accepts(self, sample: u16) -> bool {
        sample & self.mask == self.accepted
    }
}

impl SelectionRule {
    pub fn priority(&self) -> u8 {
        match self {
            Self::Encounter { priority, .. } | Self::Ordinary { priority, .. } => *priority,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Encounter { formations, .. } => ensure!(
                !formations.is_empty()
                    && formations
                        .iter()
                        .enumerate()
                        .all(|(index, id)| *id != 0 && !formations[..index].contains(id)),
                "invalid victory encounter restrictions"
            ),
            Self::Ordinary { gate, .. } => ensure!(
                gate.mask != 0 && gate.accepted & !gate.mask == 0,
                "invalid victory random gate"
            ),
        }
        Ok(())
    }

    /// Equal priorities are ambiguous only when both rules can be considered together.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.priority() == other.priority()
            && match (self, other) {
                (Self::Ordinary { .. }, Self::Ordinary { .. }) => true,
                (Self::Encounter { formations: a, .. }, Self::Encounter { formations: b, .. }) => {
                    a.iter().any(|formation| b.contains(formation))
                }
                _ => false,
            }
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn gate_masks_and_overlapping_priorities_are_validated() -> Result<()> {
        let ordinary = SelectionRule::Ordinary {
            priority: 0,
            gate: RandomGate {
                mask: 4,
                accepted: 4,
            },
        };
        ordinary.validate()?;
        assert_eq!(
            (0..8)
                .filter(|&sample| RandomGate {
                    mask: 4,
                    accepted: 4
                }
                .accepts(sample))
                .count(),
            4
        );
        for gate in [
            RandomGate {
                mask: 0,
                accepted: 0,
            },
            RandomGate {
                mask: 3,
                accepted: 4,
            },
        ] {
            assert!(
                SelectionRule::Ordinary { priority: 0, gate }
                    .validate()
                    .is_err()
            );
        }
        for formations in [vec![], vec![0], vec![31, 31]] {
            assert!(
                SelectionRule::Encounter {
                    priority: 0,
                    formations
                }
                .validate()
                .is_err()
            );
        }
        let encounter = SelectionRule::Encounter {
            priority: 0,
            formations: vec![31],
        };
        encounter.validate()?;
        assert!(ordinary.conflicts_with(&ordinary));
        assert!(encounter.conflicts_with(&encounter));
        assert!(!encounter.conflicts_with(&ordinary));
        assert!(!encounter.conflicts_with(&SelectionRule::Encounter {
            priority: 0,
            formations: vec![900]
        }));
        Ok(())
    }
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
        self.selection.validate()?;
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
