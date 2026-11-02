//! Native save circles and treasure chests use ordinary shared actor models.
use crate::{actor_model, output, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::{
        SAVE_POINT_RESOURCE, TREASURE_RESOURCE_BASE,
        document::{self, Actor, Actors, ModelBinding, ModelMappings, ModelTarget},
    },
    library::{FIELDS, Mappings},
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_script::{NativeCall, scenario};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let names: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("../../importer/src/mesh-model-names.json"))?;
    let mut mappings: ModelMappings =
        serde_json::from_slice(&fs::read(library.join(document::MODELS))?)?;
    let resources = [
        SAVE_POINT_RESOURCE,
        TREASURE_RESOURCE_BASE,
        TREASURE_RESOURCE_BASE + 1,
        TREASURE_RESOURCE_BASE + 2,
    ];
    for (index, id) in resources.into_iter().enumerate() {
        // Field initialization loads these four packages, independent of the
        // scenario's explicit ResolveScriptResource declarations.
        let call = image.slice(0x4c3649 + index as u32 * 18, 5)?;
        ensure!(call[0] == 0x68, "unknown service package selection");
        let source_name = image.text(pe::word(call, 1)?)?;
        let name = names.get(&id).context("unnamed field service model")?;
        eprintln!("  Field service: {name} ({source_name})");
        let owner = format!("meshes/{name}");
        let (model, _) = actor_model::cook(source, library, source_name, &owner)?;
        output::json(&library.join(&owner).join("model.json"), &model)?;
        mappings.0.insert(id, ModelTarget::Model(name.clone()));
    }
    mappings.by_name()?;
    output::json(&library.join(document::MODELS), &mappings)?;
    let fields: Mappings = serde_json::from_slice(&fs::read(library.join(FIELDS))?)?;
    for (_, name) in fields.iter() {
        let root = library.join("fields").join(name);
        let bytes = fs::read(root.join("scripts/scenario.ssb"))?;
        let analysis = scenario::analyze(&bytes)?;
        let has = |call: NativeCall| {
            analysis.instructions.values().any(|instruction| {
                instruction.mnemonic == "proc" && instruction.operands == [i64::from(call as u8)]
            })
        };
        let save = has(NativeCall::CreateSavePoint) || has(NativeCall::CreateSealedSavePoint);
        let treasure = has(NativeCall::CreateTreasureChest);
        if !save && !treasure {
            continue;
        }
        let path = root.join("actors.json");
        let mut actors: Actors = serde_json::from_slice(&fs::read(&path)?)?;
        actors.models.retain(|binding| {
            !matches!(binding, ModelBinding::Local(actor) if resources.contains(&actor.resource))
        });
        for id in resources {
            if if id == SAVE_POINT_RESOURCE {
                save
            } else {
                treasure
            } {
                actors.models.push(ModelBinding::Local(Actor {
                    resource: id,
                    model: mappings.model_path(id)?,
                    hidden_geometry: None,
                }));
            }
        }
        output::json(&path, &actors)?;
    }
    Ok(())
}
