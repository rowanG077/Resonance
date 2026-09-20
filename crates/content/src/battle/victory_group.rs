//! Authored party exchanges used by the victory presentation.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const GROUP_COUNT: u8 = 54;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Groups {
    pub source_sha256: String,
    pub motion: GroupMotion,
    pub entries: BTreeMap<u8, Group>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupMotion {
    /// The English release plays the exchange over the already staged poses.
    RetainStagedPose,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// A complete exchange is a single authored voice cue.
    pub voice: u16,
    /// Character IDs in staging order; the first member owns the voice.
    pub members: Vec<u8>,
}

impl Groups {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.source_sha256.len() == 64
                && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid victory group source digest"
        );
        ensure!(
            self.entries.keys().copied().eq(1..=GROUP_COUNT),
            "incomplete victory group inventory"
        );
        for (&id, group) in &self.entries {
            ensure!(
                (1..=4).contains(&group.members.len())
                    && group.members.iter().all(|m| (1..=9).contains(m))
                    && group.members.iter().collect::<BTreeSet<_>>().len() == group.members.len()
                    && group.voice & 0x8000 != 0,
                "invalid victory group {id}"
            );
        }
        Ok(())
    }

    pub fn voices(&self) -> impl Iterator<Item = u16> + '_ {
        self.entries.values().map(|group| group.voice)
    }
}
