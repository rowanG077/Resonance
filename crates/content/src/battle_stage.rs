//! Fixed battle scenery and its verified presentation dependencies.
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
    pub light_position: [f32; 3],
    /// Added to secondary-motion segment velocity by the original chain update.
    pub chain_acceleration: [f32; 3],
    /// Native RGB uses 64 as neutral; alpha uses 255.
    pub actor_color: [u8; 4],
    /// Rotate this source translation around Y by `yaw_degrees` at placement.
    pub translation: [f32; 3],
    /// Fixed models use this Y rotation and the original -90 degree X basis.
    pub yaw_degrees: f32,
    pub color: [u8; 4],
    pub camera_pitch_offset: f32,
    /// Original fixed slots. Drawing orders slot 0 before actors, then 1, 3, 2.
    pub layers: BTreeMap<u8, PreviewPart>,
    pub effects: Option<String>,
    pub textures: Vec<Texture>,
    pub files: BTreeMap<String, File>,
}

impl Stage {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.source_sha256.len() == 64
                && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && !self.layers.is_empty()
                && self.layers.keys().all(|&slot| slot < 4),
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
                    && scene.clips.is_empty()
                    && !scene.autoplay
                    && scene.texture_animations.is_empty()
                    && layer.animation.is_none()
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
            require(&scene.mesh, Role::Mesh)?;
            for texture in &scene.textures {
                require(texture, Role::Texture)?;
            }
        }
        if let Some(effects) = &self.effects {
            require(effects, Role::Data)?;
        }
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

    pub fn model_colors(&self) -> [Option<[u8; 4]>; 4] {
        std::array::from_fn(|slot| {
            self.layers
                .contains_key(&(slot as u8))
                .then_some(self.color)
        })
    }
}
