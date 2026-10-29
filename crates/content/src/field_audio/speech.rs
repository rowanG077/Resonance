//! Speech mappings contain WAV paths or exact silence clocks, never empty audio files.
use crate::{
    library::{Snapshot, validate_path},
    preparation::JSON_LIMIT,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Mappings = BTreeMap<u32, Source>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Source {
    Wave(String),
    Silence { name: String, silence: Clock },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clock {
    pub frames: u32,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Clock {
    pub fn validate(self) -> Result<()> {
        ensure!(
            (1..=32_000_000).contains(&self.frames)
                && (8000..=96000).contains(&self.sample_rate)
                && (1..=2).contains(&self.channels),
            "invalid speech silence clock"
        );
        Ok(())
    }
}

impl Source {
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Wave(path) => Some(path),
            Self::Silence { .. } => None,
        }
    }
    pub fn resolve(&self) -> Result<Self> {
        Ok(match self {
            Self::Wave(path) => {
                validate_path(path)?;
                Self::Wave(format!("audio/speech/{path}"))
            }
            Self::Silence { name, silence } => {
                validate_path(name)?;
                silence.validate()?;
                self.clone()
            }
        })
    }
}

pub fn read(snapshot: &mut Snapshot) -> Result<Mappings> {
    snapshot.json(super::document::SPEECH, JSON_LIMIT)
}
