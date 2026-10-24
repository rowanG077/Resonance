//! Recorded background music. File facts come from the WAV, not author metadata.
use crate::{
    library::{Snapshot, validate_path},
    preparation::JSON_LIMIT,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs::File;

pub const VERSION: u32 = 1;
pub const MAPPINGS: &str = "audio/music/mappings.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loop {
    pub start_frame: u32,
    pub end_frame: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub audio: String,
    #[serde(default, rename = "loop", skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Loop>,
    /// Only tracks that inherit a studio need a second original recording.
    /// A replacement may omit it and use its main recording in every context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battle: Option<Variant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub audio: String,
    #[serde(default, rename = "loop", skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Loop>,
}

pub struct Captured {
    pub definition: Definition,
    pub audio: File,
    pub battle: Option<File>,
}

impl Definition {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "unsupported music recording version"
        );
        for (path, repeat) in std::iter::once((&self.audio, self.repeat)).chain(
            self.battle
                .iter()
                .map(|variant| (&variant.audio, variant.repeat)),
        ) {
            validate_path(path)?;
            if let Some(repeat) = repeat {
                ensure!(
                    repeat.start_frame < repeat.end_frame,
                    "empty or reversed music loop in {path}"
                );
            }
        }
        Ok(())
    }
}

pub fn capture(
    snapshot: &mut Snapshot,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Captured> {
    ensure!(!cancelled(), "music preparation cancelled");
    let definition: Definition = snapshot.json(path, JSON_LIMIT)?;
    definition.validate()?;
    let audio = snapshot.open_stream(&definition.audio)?;
    let battle = definition
        .battle
        .as_ref()
        .map(|variant| snapshot.open_stream(&variant.audio))
        .transpose()?;
    ensure!(!cancelled(), "music preparation cancelled");
    Ok(Captured {
        definition,
        audio,
        battle,
    })
}
