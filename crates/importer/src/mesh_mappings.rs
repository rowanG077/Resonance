//! One global lookup from native field model IDs to editable mesh owners.
//!
//! These maintained names describe the native model catalogues. They are
//! importer input, not a copy of the cooked mapping document.

use crate::{character_attachments, character_models, field_wings};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    appearance::costume_resource,
    field::document::{ModelAlias, ModelBinding, ModelMappings, ModelTarget},
};
use std::{collections::BTreeMap, path::Path};

fn merge(
    mappings: &mut BTreeMap<u32, ModelTarget>,
    entries: BTreeMap<u32, ModelTarget>,
) -> Result<()> {
    for (id, target) in entries {
        if let Some(previous) = mappings.get(&id) {
            ensure!(
                previous.name() == target.name() && previous.model() == target.model(),
                "conflicting model mapping {id}: {} -> {} versus {} -> {}",
                previous.name(),
                previous.model(),
                target.name(),
                target.model()
            );
        } else {
            mappings.insert(id, target);
        }
    }
    Ok(())
}

/// Combine the maintained names with party costumes and their physical owners.
pub fn mesh_mappings() -> Result<ModelMappings> {
    let names: BTreeMap<u32, String> = serde_json::from_str(include_str!("mesh-model-names.json"))?;
    let aliases: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("mesh-model-aliases.json"))?;
    let mut mappings = names
        .into_iter()
        .map(|(id, name)| (id, ModelTarget::Model(name)))
        .collect();
    merge(&mut mappings, character_models::party_model_mappings()?)?;
    for (id, name) in aliases {
        let previous = mappings
            .get_mut(&id)
            .with_context(|| format!("model alias {id} has no physical owner"))?;
        let owner = previous.model().to_owned();
        ensure!(
            previous.name() == owner || previous.name() == name,
            "model alias {id} disagrees with the costume owner"
        );
        *previous = ModelTarget::Alias(ModelAlias { name, model: owner });
    }
    merge(&mut mappings, character_attachments::attachment_mappings())?;
    merge(&mut mappings, field_wings::field_wing_mappings())?;
    let mappings = ModelMappings(mappings);
    mappings.by_name()?;
    Ok(mappings)
}

fn party_group(mappings: &ModelMappings) -> Result<BTreeMap<String, Vec<ModelBinding>>> {
    let mut party = Vec::new();
    for id in 1..=9 {
        for costume in 0..=4 {
            let resource = costume_resource(id, costume);
            if let Some(model) = mappings.0.get(&resource) {
                party.push(ModelBinding::Shared(model.name().to_owned()));
            }
        }
    }
    ensure!(
        party.len() == character_models::party_model_mappings()?.len(),
        "party group does not contain every costume"
    );
    Ok(BTreeMap::from([("party".into(), party)]))
}

fn write(library: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    crate::output::write(&library.join("meshes").join(name), bytes)
}

/// Publish the complete shared model lookup, party roster, and joint labels.
pub fn cook_mesh_mappings(extracted: &Path, library: &Path) -> Result<()> {
    let mut mappings = mesh_mappings()?;
    merge(
        &mut mappings.0,
        crate::field_npc_models::npc_mappings(extracted)?,
    )?;
    merge(
        &mut mappings.0,
        crate::field_resource_models::mappings(extracted)?,
    )?;
    mappings.by_name()?;
    let groups = party_group(&mappings)?;
    let model_bytes = resonance_asset_writer::json::encode(&mappings)?;
    let group_bytes = resonance_asset_writer::json::encode(&groups)?;
    let joint_bytes = include_bytes!("model-joints.json");
    write(library, "mappings.json", &model_bytes)?;
    write(library, "groups.json", &group_bytes)?;
    write(library, "joint-mappings.json", joint_bytes)
}
