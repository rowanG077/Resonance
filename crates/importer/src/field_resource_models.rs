//! Native shared props exist independently of the known-name catalogue.
use crate::{
    compression, dol, field_core, field_npc_models as models, native_archive, text, world_gpl,
};
use anyhow::{Context, Result};
use resonance_content::field::document::{ModelAlias, ModelTarget};
use std::{collections::BTreeMap, fs, path::Path};

fn package(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let bytes = if bytes.starts_with(b"MSCF") {
        compression::cabinet(bytes)?
    } else if bytes.len() >= 9
        && matches!(bytes[0], 0 | 1 | 3)
        && u32::from_le_bytes(bytes[1..5].try_into()?) as usize + 9 == bytes.len()
    {
        compression::decode(bytes)?
    } else {
        bytes.to_vec()
    };
    if world_gpl::is_model(&bytes) {
        return models::expanded_member(&bytes).map(Some);
    }
    if bytes.get(..4) == Some(&31_u32.to_be_bytes()) {
        if let Some(body) = field_core::optional_section(&bytes, 0)? {
            anyhow::ensure!(
                world_gpl::is_model(body),
                "native model package has an invalid body"
            );
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

fn tile_name(id: u32) -> String {
    let mut first = 67867;
    for (count, set) in [
        (9, "9-ag"),
        (9, "9-db"),
        (9, "9-hb"),
        (9, "9-lb"),
        (9, "9-og"),
        (9, "9-pb"),
        (25, "25-lb"),
        (25, "25-lg"),
        (25, "25-mb"),
        (25, "25-mg"),
        (25, "25-mi"),
        (25, "25-pg"),
        (25, "25-ly"),
    ] {
        if (first..first + count).contains(&id) {
            return format!("props/tiles/mini-{set}/tile-{:02}", id - first + 1);
        }
        first += count;
    }
    format!("unidentified/model-{id}")
}

fn shared_models(extracted: &Path) -> Result<BTreeMap<u32, (String, Vec<u8>)>> {
    let known: BTreeMap<u32, String> = serde_json::from_str(include_str!("mesh-model-names.json"))?;
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let bank = models::grouped_bank(extracted, &exe, 1)?;
    let mut result = BTreeMap::new();
    for index in 0..dol::u32(&bank, 0)? {
        let id = 0x10000 + index;
        if known.contains_key(&id) || dol::u32(&bank, 8 + index as usize * 8)? == 0 {
            continue;
        }
        if let Some(package) = package(models::grouped_member(&bank, id)?)
            .with_context(|| format!("classify native model {id}"))?
        {
            result.insert(id, (tile_name(id), package));
        }
    }
    Ok(result)
}

pub(crate) fn mappings(extracted: &Path) -> Result<BTreeMap<u32, ModelTarget>> {
    Ok(shared_models(extracted)?
        .into_iter()
        .map(|(id, (name, _))| (id, ModelTarget::Model(name)))
        .collect())
}

/// Convert omitted native props and the independent standalone namespace.
pub fn cook_native_models(extracted: &Path, library: &Path) -> Result<()> {
    let mut shared = BTreeMap::new();
    for (id, (name, package)) in shared_models(extracted)? {
        if !library.join(format!("meshes/{name}/model.json")).is_file() {
            models::convert_shared(library, id, &name, &package, None, &mut shared)
                .with_context(|| format!("shared native model {id}"))?;
        }
    }
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let files = extracted.join("disc1/files");
    let available = fs::read_dir(&files)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase()))
        .collect::<std::io::Result<std::collections::BTreeSet<_>>>()?;
    let mut targets = BTreeMap::new();
    let mut packages = BTreeMap::<Vec<u8>, String>::new();
    for id in 0..53 {
        let filename = text::pointer(&exe, 0x801f_85e4 + id * 4)?;
        if !available.contains(&filename.to_ascii_lowercase()) {
            continue;
        }
        let Some(package) = package(&native_archive::read_named(&files, &filename)?)
            .with_context(|| format!("classify standalone model {id} ({filename})"))?
        else {
            continue;
        };
        let name = match id {
            15..=17 => format!("characters/dogs/variant-{}", id - 14),
            _ => format!("unidentified/standalone-model-{id}"),
        };
        let target = if let Some(owner) = packages.get(&package) {
            ModelTarget::Alias(ModelAlias {
                name: format!("{owner}/alias-{id}"),
                model: owner.clone(),
            })
        } else {
            if !library.join(format!("meshes/{name}/model.json")).is_file() {
                models::convert_shared(library, id, &name, &package, None, &mut shared)
                    .with_context(|| format!("standalone native model {id}"))?;
            }
            packages.insert(package, name.clone());
            ModelTarget::Model(name)
        };
        targets.insert(id, target);
    }
    crate::output::json(&library.join("meshes/standalone/mappings.json"), &targets)
}

/// Preserve the shared camera trajectories, including previously unnamed ones.
pub fn cook_native_cameras(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let bank = models::grouped_bank(extracted, &exe, 1)?;
    let mut mappings = BTreeMap::new();
    for index in 0..dol::u32(&bank, 0)? {
        if dol::u32(&bank, 8 + index as usize * 8)? == 0 {
            continue;
        }
        let id = 0x10000 + index;
        let source = models::grouped_member(&bank, id)?;
        if !source.starts_with(b"CAMM") {
            continue;
        }
        let name = format!("unidentified-camera-{id}.glb");
        crate::output::write(
            &library.join("cameras").join(&name),
            &crate::world_cinematics::camera(source)
                .with_context(|| format!("camera resource {id}"))?,
        )?;
        mappings.insert(id, name);
    }
    crate::output::json(&library.join("cameras/mappings.json"), &mappings)
}
