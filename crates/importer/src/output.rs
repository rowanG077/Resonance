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

pub(crate) fn redirect_paths(
    value: &mut serde_json::Value,
    paths: &std::collections::BTreeMap<String, String>,
) {
    match value {
        serde_json::Value::String(path) => {
            if let Some(owner) = paths.get(path) {
                *path = owner.clone();
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redirect_paths(value, paths);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                redirect_paths(value, paths);
            }
        }
        _ => {}
    }
}
