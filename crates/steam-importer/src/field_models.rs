//! Global actor directories become the same named model catalogue as GameCube.
use crate::{
    actor_model, output,
    package::Package,
    resources::Resources,
    source::{Source, validate_name},
};
use anyhow::{Context, Result};
use resonance_content::field::{
    STANDALONE_MODEL_RESOURCE_BASE,
    document::{self, ModelAlias, ModelBinding, ModelMappings, ModelTarget},
};
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) struct Models {
    pub resources: Resources,
    pub sources: BTreeMap<u32, String>,
    pub mappings: ModelMappings,
    pub standalone: ModelMappings,
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<Models> {
    let resources = Resources::read(source)?;
    let mut mappings: ModelMappings =
        serde_json::from_slice(&fs::read(library.join(document::MODELS))?)?;
    let names: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("../../importer/src/mesh-model-names.json"))?;
    let aliases: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("../../importer/src/mesh-model-aliases.json"))?;
    let mut standalone = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut owners = BTreeMap::<String, String>::new();
    let mut missing = 0;
    for (&id, entry) in resources.entries.iter().filter(|(id, _)| **id >> 16 <= 2) {
        let name = match entry.as_deref() {
            Some(name) => name,
            None => match resources.loaded(id) {
                Ok(name) => name,
                Err(_) => continue,
            },
        };
        if validate_name(name).is_err() {
            continue;
        }
        let resource = if id < 53 {
            STANDALONE_MODEL_RESOURCE_BASE | id
        } else {
            id
        };
        let target = aliases
            .get(&id)
            .or_else(|| names.get(&id))
            .cloned()
            .unwrap_or_else(|| format!("unidentified/model-{id}"));
        let target = if id < 53 {
            format!("standalone/{target}")
        } else {
            target
        };
        let mapping = if let Some(owner) = owners.get(&name.to_ascii_lowercase()) {
            ModelTarget::Alias(ModelAlias {
                name: target,
                model: owner.clone(),
            })
        } else {
            let Some(bytes) = source.read(name)? else {
                missing += 1;
                continue;
            };
            let extension = name
                .rsplit_once('.')
                .map(|(_, ext)| ext.to_ascii_lowercase());
            let is_model = match extension.as_deref() {
                Some("tomdlb_d") => true,
                Some("pac") => {
                    let package = Package::parse(&bytes)?;
                    if package.len() != 31 {
                        continue;
                    }
                    if package.reference(0)?.is_none() {
                        continue;
                    }
                    let mut complete = true;
                    for slot in 0..package.len() {
                        if let Some(name) = package.reference(slot)? {
                            complete &= source.contains(name)?;
                        }
                    }
                    complete
                }
                _ => continue,
            };
            if !is_model {
                missing += 1;
                continue;
            }
            eprintln!("  Field model: {target}");
            let owner = format!("meshes/{target}");
            let (document, _) = actor_model::cook(source, library, name, &owner)
                .with_context(|| format!("global actor {id:#x}: {name}"))?;
            output::json(&library.join(&owner).join("model.json"), &document)?;
            owners.insert(name.to_ascii_lowercase(), target.clone());
            ModelTarget::Model(target)
        };
        if id < 53 {
            standalone.insert(id, mapping.clone());
        }
        sources.insert(resource, name.to_owned());
        mappings.0.insert(resource, mapping);
    }
    mappings.by_name()?;
    output::json(&library.join(document::MODELS), &mappings)?;
    let standalone = ModelMappings(standalone);
    output::json(
        &library.join("meshes/standalone/mappings.json"),
        &standalone,
    )?;
    let party = mappings
        .0
        .iter()
        .filter(|(id, _)| (1..=9).contains(*id) || **id >> 24 == 0x23)
        .map(|(_, target)| ModelBinding::Shared(target.name().to_owned()))
        .collect::<Vec<_>>();
    output::json(
        &library.join(document::MODEL_GROUPS),
        &BTreeMap::from([("party", party)]),
    )?;
    output::write(
        &library.join(document::JOINT_MAPPINGS),
        include_bytes!("../../importer/src/model-joints.json"),
    )?;
    if missing > 0 {
        eprintln!("  Skipped {missing} global entries whose model sources are absent");
    }
    Ok(Models {
        resources,
        sources,
        mappings,
        standalone,
    })
}
