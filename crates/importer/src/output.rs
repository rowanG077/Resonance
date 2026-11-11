//! Direct writes to final asset paths, replacing existing cooked output.
use anyhow::{Context, Result};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// JSON written by this process. Publication splits the supported documents
/// among them into owners without searching the library for them.
static DOCUMENTS: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().context("asset output has no parent")?)?;
    fs::write(path, bytes).with_context(|| format!("write asset {}", path.display()))?;
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        DOCUMENTS.lock().unwrap().insert(path.to_owned());
    }
    Ok(())
}

/// Take the JSON written beneath `library`, as `/`-separated library paths.
pub(crate) fn take_documents(library: &Path) -> Vec<String> {
    let relative = |path: PathBuf| {
        Some(
            path.strip_prefix(library)
                .ok()?
                .to_str()?
                .replace('\\', "/"),
        )
    };
    let mut written = DOCUMENTS.lock().unwrap();
    written
        .extract_if(.., |path| path.starts_with(library))
        .filter_map(relative)
        .collect()
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
