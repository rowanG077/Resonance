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

/// Convert a relative filesystem path into the portable spelling used in assets.
pub(crate) fn asset_path(path: &Path) -> Result<String> {
    let logical = path
        .to_str()
        .context("non-UTF-8 asset path")?
        .replace('\\', "/");
    resonance_content::library::validate_path(&logical)?;
    Ok(logical)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filesystem_asset_paths_use_portable_separators_and_stay_relative() -> Result<()> {
        for path in [
            "meshes/monsters/assassin/animations/motion-23.glb",
            r"meshes/monsters\assassin\animations\motion-23.glb",
            r"meshes\monsters\assassin\animations\motion-23.glb",
        ] {
            assert_eq!(
                asset_path(Path::new(path))?,
                "meshes/monsters/assassin/animations/motion-23.glb"
            );
        }
        for path in [
            "",
            "../outside.glb",
            r"..\outside.glb",
            "/absolute.glb",
            r"C:\asset.glb",
        ] {
            assert!(asset_path(Path::new(path)).is_err(), "{path}");
        }
        Ok(())
    }
}
