//! Editable arena owners. Native layer slots retain their rendering meaning;
//! paths describe resources, never an independently maintained file inventory.
use super::*;
use crate::{
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};

pub const VERSION: u32 = 1;
pub const MAPPINGS: &str = "battle/arenas/mappings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub unsupported_reason: Option<String>,
    pub light_position: [f32; 3],
    pub chain_acceleration: [f32; 3],
    pub actor_color: [u8; 4],
    pub translation: [f32; 3],
    pub yaw_degrees: f32,
    pub color: [u8; 4],
    pub camera_pitch_offset: f32,
    pub layers: BTreeMap<u8, String>,
    pub scenery_bones: BTreeMap<u8, u16>,
    pub scenery_fade: Option<SceneryFade>,
    pub animation_rates: BTreeMap<u8, f32>,
    pub texture_animations: BTreeMap<u8, Vec<TextureAnimation>>,
    pub fog: Option<Fog>,
    pub effects: Option<String>,
    pub recurring_interval: Option<u16>,
    pub textures: Vec<Texture>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub version: u32,
    pub model: PreviewPart,
}

impl Definition {
    pub fn from_stage(stage: Stage, layers: BTreeMap<u8, String>) -> Self {
        Self {
            version: VERSION,
            unsupported_reason: stage.unsupported_reason,
            light_position: stage.light_position,
            chain_acceleration: stage.chain_acceleration,
            actor_color: stage.actor_color,
            translation: stage.translation,
            yaw_degrees: stage.yaw_degrees,
            color: stage.color,
            camera_pitch_offset: stage.camera_pitch_offset,
            layers,
            scenery_bones: stage.scenery_bones,
            scenery_fade: stage.scenery_fade,
            animation_rates: stage.animation_rates,
            texture_animations: stage.texture_animations,
            fog: stage.fog,
            effects: stage.effects,
            recurring_interval: stage.recurring_interval,
            textures: stage.textures,
        }
    }

    fn assemble(self, layers: BTreeMap<u8, PreviewPart>) -> Stage {
        Stage {
            source_sha256: String::new(),

            unsupported_reason: self.unsupported_reason,
            light_position: self.light_position,
            chain_acceleration: self.chain_acceleration,
            actor_color: self.actor_color,
            translation: self.translation,
            yaw_degrees: self.yaw_degrees,
            color: self.color,
            camera_pitch_offset: self.camera_pitch_offset,
            layers,
            scenery_bones: self.scenery_bones,
            scenery_fade: self.scenery_fade,
            animation_rates: self.animation_rates,
            texture_animations: self.texture_animations,
            fog: self.fog,
            effects: self.effects,
            recurring_interval: self.recurring_interval,
            textures: self.textures,
        }
    }
}

pub fn capture(
    snapshot: &mut Snapshot,
    arena: u16,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Stage> {
    ensure!(!cancelled(), "arena preparation cancelled");
    let path = snapshot.resolve(MAPPINGS, u32::from(arena), Some("arena.json"))?;
    let definition: Definition = snapshot.json(&path, JSON_LIMIT)?;
    ensure!(
        definition.version == VERSION,
        "unsupported arena version in {path}"
    );
    let mut layers = BTreeMap::new();
    for (&slot, reference) in &definition.layers {
        ensure!(!cancelled(), "arena preparation cancelled");
        let mut layer: Layer = snapshot
            .json(reference, JSON_LIMIT)
            .with_context(|| format!("{path} layer {slot}: {reference}"))?;
        ensure!(
            layer.version == VERSION,
            "unsupported arena layer version in {reference}"
        );
        crate::battle_model::document::prepare_part(&mut layer.model, |resource| {
            ensure!(!cancelled(), "arena preparation cancelled");
            snapshot.read(resource, ASSET_LIMIT)
        })
        .with_context(|| format!("{reference} layer {slot}"))?;
        for texture in &layer.model.scene.textures {
            ensure!(!cancelled(), "arena preparation cancelled");
            diagnostics.attempt(reference, snapshot.read(texture, ASSET_LIMIT))?;
        }
        layers.insert(slot, layer.model);
    }
    let stage = definition.assemble(layers);
    stage
        .validate_content(|resource| crate::library::validate_path(resource))
        .with_context(|| path.clone())?;
    for texture in &stage.textures {
        for image in &texture.images {
            ensure!(!cancelled(), "arena preparation cancelled");
            diagnostics.attempt(&path, snapshot.read(&image.path, ASSET_LIMIT))?;
        }
    }
    if let Some(effects) = &stage.effects {
        let result =
            crate::battle_effect::document::capture(snapshot, effects, diagnostics, cancelled);
        ensure!(!cancelled(), "arena preparation cancelled");
        diagnostics.attempt(effects, result)?;
    }
    ensure!(!cancelled(), "arena preparation cancelled");
    Ok(stage)
}
