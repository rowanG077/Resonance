//! Publish saved-state identity once from cooked gameplay definitions and scripts.
use anyhow::{Result, ensure};
use resonance_content::{
    field::FieldAssets,
    menu_data::MenuData,
    save_identity::{Identity, PATH, SCHEMA},
    session::SessionData,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(crate) fn cook(
    output: &Path,
    fields: &BTreeMap<u32, BTreeSet<String>>,
    shared: &BTreeSet<String>,
) -> Result<()> {
    let identity = prepare(fields.keys().copied(), shared, |path| {
        Ok(fs::read(output.join(path))?)
    })?;
    crate::write_atomic(&output.join(PATH), &serde_json::to_vec(&identity)?)
}

/// Build the save identity from one publication snapshot, including staged content.
pub fn prepare(
    fields: impl IntoIterator<Item = u32>,
    shared: &BTreeSet<String>,
    read: impl Fn(&str) -> Result<Vec<u8>>,
) -> Result<Identity> {
    let session: SessionData = serde_json::from_slice(&read("game/session-data.json")?)?;
    let mut rules: MenuData = serde_json::from_slice(&read("game/menu-data.json")?)?;
    rules.validate_gameplay()?;
    // Display-only equipment caption identifiers do not change saved-state meaning.
    for item in &mut rules.items {
        item.properties.caption_ids.clear();
    }
    let gameplay = serde_json::to_vec(&(
        &session.items,
        &session.characters,
        &session.experience,
        &rules.items,
        &rules.titles,
        &rules.techniques,
        &rules.strategy,
        &rules.cooking,
        &rules.world_map,
        &rules.ex_skills,
    ))?;
    let mut hash = Sha256::new();
    hash.update(b"Resonance native saved state");
    hash.update(SCHEMA.to_be_bytes());
    record(&mut hash, "gameplay", &gameplay);
    for map in fields {
        let field: FieldAssets =
            serde_json::from_slice(&read(&resonance_content::field::metadata_path(map))?)?;
        ensure!(field.map_id == map, "save identity field binding differs");
        field.validate()?;
        record(&mut hash, &format!("field/{map}"), &read(&field.script)?);
    }
    for path in shared.iter().filter(|path| {
        path.ends_with(".sym")
            && (path.starts_with("scripts/battle/") || path.starts_with("scripts/std/"))
    }) {
        record(&mut hash, path, &read(path)?);
    }
    Ok(Identity {
        schema: SCHEMA,
        content: hash.finalize().into(),
    })
}

fn record(hash: &mut Sha256, name: &str, bytes: &[u8]) {
    hash.update((name.len() as u64).to_be_bytes());
    hash.update(name.as_bytes());
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}
