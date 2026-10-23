//! Explicit saved-identity repair; ordinary loading never invokes this module.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::FieldAssets,
    field_preload::Shared,
    menu_data::{self, MenuData},
    prepared::Files,
    save_identity::{Identity, PATH, SCHEMA},
    session::SessionData,
};
use resonance_game::Checkpoint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
    sync::Arc,
};

/// Evidence for a fully validated copy, including the bytes a repair will write.
#[derive(Debug, Serialize)]
pub struct IdentityRepair {
    pub previous: Identity,
    pub current: Identity,
    pub input_sha256: String,
    pub output_sha256: String,
    pub fields_compared: usize,
    pub shared_scripts_compared: usize,
}

/// Validate the proposed repair without creating or changing any save file.
pub fn check_checkpoint_identity(
    input: &Path,
    source_library: &Path,
    target_library: &Path,
) -> Result<IdentityRepair> {
    prepare(input, source_library, target_library).map(|(report, _)| report)
}

/// Repair a stale fingerprint only when the supplied libraries give identical
/// meanings to saved state. Preserve the original and refuse an existing output.
/// This supports schema 5 and menu formats 34–37; it is not a general save upgrader.
pub fn repair_checkpoint_identity(
    input: &Path,
    source_library: &Path,
    target_library: &Path,
    output: &Path,
) -> Result<IdentityRepair> {
    ensure!(!output.try_exists()?, "migration output already exists");
    let (report, bytes) = prepare(input, source_library, target_library)?;
    // A temporary file plus noclobber publication protects both the input and
    // any concurrently created output, including aliases and symlinks.
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(output)
        .context("publish migrated copy without replacing a file")?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(report)
}

fn prepare(
    input: &Path,
    source_library: &Path,
    target_library: &Path,
) -> Result<(IdentityRepair, Vec<u8>)> {
    let input_bytes = resonance_persistence::read_bounded(input)?;
    let source: Identity = read_json(source_library, PATH)?;
    source.validate()?;
    let original = resonance_persistence::decode::<Checkpoint>(&input_bytes)?;
    let (_, checkpoint) = original.admit(&source)?;
    let old = fingerprint(source_library)?;
    let new = fingerprint(target_library)?;
    let target: Identity = read_json(target_library, PATH)?;
    target.validate()?;
    ensure!(
        target == new.identity,
        "target library has a stale saved-state fingerprint; recook it first"
    );
    ensure!(
        old.identity == new.identity,
        "source and target saved-state definitions or scripts differ"
    );
    ensure!(source != target, "save already has the target identity");

    // Keep raw JSON state and its numeric precision, rather than
    // serializing the typed checkpoint back through f32 fields.
    let mut document = resonance_persistence::decode::<serde_json::Value>(&input_bytes)?;
    document.header.identity = target.clone();
    let bytes = resonance_persistence::encode(&document.header, &document.state)?;
    let (_, admitted) = resonance_persistence::decode::<Checkpoint>(&bytes)?.admit(&target)?;
    ensure!(
        serde_json::to_value(&checkpoint)? == serde_json::to_value(&admitted)?,
        "migration changed checkpoint state"
    );
    validate_prepared(target_library, &target, admitted)?;

    let report = IdentityRepair {
        previous: source,
        current: target,
        input_sha256: format!("{:x}", Sha256::digest(&input_bytes)),
        output_sha256: format!("{:x}", Sha256::digest(&bytes)),
        fields_compared: old.fields,
        shared_scripts_compared: old.shared_scripts,
    };
    Ok((report, bytes))
}

struct Fingerprint {
    identity: Identity,
    fields: usize,
    shared_scripts: usize,
}

// This is the bounded legacy schema-5 identity projection. Presentation, future
// title eligibility and shop visit groups are outside that saved-state boundary.
// Comparing the target recomputation with its cooked identity guards the order
// and serialization contract against drift from the importer's implementation.
#[derive(Deserialize)]
struct SavedRules {
    version: u32,
    items: Vec<menu_data::Item>,
    titles: Vec<Vec<menu_data::Title>>,
    techniques: Vec<menu_data::Technique>,
    strategy: menu_data::StrategyData,
    cooking: menu_data::CookingData,
    world_map: SavedWorldMap,
    ex_skills: menu_data::ExSkillData,
}

