//! Named stored choreography. Scene models and native texture bank 6 are owned
//! by the effect bank; runtime scene views derive from those same declarations.
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

pub const VERSION: u32 = 1;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub rules: String,
    pub effects: String,
    pub cutin: Option<Texture>,
    pub weapon: Option<models::WeaponDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub version: u32,
    pub projectiles: Vec<SceneProjectile>,
    pub actions: BTreeMap<u8, SceneAction>,
    pub actions_error: Option<String>,
    pub duration: u16,
    pub stage_color: Option<[u8; 3]>,
    pub camera: Option<CameraBounds>,
    pub strike_points: Vec<[f32; 3]>,
    pub recovery_radius: Option<f32>,
    pub spell_recipe: Option<SpellRecipe>,
}

impl From<Scene> for Rules {
    fn from(scene: Scene) -> Self {
        Self {
            version: VERSION,
            projectiles: scene.projectiles,
            actions: scene.actions,
            actions_error: scene.actions_error,
            duration: scene.duration,
            stage_color: scene.stage_color,
            camera: scene.camera,
            strike_points: scene.strike_points,
            recovery_radius: scene.recovery_radius,
            spell_recipe: scene.spell_recipe,
        }
    }
}

pub fn read(
    path: &str,
    diagnostics: &Diagnostics,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<(Scene, SourceBank)> {
    let definition: Definition = json(path, &mut read)?;
    ensure!(
        definition.version == VERSION,
        "unsupported battle scene in {path}"
    );
    let rules: Rules = json(&definition.rules, &mut read)?;
    ensure!(
        rules.version == VERSION,
        "unsupported scene rules in {}",
        definition.rules
    );
    let bank = effects::read(&definition.effects, diagnostics, &mut read)?;
    let (models, textures) = bank
        .art
        .as_ref()
        .map(|art| {
            (
                art.models.clone(),
                art.textures
                    .get(&6)
                    .into_iter()
                    .flatten()
                    .map(|texture| Texture {
                        images: texture.images.clone(),
                        sampler: texture.sampler.clone(),
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();
    let weapon = definition
        .weapon
        .map(|weapon| models::read_weapon(weapon, &mut read))
        .transpose()?;
    let scene = Scene {
        version: super::VERSION,
        source_sha256: String::new(),

        effects: definition.effects,
        textures,
        models,
        cutin: definition.cutin,
        weapon,
        projectiles: rules.projectiles,
        actions: rules.actions,
        actions_error: rules.actions_error,
        duration: rules.duration,
        stage_color: rules.stage_color,
        camera: rules.camera,
        strike_points: rules.strike_points,
        recovery_radius: rules.recovery_radius,
        spell_recipe: rules.spell_recipe,
    };
    scene.validate().with_context(|| path.to_owned())?;
    if let Some(cutin) = &scene.cutin {
        cutin.sampler.validate()?;
        ensure!(!cutin.images.is_empty(), "empty scene cut-in in {path}");
        for image in &cutin.images {
            image.validate()?;
        }
    }
    Ok((scene, bank))
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
    for texture in scene
        .models
        .values()
        .flat_map(|model| &model.layers)
        .chain(
            scene
                .weapon
                .iter()
                .flat_map(|weapon| weapon.parts.values())
                .flat_map(|model| &model.layers),
        )
        .flat_map(|layer| &layer.scene.textures)
        .chain(
            scene
                .cutin
                .iter()
                .flat_map(|texture| &texture.images)
                .map(|image| &image.path),
        )
        .chain(
            bank.art
                .iter()
                .flat_map(|art| art.textures.values())
                .flatten()
                .flat_map(|texture| &texture.images)
                .map(|image| &image.path),
        )
    {
        ensure!(!cancelled(), "battle scene preparation cancelled");
        diagnostics.attempt(&path, snapshot.read(texture, ASSET_LIMIT))?;
    }
    ensure!(!cancelled(), "battle scene preparation cancelled");
    Ok((scene, bank))
}

/// Runtime consumers use the mapped scene from their already captured view.
pub fn load(files: &Files, kind: Kind, id: u16) -> Result<Scene> {
    let mappings: Mappings = files.json(kind.mapping())?;
    let path = mappings.resolve(kind.mapping(), u32::from(id), Some("scene.json"))?;
    Ok(read(&path, files.diagnostics(), |path| files.read(path))?.0)
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
