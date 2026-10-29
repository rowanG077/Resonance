//! Named stored choreography. Scene models and native texture bank 6 are owned
//! by the effect bank, which supplies them directly to effect preparation.
use super::*;
use crate::{
    battle_effect::{SourceBank, document as effects},
    battle_model::document as models,
    diagnostics::Diagnostics,
    library::{Mappings, Snapshot},
    preparation::{ASSET_LIMIT, JSON_LIMIT},
    prepared::Files,
};
use std::sync::Arc;

pub const TECHNIQUES: &str = "battle/scenes/mappings.json";
pub const SKILLS: &str = "battle/skills/mappings.json";

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Technique,
    Skill,
}

impl Kind {
    pub fn mapping(self) -> &'static str {
        match self {
            Self::Technique => TECHNIQUES,
            Self::Skill => SKILLS,
        }
    }
}

pub type Definition = Scene<models::WeaponDefinition>;

pub fn read(
    path: &str,
    diagnostics: &Diagnostics,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<(Scene, SourceBank)> {
    let definition: Definition = json(path, &mut read)?;
    let bank = effects::read(&definition.effects, diagnostics, &mut read)?;
    Ok((prepare(definition, path, diagnostics, &mut read)?, bank))
}

fn prepare(
    mut definition: Definition,
    path: &str,
    diagnostics: &Diagnostics,
    read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<Scene> {
    let weapon = definition
        .weapon
        .take()
        .map(|weapon| models::read_weapon(weapon, diagnostics, &mut *read))
        .transpose()?;
    let mut scene = definition.with_weapon(weapon);
    scene.validate().with_context(|| path.to_owned())?;
    if let Some(cutin) = &mut scene.cutin {
        cutin.sampler.validate()?;
        ensure!(!cutin.images.is_empty(), "empty scene cut-in in {path}");
        for image in &mut cutin.images {
            diagnostics.attempt(path, crate::texture::images::prepare(image, &mut *read))?;
        }
    }
    Ok(scene)
}

pub fn capture(
    snapshot: &mut Snapshot,
    kind: Kind,
    id: u16,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<(Scene, SourceBank)> {
    ensure!(!cancelled(), "battle scene preparation cancelled");
    let path = snapshot.resolve(kind.mapping(), u32::from(id), Some("scene.json"))?;
    let result = read(&path, diagnostics, |path| {
        ensure!(!cancelled(), "battle scene preparation cancelled");
        snapshot.read(path, ASSET_LIMIT)
    });
    ensure!(!cancelled(), "battle scene preparation cancelled");
    let (scene, bank) = result?;
    for texture in bank
        .art
        .iter()
        .flat_map(|art| art.models.values())
        .flat_map(|model| &model.layers)
        .chain(
            scene
                .weapon
                .iter()
                .flat_map(|weapon| weapon.parts.values())
                .flat_map(|model| &model.layers),
        )
        .flat_map(|layer| &layer.scene.textures)
    {
        ensure!(!cancelled(), "battle scene preparation cancelled");
        diagnostics.attempt(&path, snapshot.read(texture, ASSET_LIMIT))?;
    }
    ensure!(!cancelled(), "battle scene preparation cancelled");
    Ok((scene, bank))
}

/// Runtime consumers read scene rules and cut-ins from the captured view. Effect
/// preparation owns the bank and its artwork; do not decode it again here.
pub fn load(files: &Files, kind: Kind, id: u16) -> Result<Scene> {
    let mappings: Mappings = files.json(kind.mapping())?;
    let path = mappings.resolve(kind.mapping(), u32::from(id), Some("scene.json"))?;
    let mut read = |path: &str| files.read(path);
    prepare(
        json(&path, &mut read)?,
        &path,
        files.diagnostics(),
        &mut read,
    )
}

fn json<T: serde::de::DeserializeOwned>(
    path: &str,
    read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<T> {
    crate::library::validate_path(path)?;
    let bytes = read(path)?;
    ensure!(
        bytes.len() <= JSON_LIMIT,
        "battle scene document exceeds size limit: {path}"
    );
    serde_json::from_slice(&bytes).with_context(|| format!("decode scene document {path}"))
}
