//! Original model variants without a global field-resource ID retain named owners.
use crate::{battle_party_model, compression, field_npc_models};
use anyhow::{Context, Result};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::PartyCharacterId;

pub fn cook_original_models(extracted: &Path, library: &Path) -> Result<()> {
    let mut shared = BTreeMap::new();
    let mut party = BTreeMap::new();
    for character in PartyCharacterId::ALL {
        let name = character
            .resource_name()
            .context("unnamed party character")?;
        let package = battle_party_model::motion_bank(extracted, character.raw(), 0)?;
        // The native low-detail files contain exactly these two CAB geometry
        // sections. Select this source once, including its correct battle motions.
        let owner = format!("originals/party/{name}");
        if !library.join(format!("meshes/{owner}/model.json")).is_file() {
            field_npc_models::convert_package(library, &owner, &package, None, &mut shared, true)
                .with_context(|| format!("original {name} battle model"))?;
        }
        party.insert(character.raw(), name.to_owned());
    }
    crate::output::json(
        &library.join("meshes/originals/party/mappings.json"),
        &party,
    )?;
    for (source, name, battle) in [
        ("BTL/BTLrutee.cab", "characters/rutee", true),
        ("mv_terr.bin", "unidentified/mv-terr", false),
        ("ring_trans.bin", "unidentified/ring-trans", false),
        ("sym_angel.bin", "symbols/angel", false),
        ("sym_aqua.bin", "symbols/aqua", false),
        ("sym_dragon.bin", "symbols/dragon", false),
        ("sym_human.bin", "symbols/human", false),
        ("sym_mech.bin", "symbols/mech", false),
        ("sym_myst.bin", "symbols/myst", false),
        ("sym_plant.bin", "symbols/plant", false),
    ] {
        let owner = format!("originals/{name}");
        if !library.join(format!("meshes/{owner}/model.json")).is_file() {
            let bytes = fs::read(extracted.join("disc1/files").join(source))?;
            let package = if bytes.starts_with(b"MSCF") {
                compression::cabinet(&bytes)?
            } else {
                bytes
            };
            field_npc_models::convert_package(library, &owner, &package, None, &mut shared, battle)
                .with_context(|| format!("original model {name}"))?;
        }
    }
    Ok(())
}
