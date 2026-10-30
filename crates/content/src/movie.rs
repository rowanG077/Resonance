use crate::library::Snapshot;
use anyhow::{Context, Result};
use std::fs::File;

/// A movie mapping selects the media file itself. The first audio track is used.
pub fn capture(snapshot: &mut Snapshot, id: u32) -> Result<File> {
    let path = snapshot.movie(id)?;
    snapshot
        .open_stream(&path)
        .with_context(|| format!("movie {id}: {path}"))
}
