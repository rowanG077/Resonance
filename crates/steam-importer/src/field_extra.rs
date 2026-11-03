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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;
    use std::path::PathBuf;

    fn inputs() -> Result<(Source, PathBuf)> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let library = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        Ok((source, library))
    }

    #[test]
    #[ignore = "requires installed Steam and cooked fields; cooks extra field owners and checks affected field loading"]
    fn installed_field_extra_cooks_and_prepares_affected_fields() -> Result<()> {
        let (source, root) = inputs()?;
        cook(&source, &root)?;
        let models: ModelMappings =
            serde_json::from_slice(&fs::read(root.join(document::MODELS))?)?;
        ensure!(
            models.0.contains_key(&COLETTE_WINGS_RESOURCE),
            "missing resident flight wings"
        );
        ensure!(
            models.0.contains_key(&361),
            "missing retained Thoda Geyser actor"
        );
        let fields: resonance_content::library::Mappings =
            serde_json::from_slice(&fs::read(root.join(resonance_content::library::FIELDS))?)?;
        for id in [8, 525] {
            let path = format!("fields/{}/field.json", fields.target(id)?);
            let field: document::Definition = serde_json::from_slice(&fs::read(root.join(path))?)?;
            let assets = field
                .assemble(id, |path| Ok(fs::read(root.join(path))?.into()))
                .with_context(|| format!("prepare extended field {id}"))?;
            ensure!(
                assets
                    .actors
                    .iter()
                    .any(|actor| actor.resource == COLETTE_WINGS_RESOURCE),
                "field has no resident wing resource"
            );
            if id == 8 {
                ensure!(
                    assets.actors.iter().any(|actor| actor.resource == 361),
                    "field has no retained actor"
                );
            }
            if id == 525 {
                ensure!(
                    assets
                        .actors
                        .iter()
                        .flat_map(|actor| &actor.parts)
                        .flat_map(|part| &part.clips)
                        .any(|clip| clip.animation_resource == Some(178)),
                    "field has no retained motion bank"
                );
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked field-extra owners; checks a script-computed bank through the shared dynamic resource boundary"]
    fn cooked_field_animation_catalogue_supports_dynamic_requests() -> Result<()> {
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let bytes = fs::read(root.join("animations/bindings.json"))?;
        ensure!(
            bytes.len() <= resonance_content::preparation::JSON_LIMIT,
            "shared field bindings exceed the runtime JSON limit"
        );
        let _: document::SharedBankBindings = serde_json::from_slice(&bytes)?;
        let fields: resonance_content::library::Mappings =
            serde_json::from_slice(&fs::read(root.join(resonance_content::library::FIELDS))?)?;
        let path = format!("fields/{}/field.json", fields.target(8)?);
        let field: document::Definition = serde_json::from_slice(&fs::read(root.join(path))?)?;
        let mut assets = field.assemble(8, |path| Ok(fs::read(root.join(path))?.into()))?;
        let library = resonance_content::library::Library::open(&root, &[])?;
        // Cover an existing local event, a dedicated party bank and a native
        // null-slot alias against the published library without an overlay.
        for resource in [196630, 0x30001, 0x30002] {
            let actors = document::Actors::capture_resource(
                &mut library.snapshot(),
                &mut assets,
                resource,
                &|| false,
            )?;
            ensure!(
                actors
                    .iter()
                    .filter(|actor| actor.resource == 1)
                    .flat_map(|actor| &actor.parts)
                    .flat_map(|part| &part.clips)
                    .any(|clip| clip.animation_resource == Some(resource)),
                "dynamic event bank {resource:#x} did not bind to Lloyd's loaded body"
            );
        }
        Ok(())
    }
}