#[derive(Deserialize, Serialize)]
struct SavedWorldMap {
    locations: BTreeMap<u16, menu_data::MapLocation>,
    field_locations: BTreeMap<u32, u16>,
    shops: Vec<menu_data::Shop>,
}

fn fingerprint(root: &Path) -> Result<Fingerprint> {
    let session: SessionData = read_json(root, "game/session-data.json")?;
    let mut rules: SavedRules = read_json(root, "game/menu-data.json")?;
    ensure!(
        (34..=37).contains(&rules.version),
        "unsupported source menu format for identity repair"
    );
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
    let mut fields = BTreeSet::new();
    for entry in fs::read_dir(root.join("fields"))? {
        let name = entry?.file_name();
        if let Some(map) = name
            .to_str()
            .and_then(|s| s.strip_prefix("map-"))
            .and_then(|s| s.strip_suffix(".json"))
            .and_then(|s| s.parse::<u32>().ok())
        {
            fields.insert(map);
        }
    }
    ensure!(!fields.is_empty(), "library has no fields");
    for &map in &fields {
        let field: FieldAssets = read_json(root, &resonance_content::field::metadata_path(map))?;
        field.validate()?;
        ensure!(field.map_id == map, "field identity binding differs");
        resonance_content::validate_asset_path(&field.script)?;
        record(
            &mut hash,
            &format!("field/{map}"),
            &fs::read(root.join(&field.script))?,
        );
    }
    let shared: Shared = read_json(root, resonance_content::field_preload::SHARED_PATH)?;
    shared.validate()?;
    let scripts: Vec<_> = shared
        .files
        .keys()
        .filter(|path| {
            path.ends_with(".sym")
                && (path.starts_with("scripts/battle/") || path.starts_with("scripts/std/"))
        })
        .collect();
    for path in &scripts {
        record(&mut hash, path, &fs::read(root.join(path))?);
    }
    Ok(Fingerprint {
        identity: Identity {
            schema: SCHEMA,
            content: hash.finalize().into(),
        },
        fields: fields.len(),
        shared_scripts: scripts.len(),
    })
}

fn record(hash: &mut Sha256, name: &str, bytes: &[u8]) {
    hash.update((name.len() as u64).to_be_bytes());
    hash.update(name.as_bytes());
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

fn read_json<T: serde::de::DeserializeOwned>(root: &Path, path: &str) -> Result<T> {
    serde_json::from_slice(&fs::read(root.join(path))?)
        .with_context(|| format!("read {}", root.join(path).display()))
}

fn validate_prepared(root: &Path, expected: &Identity, checkpoint: Checkpoint) -> Result<()> {
    let mut paths = Vec::new();
    if let Checkpoint::Field(saved) = &checkpoint {
        paths.push(super::new_game::manifest_path(saved.map_id));
        if saved.map_id == 5 {
            paths.push(super::new_game::manifest_path(340));
        }
    }
    let mut cache = super::loading::Cache::default();
    let files = Arc::new(Files::load_with_diagnostics(
        root,
        &paths.iter().map(String::as_str).collect::<Vec<_>>(),
        &mut cache.bytes,
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?);
    ensure!(
        &Identity::load(&files)? == expected,
        "target identity changed during validation"
    );
    // Require current format and normal native state validation; legacy support
    // above applies solely to comparison of the supplied source library.
    MenuData::load(&files)?.validate_gameplay()?;
    match checkpoint {
        Checkpoint::Field(saved) => {
            super::new_game::Session::load_prepared(root, files, Some(saved), None, &mut cache)?;
        }
        Checkpoint::World(saved) => {
            super::new_game::Session::load_world_prepared(root, files, saved, &mut cache, || {
                false
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
