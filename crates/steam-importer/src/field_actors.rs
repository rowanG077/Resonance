//! Field-owned actor selections and baked motion bindings in the shared format.
use crate::{
    actor_model, animation::Animation, field_models::Models, model, output, package::Package, pe,
    source::Source,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::Skeleton,
    appearance::costume_resource,
    field::{
        LOCAL_MODEL_RESOURCES, STANDALONE_MODEL_RESOURCE_BASE,
        document::{self, Actor, Actors, AnimationBinding, ModelAlias, ModelBinding, ModelTarget},
    },
    library::{FIELDS, Mappings},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_script::{NativeCall, Program, scenario};

pub(crate) fn cook(source: &Source, library: &Path, models: &mut Models) -> Result<()> {
    let fields: Mappings = serde_json::from_slice(&fs::read(library.join(FIELDS))?)?;
    let image = pe::Image::parse(&source.executable)?;
    let mut motions = BTreeMap::<(String, String), String>::new();
    let mut pending = BTreeSet::new();
    let mut rigs = BTreeMap::<String, (model::Rig, Skeleton)>::new();
    let mut owners = models
        .sources
        .iter()
        .map(|(&id, name)| Ok((name.to_ascii_lowercase(), models.mappings.model_path(id)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    for (id, name) in fields.iter() {
        eprintln!("  Field actors: {name} ({id})");
        let bytes =
            source.require(image.text(pe::word(image.slice(0x8cb908 + id * 24, 4)?, 0)?)?)?;
        let package = Package::parse(&bytes)?;
        let root = format!("fields/{name}");
        let script = fs::read(library.join(&root).join("scripts/scenario.ssb"))?;
        let mut actors = Actors {
            ..Default::default()
        };
        actors.models.push(ModelBinding::Group {
            group: "party".into(),
        });
        let mut selections = BTreeMap::<u32, (String, String)>::new();
        for character in 1..=9u32 {
            for costume in 0..7 {
                let resource = costume_resource(character, costume);
                let Some(target) = models.mappings.0.get(&resource) else {
                    continue;
                };
                let at = 0x8cfe80 + ((character - 1) * 7 + u32::from(costume)) * 4;
                let source = image.text(pe::word(image.slice(at, 4)?, 0)?)?;
                selections.insert(
                    resource,
                    (
                        format!("meshes/{}/model.json", target.model()),
                        source.to_owned(),
                    ),
                );
            }
        }
        let mut local = BTreeMap::new();
        if let Some(name) = package.reference(7)? {
            let bytes = source.require(name)?;
            let bank = Package::parse(&bytes)?;
            if bank.len() > 1 {
                if name.to_ascii_lowercase().ends_with(".pac") {
                    let ids =
                        source.require(bank.reference(0)?.context("field bank lacks ID table")?)?;
                    for (slot, resource) in ids_table(&ids, bank.len())?.into_iter().enumerate() {
                        local.insert(
                            resource,
                            bank.reference(slot + 1)?
                                .context("null field bank member")?
                                .to_owned(),
                        );
                    }
                } else {
                    pending.insert(name.to_owned());
                }
            }
        }
        for slot in 16..package.len() {
            if let Some(name) = package.reference(slot)? {
                if !name.to_ascii_lowercase().ends_with(".cam") {
                    local.insert(
                        LOCAL_MODEL_RESOURCES.start + (slot - 16) as u32,
                        name.to_owned(),
                    );
                }
            }
        }
        let local_ids = local.keys().copied().collect::<BTreeSet<_>>();
        actors
            .local_resources
            .extend(local_ids.iter().copied().filter(|id| *id < 53));
        let (declared, dynamic) = declarations(&script)?;
        actors.dynamic_resources = dynamic;
        for resource in declared {
            if local_ids.contains(&resource) {
                continue;
            }
            if resource < 53 {
                if models.standalone.0.contains_key(&resource) {
                    let source_name = models.resources.loaded(resource)?.to_owned();
                    let path = models.standalone.model_path(resource)?;
                    selections.insert(
                        STANDALONE_MODEL_RESOURCE_BASE | resource,
                        (path, source_name),
                    );
                    continue;
                }
            } else if models.mappings.0.contains_key(&resource) {
                selections.insert(
                    resource,
                    (
                        models.mappings.model_path(resource)?,
                        models
                            .sources
                            .get(&resource)
                            .context("actor has no source binding")?
                            .clone(),
                    ),
                );
                continue;
            }
            match models.resources.loaded(resource) {
                Ok(name) => {
                    local.insert(resource, name.to_owned());
                }
                Err(_) => {
                    pending.insert(format!("field {id} resource {resource:#x}"));
                }
            }
        }
        let mut banks = BTreeMap::new();
        for (resource, source_name) in local {
            let extension = source_name
                .rsplit_once('.')
                .map(|(_, e)| e.to_ascii_lowercase())
                .unwrap_or_default();
            if extension == "toanmb" {
                banks.insert(resource, source_name);
                continue;
            }
            if matches!(extension.as_str(), "totexb_d" | "cam") {
                continue;
            }
            if extension == "pac" {
                let bytes = source.require(&source_name)?;
                let package = Package::parse(&bytes)?;
                if package.len() == 4 {
                    continue;
                } // A texture bank belongs to field overlays.
                if package.len() != 31 || package.reference(0)?.is_none() {
                    pending.insert(source_name);
                    continue;
                }
            } else if extension != "tomdlb_d" {
                pending.insert(source_name);
                continue;
            }
            let path = if let Some(path) = owners.get(&source_name.to_ascii_lowercase()) {
                path.clone()
            } else {
                let owner = format!("meshes/field-models/{name}/model-{resource:08x}");
                let (document, _) = actor_model::cook(source, library, &source_name, &owner)
                    .with_context(|| format!("field {id} actor {resource:#x}: {source_name}"))?;
                let path = format!("{owner}/model.json");
                output::json(&library.join(&path), &document)?;
                owners.insert(source_name.to_ascii_lowercase(), path.clone());
                path
            };
            selections.insert(resource, (path, source_name));
        }
        for (&resource, (path, source_name)) in &selections {
            if !models.mappings.0.contains_key(&resource) {
                let owner = path
                    .strip_prefix("meshes/")
                    .and_then(|p| p.strip_suffix("/model.json"))
                    .context("field actor model has no mesh owner")?;
                models.mappings.0.insert(
                    resource,
                    ModelTarget::Alias(ModelAlias {
                        name: format!("field-models/model-{resource:08x}"),
                        model: owner.to_owned(),
                    }),
                );
                models.sources.insert(resource, source_name.clone());
            }
            if (1..=9).contains(&resource) || resource >> 24 == 0x23 {
                continue;
            }
            actors.models.push(ModelBinding::Local(Actor {
                resource,
                model: path.clone(),
                hidden_geometry: None,
            }));
        }
        for (resource, name) in banks {
            let Some(bytes) = source.read(&name)? else {
                pending.insert(name);
                continue;
            };
            let animation = Animation::read(&bytes)?;
            let mut bindings = Vec::new();
            for (&actor, (path, source_name)) in &selections {
                let document: document::Model =
                    serde_json::from_slice(&fs::read(library.join(path))?)?;
                let bytes = source.require(source_name)?;
                let package = (!source_name.to_ascii_lowercase().ends_with(".tomdlb_d"))
                    .then(|| Package::parse(&bytes))
                    .transpose()?;
                for (part, spec) in document.parts.iter().enumerate() {
                    let descriptor = match &package {
                        Some(package) => package
                            .reference(part)?
                            .or(package.reference(0)?)
                            .context("actor has no body")?,
                        None => source_name,
                    };
                    let key = (name.to_ascii_lowercase(), descriptor.to_ascii_lowercase());
                    let motion = if let Some(path) = motions.get(&key) {
                        path.clone()
                    } else {
                        if !rigs.contains_key(descriptor) {
                            let rig = model::Rig::read(&source.require(descriptor)?)?;
                            let skeleton =
                                Skeleton::from_glb(&fs::read(library.join(&spec.mesh))?)?;
                            rigs.insert(descriptor.to_owned(), (rig, skeleton));
                        }
                        let (native, skeleton) = &rigs[descriptor];
                        if !animation.targets.iter().any(|target| {
                            matches!(target.kind, 3 | 0x10014 | 0x20003)
                                && native
                                    .animation_bindings
                                    .contains(&(target.kind, target.binding))
                        }) {
                            continue;
                        }
                        let path = format!(
                            "{root}/animations/resource-{resource:08x}/actor-{actor:08x}-part-{part}.glb"
                        );
                        output::write(
                            &library.join(&path),
                            &animation.bind(native, skeleton)?.encode(skeleton)?,
                        )?;
                        motions.insert(key, path.clone());
                        path
                    };
                    bindings.push(AnimationBinding {
                        actors: vec![models.mappings.0[&actor].name().to_owned()],
                        parts: vec![part as u16],
                        motion,
                        joints: Default::default(),
                        resource_slot: 12,
                        pose_chains: Vec::new(),
                    });
                }
            }
            if !bindings.is_empty() {
                actors.local_animations.insert(resource, bindings);
            }
        }
        output::json(&library.join(&root).join("actors.json"), &actors)?;
        let base = library.join(&root);
        let definition = document::Definition {
            script: format!("{root}/scripts/scenario.ssb"),
            messages: base
                .join("messages.json")
                .is_file()
                .then(|| format!("{root}/messages.json")),
            scene: format!("{root}/scene.json"),
            collision: base
                .join("collision.glb")
                .is_file()
                .then(|| format!("{root}/collision.glb")),
            actors: format!("{root}/actors.json"),
            audio: Default::default(),
            movies: BTreeSet::new(),
        };
        output::json(&base.join("field.json"), &definition)?;
    }
    models.mappings.by_name()?;
    output::json(&library.join(document::MODELS), &models.mappings)?;
    let mut retained = Vec::new();
    let mut unavailable = Vec::new();
    let mut unconverted = Vec::new();
    for resource in pending {
        if crate::field_extra::retains_source(source, &resource)? {
            retained.push(resource);
        } else if crate::source::validate_name(&resource).is_ok() && !source.contains(&resource)? {
            unavailable.push(resource);
        } else {
            unconverted.push(resource);
        }
    }
    if !retained.is_empty() {
        eprintln!(
            "  {} field references delegated to retained resource conversion",
            retained.len()
        );
    }
    if !unavailable.is_empty() {
        eprintln!(
            "  {} field resource references are absent from source archives",
            unavailable.len()
        );
        for resource in unavailable {
            eprintln!("    {resource}");
        }
    }
    if !unconverted.is_empty() {
        eprintln!(
            "  {} available field resource references are not converted yet",
            unconverted.len()
        );
        for resource in unconverted {
            eprintln!("    {resource}");
        }
    }
    Ok(())
}

pub(crate) fn declarations(bytes: &[u8]) -> Result<(BTreeSet<u32>, bool)> {
    let program = Program::decode(bytes)?;
    let analysis = scenario::analyze(bytes)?;
    let call = NativeCall::ResolveScriptResource;
    let arguments = scenario::calls::analyzed_arguments(&program, &analysis, call, 1)?;
    let finite = if arguments.values().any(|args| args[0].is_none()) {
        scenario::calls::finite_arguments(&program, &analysis, call)
    } else {
        BTreeMap::new()
    };
    let mut ids = BTreeSet::new();
    let mut dynamic = false;
    for (pc, args) in arguments {
        if let Some(id) = args[0] {
            ids.insert(id as u32);
        } else if let Some(possible) = finite.get(&pc) {
            ids.extend(possible.iter().map(|id| *id as u32));
        } else {
            dynamic = true;
        }
    }
    Ok((ids, dynamic))
}

fn ids_table(bytes: &[u8], count: usize) -> Result<Vec<u32>> {
    let size = (count - 1) * 2;
    ensure!(
        bytes
            .get(size..)
            .context("truncated field actor IDs")?
            .iter()
            .all(|b| matches!(b, 0 | 0xfe)),
        "invalid field actor ID padding"
    );
    let mut seen = BTreeSet::new();
    (0..count - 1)
        .map(|i| {
            let id = u32::from(pe::half(bytes, i * 2)?);
            ensure!(
                id > 9 && id != 24 && seen.insert(id),
                "duplicate or reserved field actor ID {id}"
            );
            Ok(id)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actor_ids_keep_native_order_and_reject_bad_tables() -> Result<()> {
        let bytes = [71u16, 60, 0xfefe]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(ids_table(&bytes, 3)?, [71, 60]);
        assert!(ids_table(&bytes[..3], 3).is_err());
        for ids in [[71u16, 71], [71, 24], [1, 71]] {
            let bytes = ids
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            assert!(ids_table(&bytes, 3).is_err());
        }
        Ok(())
    }
}
