//! Shared music mappings point directly to standard WAV recordings.
use crate::library::Snapshot;
use anyhow::{Result, ensure};
use std::fs::File;

pub const MAPPINGS: &str = "audio/music/mappings.json";

/// Pin the selected mod's file without loading its PCM into the library cache.
pub fn capture(snapshot: &mut Snapshot, path: &str, cancelled: &impl Fn() -> bool) -> Result<File> {
    ensure!(!cancelled(), "music preparation cancelled");
    ensure!(path.ends_with(".wav"), "music must reference a WAV file");
    let file = snapshot.open_stream(path)?;
    ensure!(!cancelled(), "music preparation cancelled");
    Ok(file)
}
