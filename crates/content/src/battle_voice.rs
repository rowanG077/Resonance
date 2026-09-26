//! Original voice storage selection. Actor-relative lines use their profile base.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "battle/voices.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub source_sha256: String,
    /// One low-bit-first flag per voice: set for streamed audio, clear for a cue.
    pub streams: Vec<u8>,
    /// Original casting timing, indexed by voice ID with its stream flag removed.
    pub durations: Vec<u16>,
}

impl Table {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.streams.is_empty()
                && self.streams.len() <= 8192
                && !self.durations.is_empty()
                && self.durations.len() <= 32768
                && self.durations.len() <= self.streams.len() * 8,
            "invalid battle voice table"
        );
        Ok(())
    }
}
