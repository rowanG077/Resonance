//! File output shared by capture tools; scheduling and completion stay with callers.
use anyhow::Result;
use bevy::prelude::Image;
use std::{fs, path::Path};

pub(super) fn write(
    image: &Image,
    path: &Path,
    metadata: Option<&serde_json::Value>,
) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    image.clone().try_into_dynamic()?.save(path)?;
    if let Some(metadata) = metadata {
        fs::write(
            path.with_extension("json"),
            serde_json::to_vec_pretty(metadata)?,
        )?;
    }
    Ok(())
}
