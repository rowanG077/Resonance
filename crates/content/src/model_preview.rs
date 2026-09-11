//! Converted catalogue models shared by item viewers.
use crate::ScenePart;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPreview {
    pub scale: f32,
    pub elevation: f32,
    pub parts: Vec<PreviewPart>,
    /// Hide geometry on attachment anchors without hiding their children.
    pub hidden_geometry: Vec<String>,
    pub node_scales: Vec<NodeScale>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewPart {
    pub scene: ScenePart,
    /// None shares the root transform; otherwise follows this primary-model bone.
    pub attached_to: Option<String>,
    pub additive: bool,
    /// Per-material offsets for the color and multiply texture coordinates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uv_offsets: Vec<[f32; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeScale {
    pub bone: String,
    pub scale: [f32; 3],
    /// Apply only while this persistent event flag is unset.
    pub unless_flag: Option<u16>,
}

impl ModelPreview {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.scale.is_finite()
                && self.scale > 0.
                && self.elevation.is_finite()
                && !self.parts.is_empty(),
            "invalid model preview"
        );
        let bones = &self.parts[0].scene.bone_names;
        for part in &self.parts {
            for chain in &part.scene.secondary_motion {
                chain.validate(part.scene.bone_names.len())?;
            }
            crate::validate_asset_path(&part.scene.mesh)?;
            for texture in &part.scene.textures {
                crate::validate_asset_path(texture)?;
            }
            anyhow::ensure!(
                part.attached_to
                    .as_ref()
                    .is_none_or(|bone| bones.contains(bone))
                    && part.scene.translation.iter().all(|v| v.is_finite())
                    && part.scene.materials.iter().all(|m| m
                        .color
                        .iter()
                        .chain(&m.multiply)
                        .all(|b| b.texture < part.scene.textures.len()))
                    && (part.uv_offsets.is_empty()
                        || part.uv_offsets.len() == part.scene.materials.len())
                    && part.uv_offsets.iter().flatten().all(|v| v.is_finite()),
                "invalid preview part"
            );
        }
        anyhow::ensure!(
            self.hidden_geometry.iter().all(|bone| bones.contains(bone))
                && self.node_scales.iter().all(|n| bones.contains(&n.bone)
                    && n.scale.iter().all(|v| v.is_finite() && *v >= 0.)),
            "invalid preview bones"
        );
        Ok(())
    }
}
