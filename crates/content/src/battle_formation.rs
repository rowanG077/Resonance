//! Original formation declarations, independent of battle activation and save state.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "battle/formations.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Formations {
    pub source_sha256: String,
    pub records: Vec<Formation>,
}

/// All source slots remain present, including inactive values. Preparing a
/// selected encounter resolves its active resource slots to enemy definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Formation {
    pub actor_count: u8,
    pub resource_count: u8,
    pub flags: u8,
    pub hidden_names: u8,
    pub resources: [i16; 4],
    pub actors: [Actor; 8],
    pub storage: [u8; 8],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub resource: u8,
    pub appearance: u8,
    pub variant: u8,
    pub attachments: [u8; 2],
    pub position: [i16; 2],
}

impl Formation {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.actor_count <= 8 && self.resource_count <= 4,
            "invalid formation slot counts"
        );
        ensure!(
            self.actors[..usize::from(self.actor_count)]
                .iter()
                .all(|actor| actor.resource < self.resource_count),
            "formation refers to a missing enemy slot"
        );
        Ok(())
    }
}

impl Formations {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.source_sha256.len() == 64
                && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && !self.records.is_empty()
                && self.records.len() <= usize::from(u16::MAX) + 1,
            "invalid formation catalogue"
        );
        for record in &self.records {
            record.validate()?;
        }
        Ok(())
    }
}
