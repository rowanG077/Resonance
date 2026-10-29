//! One editable arena entry with ordered, inline layers. Attachment names refer
//! to joints in the first layer's GLB; file boundaries are ordinary mod boundaries.
use super::*;
use crate::{
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};

pub const MAPPINGS: &str = "battle/arenas/mappings.json";
fn neutral() -> [u8; 4] {
    [64, 64, 64, 255]
}
fn is_neutral(value: &[u8; 4]) -> bool {
    *value == neutral()
}
fn fade_step() -> u8 {
    24
}
fn is_fade_step(value: &u8) -> bool {
    *value == fade_step()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
    pub light_position: [f32; 3],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub chain_acceleration: [f32; 3],
    #[serde(default = "neutral", skip_serializing_if = "is_neutral")]
    pub actor_color: [u8; 4],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub translation: [f32; 3],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub yaw_degrees: f32,
    #[serde(default = "neutral", skip_serializing_if = "is_neutral")]
    pub color: [u8; 4],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub camera_pitch_offset: f32,
    pub layers: Vec<Layer>,
    #[serde(default = "fade_step", skip_serializing_if = "is_fade_step")]
    pub fade_step: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fog: Option<Fog>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurring_interval: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub name: String,
    pub model: PreviewPart,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub foreground: bool,
    /// Joint in the first layer's model. Independent of GLB node order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joint: Option<String>,
    /// Enable camera-facing fading down to this opacity (0–255).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_alpha: Option<u8>,
    /// Clip frames per battle update; animated layers default to 0.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_rate: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texture_animations: Vec<TextureAnimation>,
}

impl Definition {
    pub fn from_stage(mut stage: Stage, names: &BTreeMap<u8, String>) -> Result<Self> {
        // Keep attachment names available while moving the owned model layers.
        let primary_joints = stage
            .layers
            .get(&0)
            .context("arena needs a primary layer")?
            .scene
            .bone_names
            .clone();
        let mut layers = Vec::new();
        // Keep background order, then foreground order. The first model remains
        // the attachment root. Names are labels, not implicit native slots.
        let order = stage
            .layers
            .keys()
            .copied()
            .filter(|slot| !stage.foreground_layers.contains(slot))
            .chain(stage.foreground_layers.iter().copied())
            .collect::<Vec<_>>();
        for slot in order {
            let joint = stage
                .scenery_bones
                .get(&slot)
                .map(|&bone| {
                    primary_joints
                        .get(usize::from(bone))
                        .cloned()
                        .context("invalid arena scenery joint")
                })
                .transpose()?;
            layers.push(Layer {
                name: names
                    .get(&slot)
                    .context("missing arena layer name")?
                    .clone(),
                model: stage.layers.remove(&slot).context("missing arena layer")?,
                foreground: stage.foreground_layers.contains(&slot),
                joint,
                minimum_alpha: stage
                    .scenery_fade
                    .as_ref()
                    .and_then(|fade| fade.minimum_alpha.get(&slot))
                    .copied(),
                animation_rate: stage
                    .animation_rates
                    .get(&slot)
                    .copied()
                    .filter(|rate| *rate != 0.5),
                texture_animations: stage.texture_animations.remove(&slot).unwrap_or_default(),
            });
        }
        Ok(Self {
            unsupported_reason: stage.unsupported_reason,
            light_position: stage.light_position,
            chain_acceleration: stage.chain_acceleration,
            actor_color: stage.actor_color,
            translation: stage.translation,
            yaw_degrees: stage.yaw_degrees,
            color: stage.color,
            camera_pitch_offset: stage.camera_pitch_offset,
            layers,
            fade_step: stage.scenery_fade.map_or_else(fade_step, |fade| fade.step),
            fog: stage.fog,
            effects: stage.effects,
            recurring_interval: stage.recurring_interval,
        })
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
    let mut layers = BTreeMap::new();
    let mut foreground_layers = Vec::new();
    let mut scenery_bones = BTreeMap::new();
    let mut minimum_alpha = BTreeMap::new();
    let mut animation_rates = BTreeMap::new();
    let mut texture_animations = BTreeMap::new();
    let mut names = std::collections::BTreeSet::new();
    for (index, mut layer) in definition.layers.into_iter().enumerate() {
        ensure!(!cancelled(), "arena preparation cancelled");
        let slot = u8::try_from(index).context("too many arena layers")?;
        ensure!(
            slot != 0 || !layer.foreground,
            "the primary arena layer must be a background"
        );
        ensure!(
            !layer.name.trim().is_empty() && names.insert(layer.name.clone()),
            "empty or duplicate arena layer name"
        );
        let context = format!("{path} layer {}", layer.name);
        crate::battle_model::document::prepare_part(&mut layer.model, |resource| {
            ensure!(!cancelled(), "arena preparation cancelled");
            snapshot.read(resource, ASSET_LIMIT)
        })
        .with_context(|| context.clone())?;
        for texture in &layer.model.scene.textures {
            ensure!(!cancelled(), "arena preparation cancelled");
            diagnostics.attempt(&context, snapshot.read(texture, ASSET_LIMIT))?;
        }
        if let Some(joint) = layer.joint {
            let primary: &PreviewPart = layers
                .get(&0)
                .context("first arena layer cannot have an attachment")?;
            let bone = primary
                .scene
                .bone_names
                .iter()
                .position(|name| name == &joint)
                .with_context(|| format!("{context}: missing primary joint {joint}"))?;
            scenery_bones.insert(slot, u16::try_from(bone)?);
        }
        if layer.foreground {
            foreground_layers.push(slot);
        }
        if let Some(alpha) = layer.minimum_alpha {
            minimum_alpha.insert(slot, alpha);
        }
        if layer.model.animation.is_some() {
            animation_rates.insert(slot, layer.animation_rate.unwrap_or(0.5));
        } else {
            ensure!(
                layer.animation_rate.is_none(),
                "{context}: animation rate without a clip"
            );
        }
        if !layer.texture_animations.is_empty() {
            texture_animations.insert(slot, layer.texture_animations);
        }
        layers.insert(slot, layer.model);
    }
    let stage = Stage {
        unsupported_reason: definition.unsupported_reason,
        light_position: definition.light_position,
        chain_acceleration: definition.chain_acceleration,
        actor_color: definition.actor_color,
        translation: definition.translation,
        yaw_degrees: definition.yaw_degrees,
        color: definition.color,
        camera_pitch_offset: definition.camera_pitch_offset,
        layers,
        foreground_layers,
        scenery_bones,
        scenery_fade: (!minimum_alpha.is_empty()).then_some(SceneryFade {
            step: definition.fade_step,
            minimum_alpha,
        }),
        animation_rates,
        texture_animations,
        fog: definition.fog,
        effects: definition.effects,
        recurring_interval: definition.recurring_interval,
    };
    stage
        .validate_content(crate::library::validate_path)
        .with_context(|| path.clone())?;
    if let Some(effects) = &stage.effects {
        let result =
            crate::battle_effect::document::capture(snapshot, effects, diagnostics, cancelled);
        ensure!(!cancelled(), "arena preparation cancelled");
        diagnostics.attempt(effects, result)?;
    }
    ensure!(!cancelled(), "arena preparation cancelled");
    Ok(stage)
}
