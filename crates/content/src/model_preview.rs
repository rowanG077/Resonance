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
    /// Animation resource slot to preview; None preserves the model's bind pose.
    pub animation: Option<u16>,
    /// None shares the root transform; otherwise follows this primary-model bone.
    pub attached_to: Option<String>,
    pub additive: bool,
    /// Per-material offsets for the color and multiply texture coordinates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uv_offsets: Vec<[f32; 4]>,
}

impl PreviewPart {
    pub fn selected_clip(&self) -> anyhow::Result<Option<(usize, &crate::SceneClip)>> {
        let Some(slot) = self.animation else {
            return Ok(None);
        };
        let mut clips = self
            .scene
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| clip.resource_slot == slot);
        let selected = clips
            .next()
            .ok_or_else(|| anyhow::anyhow!("preview animation slot {slot} is absent"))?;
        anyhow::ensure!(
            clips.next().is_none(),
            "duplicate preview animation slot {slot}"
        );
        Ok(Some(selected))
    }
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
            if let Some((_, clip)) = part.selected_clip()? {
                anyhow::ensure!(
                    clip.duration_seconds.is_finite()
                        && clip.duration_seconds >= 0.
                        && clip
                            .secondary_pose_nodes
                            .iter()
                            .all(|&node| usize::from(node) < part.scene.bone_names.len()),
                    "invalid preview animation"
                );
            }
            for chain in &part.scene.secondary_motion.chains {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_selects_resource_slots_without_pruning_shared_clips() {
        let mut model = ModelPreview {
            scale: 1.,
            elevation: 0.,
            hidden_geometry: vec![],
            node_scales: vec![],
            parts: vec![PreviewPart {
                animation: Some(19),
                attached_to: None,
                additive: false,
                uv_offsets: vec![],
                scene: ScenePart {
                    resource: 0,
                    mesh: "shared/model.glb".into(),
                    textures: vec![],
                    materials: vec![],
                    appearance: None,
                    translation: [0.; 3],
                    clips: [4, 19, 2]
                        .into_iter()
                        .enumerate()
                        .map(|(index, resource_slot)| crate::SceneClip {
                            resource_slot,
                            duration_seconds: (index + 1) as f32,
                            animation_resource: None,
                            secondary_pose_nodes: vec![index as u16],
                        })
                        .collect(),
                    autoplay: false,
                    texture_animations: vec![],
                    bone_names: vec!["root".into(), "cape".into(), "tail".into()],
                    material_nodes: vec![],
                    outline_color: None,
                    secondary_motion: Default::default(),
                },
            }],
        };
        model.validate().unwrap();
        let (index, clip) = model.parts[0].selected_clip().unwrap().unwrap();
        assert_eq!(
            (
                index,
                clip.duration_ticks(),
                clip.secondary_pose_nodes.as_slice()
            ),
            (1, 120, &[1][..])
        );

        model.parts[0].animation = None;
        model.validate().unwrap();
        assert!(model.parts[0].selected_clip().unwrap().is_none());
        assert_eq!(model.parts[0].scene.clips.len(), 3);

        model.parts[0].animation = Some(1);
        assert!(model.validate().unwrap_err().to_string().contains("absent"));
        model.parts[0].animation = Some(19);
        model.parts[0].scene.clips[0].resource_slot = 19;
        assert!(
            model
                .validate()
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
    }
}
