//! High-level field data. Coordinates retain the authored Z-up world space.
/// Reserved resource range for static scenery, separate from character models.
pub const SCENERY_RESOURCE_BASE: u32 = 0x1000_0000;
use crate::{ScenePart, ScriptAsset, validate_asset_path};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionGroup {
    /// Surface classification and collision query mask used by field services.
    pub surface: u32,
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u16; 3]>,
}

impl CollisionGroup {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.vertices.is_empty() && self.vertices.len() <= 65536,
            "invalid collision vertex count"
        );
        ensure!(
            !self.triangles.is_empty() && self.triangles.len() <= 65536,
            "invalid collision triangle count"
        );
        ensure!(
            self.vertices.iter().flatten().all(|v| v.is_finite()),
            "nonfinite collision vertex"
        );
        ensure!(
            self.triangles
                .iter()
                .flatten()
                .all(|v| usize::from(*v) < self.vertices.len()),
            "collision index exceeds vertices"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldAssets {
    pub version: u32,
    pub map_id: u32,
    pub source_sha256: String,
    pub script: ScriptAsset,
    pub messages: String,
    pub parts: Vec<ScenePart>,
    pub ground: Vec<CollisionGroup>,
    pub regions: Vec<CollisionGroup>,
    #[serde(default)]
    pub actors: Vec<ActorAssets>,
    pub contact_shadow: ContactShadow,
    pub toon_ramp: String,
    pub effects: String,
    /// Complete cooked dependency inventory, excluding this manifest itself.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

/// A soft textured ground quad, positioned from an animated actor joint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactShadow {
    pub texture: String,
    pub uv_size: [f32; 2],
    pub half_size: f32,
    pub height_offset: f32,
    pub alpha: u8,
    pub anchor_node: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorAssets {
    pub resource: u32,
    pub model_sha256: String,
    pub animation_sha256: String,
    pub parts: Vec<ScenePart>,
    /// Attachment geometry disabled when this field actor is created.
    #[serde(default)]
    pub hidden_nodes: Vec<u16>,
}

impl FieldAssets {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 5 && !self.parts.is_empty() && !self.ground.is_empty(),
            "unsupported or incomplete field assets"
        );
        validate_asset_path(&self.script.path)?;
        validate_asset_path(&self.messages)?;
        ensure!(
            !self.files.is_empty() && self.files.len() <= 4096,
            "field dependency inventory is missing; run cook-classroom"
        );
        for (path, hash) in &self.files {
            validate_asset_path(path)?;
            ensure!(
                hash.len() == 64 && hash.bytes().all(|v| v.is_ascii_hexdigit()),
                "invalid field dependency digest"
            );
        }
        let shadow = &self.contact_shadow;
        validate_asset_path(&self.toon_ramp)?;
        validate_asset_path(&self.effects)?;
        ensure!(
            self.files.contains_key(&self.effects),
            "field effects are missing from dependencies"
        );
        ensure!(
            self.files.contains_key(&self.toon_ramp),
            "toon ramp is missing from field dependencies"
        );
        validate_asset_path(&shadow.texture)?;
        ensure!(
            self.files.contains_key(&shadow.texture)
                && shadow
                    .uv_size
                    .iter()
                    .all(|v| v.is_finite() && *v > 0. && *v <= 1.)
                && shadow.half_size.is_finite()
                && (0. ..=1024.).contains(&shadow.half_size)
                && shadow.height_offset.is_finite()
                && (0. ..=32.).contains(&shadow.height_offset),
            "invalid contact shadow recipe"
        );
        ensure!(
            self.files.get(&self.script.path) == Some(&self.script.sha256)
                && self.files.contains_key(&self.messages)
                && self.files.contains_key("ui/dialogue.json"),
            "field dependencies are incomplete"
        );
        for hash in [&self.source_sha256, &self.script.sha256] {
            ensure!(
                hash.len() == 64 && hash.bytes().all(|v| v.is_ascii_hexdigit()),
                "invalid field source digest"
            );
        }
        for group in self.ground.iter().chain(&self.regions) {
            group.validate()?;
        }
        for part in self
            .parts
            .iter()
            .chain(self.actors.iter().flat_map(|c| &c.parts))
        {
            for chain in &part.secondary_motion {
                chain.validate(part.bone_names.len())?;
            }
            ensure!(
                part.clips
                    .iter()
                    .flat_map(|c| &c.secondary_pose_nodes)
                    .all(|node| usize::from(*node) < part.bone_names.len()),
                "secondary animation node exceeds skeleton"
            );
            ensure!(
                part.material_nodes.is_empty() || part.material_nodes.len() == part.materials.len(),
                "field material node mapping is incomplete"
            );
            ensure!(
                part.material_nodes
                    .iter()
                    .flatten()
                    .all(|node| usize::from(*node) < part.bone_names.len()),
                "field material node index exceeds skeleton"
            );
            validate_asset_path(&part.mesh)?;
            ensure!(
                self.files.contains_key(&part.mesh),
                "field mesh is missing from dependency inventory"
            );
            for texture in &part.textures {
                validate_asset_path(texture)?;
                ensure!(
                    self.files.contains_key(texture),
                    "field texture is missing from dependency inventory"
                );
            }
            ensure!(
                part.translation.iter().all(|v| v.is_finite()),
                "invalid field translation"
            );
            for material in &part.materials {
                ensure!(
                    material
                        .color
                        .iter()
                        .chain(&material.multiply)
                        .all(|b| b.texture < part.textures.len()),
                    "invalid field material texture"
                );
            }
        }
        for actor in &self.actors {
            ensure!(
                actor.parts.first().is_some_and(|part| actor
                    .hidden_nodes
                    .iter()
                    .all(|node| usize::from(*node) < part.bone_names.len())),
                "invalid initial actor visibility"
            );
        }
        Ok(())
    }
}
