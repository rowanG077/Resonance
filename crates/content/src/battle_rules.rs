//! Editable shared battle rules. Source provenance stays with the importer;
//! gameplay IDs are translated to named owners during preparation.
use crate::{
    battle_defense_system as defense, battle_formation as formation, battle_profile as profile,
    battle_projectile as projectile, battle_recoil as recoil, battle_unison as unison_runtime,
    library::Snapshot, preparation::JSON_LIMIT, prepared::Files,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, sync::Arc};

pub const PARTY: &str = "battle/party/rules.json";
pub const NORMAL_ATTACKS: &str = "battle/party/normal-attacks.json";
pub const MARTIAL_ATTACKS: &str = "battle/party/techniques.json";

type Reader<'a> = dyn FnMut(&str) -> Result<Arc<[u8]>> + 'a;

fn json<T: DeserializeOwned>(read: &mut Reader<'_>, path: &str) -> Result<T> {
    crate::library::validate_path(path)?;
    let bytes = read(path)?;
    ensure!(
        bytes.len() <= JSON_LIMIT,
        "battle rules exceed size limit: {path}"
    );
    serde_json::from_slice(&bytes).with_context(|| format!("battle rules {path}"))
}

mod formations;
pub use formations::{
    Actor as FormationActor, Formation, Formations, Pool as FormationPool,
    Reserve as FormationReserve,
};

fn read_formations(read: &mut Reader<'_>) -> Result<formation::Formations> {
    let value: Formations = json(read, formation::PATH)?;
    let enemies = json(read, crate::battle_enemy::document::MAPPINGS)?;
    value.prepare(&enemies)
}

mod party;
pub use party::Party;

fn read_party(read: &mut Reader<'_>) -> Result<profile::Table> {
    let value: Party = json(read, PARTY)?;
    let characters = json(read, crate::battle_model::document::PARTY)?;
    let normals =
        json::<crate::battle_normal::document::NormalAttacks>(read, &value.normal_attacks)?
            .prepare()?;
    let martial = json::<crate::battle_martial::document::Catalogue>(read, &value.martial_attacks)?
        .prepare()?;
    value.prepare(&characters, normals, martial)
}

#[path = "battle_rules/defense.rs"]
mod defense_document;
pub use defense_document::DefenseSystem;
fn read_defense(read: &mut Reader<'_>) -> Result<defense::Definition> {
    let value: DefenseSystem = json(read, defense::PATH)?;
    value.prepare(&json(read, crate::battle_enemy::document::MAPPINGS)?)
}

mod unison;
pub use unison::Unison;

fn read_unison(
    read: &mut Reader<'_>,
    techniques: &crate::library::Mappings,
) -> Result<unison_runtime::Catalogue> {
    let value: Unison = json(read, unison_runtime::PATH)?;
    let characters = json(read, crate::battle_model::document::PARTY)?;
    let scenes = json(read, crate::battle_scene::document::TECHNIQUES)?;
    value.prepare(techniques, &characters, &scenes)
}

fn read_projectiles(read: &mut Reader<'_>) -> Result<projectile::Table> {
    let table: projectile::Table = json(read, projectile::PATH)?;
    ensure!(
        table.version == projectile::VERSION,
        "unsupported projectile table version"
    );
    // Full projectile admission needs the selected actor/rig and remains in
    // encounter preparation. Preserve unused unsupported templates verbatim.
    Ok(table)
}

#[path = "battle_rules/recoil.rs"]
mod recoil_document;
pub use recoil_document::{Impulse as RecoilImpulse, Recoil};
fn read_recoil(read: &mut Reader<'_>) -> Result<recoil::Table> {
    let value: Recoil = json(read, recoil::PATH)?;
    value.prepare(&json(read, crate::battle_model::document::PARTY)?)
}

// Import retains authored techniques for publication; runtime keeps their prepared catalogue.
pub struct Captured<Techniques = crate::arte::document::Document> {
    pub formations: formation::Formations,
    pub party: profile::Table,
    pub defense: defense::Definition,
    pub unison: unison_runtime::Catalogue,
    pub projectiles: projectile::Table,
    pub recoil: recoil::Table,
    pub techniques: Techniques,
}

pub fn capture(
    snapshot: &mut Snapshot,
    cancelled: &impl Fn() -> bool,
) -> Result<Captured<crate::arte::Catalogue>> {
    let mut read = |path: &str| {
        ensure!(!cancelled(), "battle rules preparation cancelled");
        snapshot.read(path, JSON_LIMIT)
    };
    let techniques: crate::arte::document::Document = json(&mut read, crate::arte::PATH)?;
    let result = Captured {
        formations: read_formations(&mut read)?,
        party: read_party(&mut read)?,
        defense: read_defense(&mut read)?,
        unison: read_unison(&mut read, &techniques.mappings)?,
        projectiles: read_projectiles(&mut read)?,
        recoil: read_recoil(&mut read)?,
        techniques: techniques.prepare()?.catalogue,
    };
    ensure!(!cancelled(), "battle rules preparation cancelled");
    Ok(result)
}

pub fn formations(files: &Files) -> Result<formation::Formations> {
    read_formations(&mut |path| files.read(path))
}

/// Pool selection and encounter preparation must share the same captured table.
pub fn capture_formations(snapshot: &mut Snapshot) -> Result<formation::Formations> {
    read_formations(&mut |path| snapshot.read(path, JSON_LIMIT))
}
pub fn party(files: &Files) -> Result<profile::Table> {
    read_party(&mut |path| files.read(path))
}
pub fn defense(files: &Files) -> Result<defense::Definition> {
    read_defense(&mut |path| files.read(path))
}
pub fn unison(files: &Files) -> Result<unison_runtime::Catalogue> {
    let mut read = |path: &str| files.read(path);
    let techniques: crate::arte::document::Document = json(&mut read, crate::arte::PATH)?;
    read_unison(&mut read, &techniques.mappings)
}
pub fn projectiles(files: &Files) -> Result<projectile::Table> {
    read_projectiles(&mut |path| files.read(path))
}
pub fn recoil(files: &Files) -> Result<recoil::Table> {
    read_recoil(&mut |path| files.read(path))
}

#[cfg(test)]
mod declarations_tests;
