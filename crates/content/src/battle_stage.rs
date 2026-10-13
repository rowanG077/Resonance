//! Cooked battle scenery and its presentation dependencies.
use crate::{
    battle_scene::Texture,
    field_preload::{File, Role},
    model_preview::PreviewPart,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub fn path(arena: u16) -> String {
    format!("battle/stages/{arena:03}.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stage {
    pub source_sha256: String,
    /// The structural assets are cooked even when their presentation is not implemented.
    /// Layer rejections retain the layer index and authored flags in this reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
    pub light_position: [f32; 3],
    /// Acceleration applied to secondary-motion segments in world space.
    pub chain_acceleration: [f32; 3],
    /// Native RGB uses 64 as neutral; alpha uses 255.
    pub actor_color: [u8; 4],
    /// Rotate this source translation around Y by `yaw_degrees` at placement.
    pub translation: [f32; 3],
    /// Fixed models use this Y rotation after converting Z-up assets into Y-up space.
    pub yaw_degrees: f32,
    pub color: [u8; 4],
    pub camera_pitch_offset: f32,
    /// Background layers are 0 and 4 onward. Foreground layers draw as 1, 3, 2.
    pub layers: BTreeMap<u8, PreviewPart>,
    /// Scenery layers attached to indexed bones of the primary model.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scenery_bones: BTreeMap<u8, u16>,
    /// Camera-facing scenery fades toward these authored opacity limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenery_fade: Option<SceneryFade>,
    /// Authored animation frames advanced per battle update, keyed by layer.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub animation_rates: BTreeMap<u8, f32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub texture_animations: BTreeMap<u8, Vec<TextureAnimation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fog: Option<Fog>,
    pub effects: Option<String>,
    /// Battle updates between emissions of arena effect program 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurring_interval: Option<u16>,
    pub textures: Vec<Texture>,
    pub files: BTreeMap<String, File>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneryFade {
    pub step: u8,
    pub minimum_alpha: BTreeMap<u8, u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fog {
    pub color: [u8; 4],
    /// Depth beyond the camera's horizontal distance from its target.
    pub start: f32,
    pub end: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureAnimation {
    /// Per-model image selector. Authored channels may have no material consumer.
    pub texture: usize,
    pub interval: u8,
    pub motion: TextureMotion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextureMotion {
    Scroll { velocity: [f32; 2] },
    Atlas { frames: u8 },
}

impl TextureAnimation {
    pub fn offset(&self, tick: u64) -> [f32; 2] {
        let step = tick / u64::from(self.interval);
        match self.motion {
            TextureMotion::Scroll { velocity } => velocity.map(|v| v * step as f32),
            TextureMotion::Atlas { frames } => {
                [0., (step % u64::from(frames)) as f32 / f32::from(frames)]
            }
        }
    }
}

impl Stage {
    /// Validate the resource structure independently of runtime presentation support.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.source_sha256.len() == 64
                && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && !self.layers.is_empty(),
            "invalid battle stage"
        );
        ensure!(
            self.light_position
                .iter()
                .chain(&self.chain_acceleration)
                .chain(&self.translation)
                .chain([&self.yaw_degrees, &self.camera_pitch_offset])
                .all(|v| v.is_finite()),
            "invalid battle stage transform"
        );
        ensure!(
            self.scenery_bones
                .keys()
                .eq(self.layers.keys().filter(|&&slot| slot >= 4))
                && self.scenery_bones.values().all(|&bone| {
                    self.layers
                        .get(&0)
                        .is_some_and(|layer| usize::from(bone) < layer.scene.bone_names.len())
                }),
            "missing or invalid stage scenery attachment"
        );
        if let Some(fade) = &self.scenery_fade {
            ensure!(
                fade.step > 0 && fade.minimum_alpha.keys().eq(self.scenery_bones.keys()),
                "invalid stage scenery fading"
            );
        }
        ensure!(
            self.animation_rates.keys().eq(self
                .layers
                .iter()
                .filter(|(_, layer)| layer.animation.is_some())
                .map(|(slot, _)| slot))
                && self
                    .animation_rates
                    .values()
                    .all(|rate| rate.is_finite() && *rate >= 0.),
            "missing or invalid stage animation rate"
        );
        if let Some(fog) = &self.fog {
            ensure!(
                fog.start.is_finite() && fog.end.is_finite() && fog.end > fog.start,
                "invalid stage fog range"
            );
        }
        for (slot, animations) in &self.texture_animations {
            self.layers
                .get(slot)
                .context("missing texture animation layer")?;
            let mut textures = std::collections::BTreeSet::new();
            for animation in animations {
                ensure!(
                    textures.insert(animation.texture) && animation.interval > 0,
                    "invalid stage texture animation target or interval"
                );
                ensure!(
                    match animation.motion {
                        TextureMotion::Scroll { velocity } =>
                            velocity.iter().all(|v| v.is_finite()),
                        TextureMotion::Atlas { frames } => frames > 0,
                    },
                    "invalid stage texture motion"
                );
            }
        }
        for (path, file) in &self.files {
            crate::validate_asset_path(path)?;
            ensure!(
                file.sha256.len() == 64
                    && file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    && !file.roles.is_empty()
                    && !file.roles.contains(&Role::Movie),
                "invalid stage dependency {path}"
            );
        }
        let require = |path: &str, role| -> Result<()> {
            let file = self
                .files
                .get(path)
                .with_context(|| format!("undeclared stage dependency {path}"))?;
            ensure!(
                file.roles.contains(&role),
                "invalid stage dependency role {path}"
            );
            Ok(())
        };
        for (&slot, layer) in &self.layers {
            let scene = &layer.scene;
            ensure!(
                scene.resource == u16::from(slot)
                    && scene.autoplay == layer.animation.is_some()
                    && scene.texture_animations.is_empty()
                    && layer.attached_to.is_none()
                    && layer.uv_offsets.is_empty()
                    && scene.secondary_motion.is_empty(),
                "unsupported animated battle stage layer {slot}"
            );
            ensure!(
                scene.translation.iter().all(|v| v.is_finite())
                    && scene.materials.iter().all(|m| m
                        .color
                        .iter()
                        .chain(&m.multiply)
                        .all(|b| b.texture < scene.textures.len())),
                "invalid battle stage layer {slot}"
            );
            layer.selected_clip()?;
            ensure!(
                scene.clips.len() == usize::from(layer.animation.is_some()),
                "unexpected stage animation clips"
            );
            for clip in &scene.clips {
                require(&clip.motion, Role::Data)?;
            }
            require(&scene.mesh, Role::Mesh)?;
            for texture in &scene.textures {
                require(texture, Role::Texture)?;
            }
        }
        if let Some(effects) = &self.effects {
            require(effects, Role::Data)?;
        }
        ensure!(
            self.recurring_interval
                .is_none_or(|interval| interval > 0 && self.effects.is_some()),
            "invalid recurring stage effect"
        );
        for texture in &self.textures {
            texture.sampler.validate()?;
            ensure!(!texture.images.is_empty(), "empty stage texture");
            for image in &texture.images {
                image.validate()?;
                require(&image.path, Role::Texture)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_texture_clocks_match_recorded_steps_and_atlas_wrap() {
        let scroll = TextureAnimation {
            texture: 0,
            interval: 2,
            motion: TextureMotion::Scroll {
                velocity: [0.55, -0.6],
            },
        };
        for (tick, expected) in [
            (0, [0., 0.]),
            (1, [0., 0.]),
            (2, [0.55, -0.6]),
            (3, [0.55, -0.6]),
            (4, [1.1, -1.2]),
        ] {
            assert_eq!(scroll.offset(tick), expected);
        }
        let atlas = TextureAnimation {
            texture: 1,
            interval: 20,
            motion: TextureMotion::Atlas { frames: 16 },
        };
        for (tick, vertical) in [
            (0, 0.),
            (19, 0.),
            (20, 0.0625),
            (80, 0.25),
            (300, 0.9375),
            (320, 0.),
        ] {
            assert_eq!(atlas.offset(tick), [0., vertical]);
        }
    }
}
