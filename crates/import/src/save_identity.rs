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
        // Award thresholds change future eligibility, not the meaning of stored
        // title IDs. Keep this identity boundary when moving native rules into
        // cooked title_awards, so this representation change preserves saves.
        &session.items,
        &session.characters,
        &session.experience,
        &rules.items,
        &rules.titles,
        &rules.techniques,
        &rules.strategy,
        &rules.cooking,
        world_map_identity(&rules.world_map),
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

/// Visit groups govern future discoveries; the saved shop IDs retain their meaning.
/// Keep the original ordered identity projection when externalizing those rules.
#[derive(serde::Serialize)]
struct WorldMapIdentity<'a> {
    locations: &'a BTreeMap<u16, resonance_content::menu_data::MapLocation>,
    field_locations: &'a BTreeMap<u32, u16>,
    shops: &'a [resonance_content::menu_data::Shop],
}

fn world_map_identity(data: &resonance_content::menu_data::WorldMapData) -> WorldMapIdentity<'_> {
    WorldMapIdentity {
        locations: &data.locations,
        field_locations: &data.field_locations,
        shops: &data.shops,
    }
}

#[test]
fn visit_groups_preserve_saved_identity_but_stock_changes_do_not() -> Result<()> {
    use resonance_content::menu_data::{Shop, WorldMapData};
    let mut data = WorldMapData {
        locations: BTreeMap::new(),
        field_locations: BTreeMap::new(),
        shops: vec![Shop { items: vec![1] }; 2],
        shop_visit_groups: vec![],
    };
    let before = serde_json::to_vec(&world_map_identity(&data))?;
    assert_eq!(
        before,
        br#"{"locations":{},"field_locations":{},"shops":[{"items":[1]},{"items":[1]}]}"#
    );
    data.shop_visit_groups = vec![vec![0, 1]];
    data.validate(2)?;
    assert_eq!(serde_json::to_vec(&world_map_identity(&data))?, before);
    data.shops[1].items = vec![2];
    assert_ne!(serde_json::to_vec(&world_map_identity(&data))?, before);
    Ok(())
}

fn record(hash: &mut Sha256, name: &str, bytes: &[u8]) {
    hash.update((name.len() as u64).to_be_bytes());
    hash.update(name.as_bytes());
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}
