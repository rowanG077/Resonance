//! Write final editable assets directly, replacing earlier output.
use anyhow::{Context, Result};
use std::{fs, path::Path};

pub(crate) fn json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    fs::create_dir_all(path.parent().context("asset output has no parent")?)?;
    fs::write(path, resonance_asset_writer::json::encode(value)?)
        .with_context(|| format!("write asset {}", path.display()))
}
