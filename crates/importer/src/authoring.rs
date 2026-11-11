//! Direct native conversion followed by granular source publication.
use anyhow::{Context, Result};
use std::{collections::BTreeSet, fs, path::Path};

/// Run one category and overwrite its generated source owners on success.
pub fn cook_incremental(library: &Path, cook: impl FnOnce() -> Result<()>) -> Result<()> {
    cook()?;
    publish(library)
}

pub(crate) fn document_exists(library: &Path, path: &str) -> bool {
    library.join(path).is_file()
        || source_paths(library, path).is_ok_and(|paths| {
            resonance_content::authoring::owned_sources(&paths).contains_key(path)
        })
}

/// JSON files beneath a document's source directory, as library paths.
fn source_paths(library: &Path, path: &str) -> Result<BTreeSet<String>> {
    let directory = path
        .strip_suffix(".json")
        .filter(|_| resonance_content::authoring::supports(path))
        .context("document has no granular ownership schema")?;
    Ok(json_paths(&library.join(directory))?
        .into_iter()
        .map(|path| format!("{directory}/{path}"))
        .collect())
}

/// Read a converter's current output or assemble its existing granular owners.
/// Only the requested document is opened; unrelated sources are left alone.
pub(crate) fn read_document(library: &Path, path: &str) -> Result<Vec<u8>> {
    resonance_content::library::validate_path(path)?;
    match fs::read(library.join(path)) {
        Ok(bytes) => return Ok(bytes),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && resonance_content::authoring::supports(path) => {}
        Err(error) => return Err(error).with_context(|| format!("read cooked {path}")),
    }
    let paths = source_paths(library, path)?;
    let document = resonance_content::authoring::assemble(path, &paths, |path| {
        Ok(serde_json::from_slice(&fs::read(library.join(path))?)?)
    })?;
    Ok(serde_json::to_vec(&document)?)
}

/// Decode a converter's document, whether still whole or already granular.
pub(crate) fn read_json<T: serde::de::DeserializeOwned>(library: &Path, path: &str) -> Result<T> {
    serde_json::from_slice(&read_document(library, path)?)
        .with_context(|| format!("decode cooked {path}"))
}

/// Split the whole documents this process wrote into their owners.
pub(crate) fn publish(library: &Path) -> Result<()> {
    for path in crate::output::take_documents(library) {
        // A converter may write a document and later remove or replace it.
        if !resonance_content::authoring::supports(&path) || !library.join(&path).is_file() {
            continue;
        }
        let document = serde_json::from_slice(&fs::read(library.join(&path))?)
            .with_context(|| format!("publish granular owners for {path}"))?;
        publish_document(library, &path, &document)?;
    }
    Ok(())
}

/// Replace one document's granular owners while preserving unrelated documents
/// and media in the destination. Every other JSON file in its source directory
/// is retired, including owners of an earlier layout: the game rejects strays.
pub fn publish_document(library: &Path, path: &str, document: &serde_json::Value) -> Result<()> {
    resonance_content::library::validate_path(path)?;
    let previous = source_paths(library, path)?;
    let files = resonance_content::authoring::publish(library, path, document)?;
    for path in previous.difference(&files) {
        fs::remove_file(library.join(path))?;
    }
    let physical = library.join(path);
    if physical.is_file() {
        fs::remove_file(physical)?;
    }
    Ok(())
}

fn json_paths(library: &Path) -> Result<BTreeSet<String>> {
    let mut paths = file_paths(library)?;
    paths.retain(|path| path.ends_with(".json"));
    Ok(paths)
}

/// Every file beneath `root`, as `/`-separated relative paths.
pub(crate) fn file_paths(root: &Path) -> Result<BTreeSet<String>> {
    if !root.is_dir() {
        return Ok(BTreeSet::new());
    }
    let mut pending = vec![root.to_owned()];
    let mut paths = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                paths.insert(
                    entry
                        .path()
                        .strip_prefix(root)?
                        .to_str()
                        .context("non-UTF-8 authored path")?
                        .replace('\\', "/"),
                );
            }
        }
    }
    Ok(paths)
}

pub(crate) fn clear_previous(library: &Path) -> Result<()> {
    let paths = json_paths(library)?;
    let owned = resonance_content::authoring::owned_sources(&paths);
    // Retire owners and flat documents alike: a fresh cook must not revive an orphan.
    let mut retired: BTreeSet<_> = owned.values().flatten().collect();
    retired.extend(
        paths
            .iter()
            .filter(|path| resonance_content::authoring::supports(path)),
    );
    for path in retired {
        fs::remove_file(library.join(path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn republication_retires_stale_owners_and_preserves_other_banks_and_media() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        let items = json!({"mappings":{"0":"gel","1":"berry"},"items":{"gel":{"category":"gel"},"berry":{"category":"fruit"}},"equipment_descriptions":{}});
        crate::output::json(&root.join("game/items.json"), &items)?;
        // An entry called "effects" is owned data, not another bank document.
        let effects = json!({"programs":[[]],"templates":{"effects":{"kind":"camera_shake","duration":7,"amplitude":3}}});
        let bank = "battle/effects/common/effects.json";
        crate::output::json(&root.join(bank), &effects)?;
        crate::output::write(&root.join("game/items/artist.ktx2"), b"unrelated image")?;
        publish(root)?;
        let read = |path: &str| -> Result<serde_json::Value> {
            Ok(serde_json::from_slice(&read_document(root, path)?)?)
        };
        assert!(!root.join("game/items.json").exists());
        assert_eq!(read("game/items.json")?, items);

        let fewer = json!({"mappings":{"0":"gel"},"items":{"gel":{"category":"gel"}},"equipment_descriptions":{}});
        cook_incremental(root, || {
            crate::output::json(&root.join("game/items.json"), &fewer)
        })?;
        assert!(!root.join("game/items/items/berry/definition.json").exists());
        assert_eq!(read("game/items.json")?, fewer);
        assert_eq!(read(bank)?, effects);
        assert!(root.join("game/items/artist.ktx2").is_file());
        Ok(())
    }

    #[test]
    fn id_mappings_publish_one_owner_per_id_beside_other_documents() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        // A bank called "mappings" stays an owner of the bank document.
        let banks = json!({"mappings":"textures/a.ktx2"});
        crate::output::json(&root.join("textures/banks.json"), &banks)?;
        let enemy = json!({"rules":{},"artwork":{},"effects":{}});
        crate::output::json(&root.join("battle/enemies/wolf/enemy.json"), &enemy)?;
        let path = "battle/enemies/mappings.json";
        crate::output::json(&root.join(path), &json!({"1":"wolf","2":"bear"}))?;
        publish(root)?;
        assert!(root.join("battle/enemies/mappings/2.json").is_file());
        assert!(document_exists(root, path) && !root.join(path).exists());
        cook_incremental(root, || {
            crate::output::json(&root.join(path), &json!({"1":"wolf"}))
        })?;
        let read = |path| read_json::<serde_json::Value>(root, path);
        assert_eq!(read(path)?, json!({"1":"wolf"}));
        assert_eq!(read("textures/banks.json")?, banks);
        assert_eq!(read("battle/enemies/wolf/enemy.json")?, enemy);
        Ok(())
    }
}
