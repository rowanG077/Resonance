use crate::library::{self, Snapshot};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs::File;

/// Editable movie choices. Dimensions, lengths and codec facts belong to the
/// selected container, not to a second description that modders must maintain.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub path: String,
    /// Zero-based audio track in container order.
    #[serde(default)]
    pub audio_track: u16,
}

impl Definition {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported movie definition version");
        library::validate_path(&self.path)?;
        ensure!(self.audio_track < 256, "invalid movie audio track");
        Ok(())
    }
}

pub struct Selected {
    pub definition: Definition,
    pub file: File,
}

/// Resolve the descriptor and pin the selected stream without buffering it.
pub fn capture(snapshot: &mut Snapshot, id: u32) -> Result<Selected> {
    let path = snapshot.movie(id)?;
    let definition: Definition = snapshot.json(&path, 64 * 1024)?;
    definition
        .validate()
        .with_context(|| format!("movie {id}: {path}"))?;
    let file = snapshot
        .open_stream(&definition.path)
        .with_context(|| format!("movie {id}: {path} refers to {}", definition.path))?;
    ensure!(
        file.metadata()?.is_file(),
        "movie {} must be a regular file",
        definition.path
    );
    Ok(Selected { definition, file })
}
