//! Remaining resident and event-selected field resources use the shared format.
use crate::{output, package::Package, pe, preview_model, resources::Resources, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::Skeleton,
    field::{
        COLETTE_WINGS_RESOURCE,
        document::{self, Model, ModelBinding, ModelMappings, ModelTarget},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

fn resident_wings(source: &Source, library: &Path, mappings: &mut ModelMappings) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let instruction = image.slice(0x4c3104, 5)?;
    ensure!(
        instruction[0] == 0x68,
        "resident field wing package load changed"
    );
    let bytes = source.require(image.text(pe::word(instruction, 1)?)?)?;
    let package = Package::parse(&bytes)?;
    ensure!(package.len() == 31, "resident field wing directory changed");
    let motions = (2..package.len())
        .filter_map(|member| match package.reference(member) {
            Ok(Some(name)) => Some(Ok(((4 + member * 4) as u16, name.to_owned()))),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        motions.iter().any(|(slot, _)| *slot == 12),
        "resident field wings have no idle motion"
    );
    let owner = "characters/colette/flight-wings";
    let (mut scene, _) = preview_model::part(
        source,
        library,
        package
            .reference(0)?
            .context("resident field wings have no body")?,
        &format!("meshes/{owner}"),
        "body",
        &motions,
    )?;
    scene.autoplay = true;
    let model = Model {
        parts: vec![scene],
        collision: None,
        hidden_geometry: Vec::new(),
    };
    model.validate()?;
    output::json(&library.join(format!("meshes/{owner}/model.json")), &model)?;
    mappings
        .0
        .insert(COLETTE_WINGS_RESOURCE, ModelTarget::Model(owner.into()));
    let mut groups: BTreeMap<String, Vec<ModelBinding>> =
        serde_json::from_slice(&fs::read(library.join(document::MODEL_GROUPS))?)?;
    let party = groups
        .get_mut("party")
        .context("missing field party model group")?;
    if !party
        .iter()
        .any(|binding| matches!(binding, ModelBinding::Shared(name) if name == owner))
    {
        party.push(ModelBinding::Shared(owner.into()));
    }
    output::json(&library.join(document::MODEL_GROUPS), &groups)
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let mut mappings: ModelMappings =
        serde_json::from_slice(&fs::read(library.join(document::MODELS))?)?;
    resident_wings(source, library, &mut mappings)?;
    mappings.by_name()?;
    output::json(&library.join(document::MODELS), &mappings)?;
    legacy::cook(source, library, &mut mappings)?;
    mappings.by_name()?;
    output::json(&library.join(document::MODELS), &mappings)?;
    scenery::cook(source, library)?;
    catalogue::cook(source, library, &mappings)
}

pub(crate) fn retains_source(source: &Source, name: &str) -> Result<bool> {
    legacy::retains_source(source, name)
}

mod catalogue;
mod legacy;
mod party_banks;
mod scenery;
