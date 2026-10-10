//! Active encounter participants and their resource dependencies.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "battle/formations.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Formations {
    pub source_sha256: String,
    pub records: Vec<Formation>,
}

/// Declared resources include reserves with no initial actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Formation {
    pub settings: Settings,
    pub resources: Vec<Resource>,
    pub actors: Vec<Actor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub play_music: bool,
    pub celebrate: bool,
    pub entry_voice: bool,
    pub escape_restricted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub enemy: u16,
    pub hidden_name: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub resource: u8,
    pub variant: u8,
    pub position: Option<[i16; 2]>,
    pub unsupported_reason: Option<String>,
}

impl Formation {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.actors
                .iter()
                .all(|actor| usize::from(actor.resource) < self.resources.len()),
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
