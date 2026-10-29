//! Direct writes to final asset paths; an existing file is a collision.
use anyhow::{Context, Result};
use std::{fs, io::Write, path::Path};

pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().context("asset output has no parent")?)?;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .with_context(|| format!("create asset {}", path.display()))?
        .write_all(bytes)
        .with_context(|| format!("write asset {}", path.display()))
}

pub(crate) fn json(path: &Path, document: &impl serde::Serialize) -> Result<()> {
    write(path, &resonance_asset_writer::json::encode(document)?)
}
