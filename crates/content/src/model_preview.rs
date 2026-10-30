//! Converted catalogue models shared by item viewers.
use crate::ScenePart;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPreview {
    /// Captured authoring entry point; never serialized as part of the model.
    #[serde(skip)]
    pub document: Option<String>,
    #[serde(
        default = "crate::defaults::unit",
        skip_serializing_if = "crate::defaults::is_unit"
    )]
    pub scale: f32,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub elevation: f32,
    #[serde(flatten, with = "crate::model_parts")]
    pub parts: Vec<PreviewPart>,
    /// Hide geometry on attachment anchors without hiding their children.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_geometry: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<crate::model_behavior::ModelBehaviorBinding>,
}

pub mod document;
#[cfg(test)]
mod metadata_tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewPart {
    #[serde(flatten)]
    pub scene: ScenePart,
    /// Animation resource slot to preview; None preserves the model's bind pose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<u16>,
    /// None shares the root transform; otherwise follows this primary-model bone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached_to: Option<String>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub additive: bool,
    /// Per-material offsets for the color and multiply texture coordinates.
    #[serde(default, skip_serializing_if = "zero_uv_offsets")]
    pub uv_offsets: Vec<[f32; 4]>,
}

fn zero_uv_offsets(offsets: &[[f32; 4]]) -> bool {
    offsets.iter().flatten().all(|&value| value == 0.)
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

impl ModelPreview {
    /// Validate catalogue metadata without opening the model media.
    pub fn validate_definition(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.scale.is_finite()
                && self.scale > 0.
                && self.elevation.is_finite()
                && !self.parts.is_empty(),
            "invalid model preview"
        );
        for part in &self.parts {
            if let Some((_, clip)) = part.selected_clip()? {
                anyhow::ensure!(
                    clip.duration_seconds.is_finite() && clip.duration_seconds >= 0.,
                    "invalid preview animation"
                );
            }
            crate::validate_asset_path(&part.scene.mesh)?;
            for texture in part
                .scene
                .textures
                .iter()
                .chain(part.scene.clips.iter().map(|clip| &clip.motion))
            {
                crate::validate_asset_path(texture)?;
            }
            anyhow::ensure!(
                part.scene.translation.iter().all(|v| v.is_finite())
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
        if let Some(behavior) = &self.behavior {
            behavior.validate()?;
        }
        Ok(())
    }

    /// Prepared models also validate bindings against their current GLB nodes.
    pub fn validate(&self) -> anyhow::Result<()> {
        self.validate_definition()?;
        let bones = &self.parts[0].scene.bone_names;
        for part in &self.parts {
            part.scene.validate(|_| true)?;
            anyhow::ensure!(
                part.attached_to
                    .as_ref()
                    .is_none_or(|bone| bones.contains(bone)),
                "invalid preview attachment"
            );
        }
        anyhow::ensure!(
            self.hidden_geometry.iter().all(|bone| bones.contains(bone)),
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
            document: None,
            scale: 1.,
            elevation: 0.,
            hidden_geometry: vec![],
            behavior: None,
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
                            joints: Default::default(),
                            motion: "clips/test.glb".into(),
                            resource_slot,
                            duration_seconds: (index + 1) as f32,
                            animation_resource: None,
                            pose_chains: vec![format!("chain-{index}")],
                        })
                        .collect(),
                    autoplay: false,
                    texture_animations: vec![],
                    bone_names: vec!["root".into(), "cape".into(), "tail".into()],
                    material_nodes: vec![],
                    outline_color: None,
                    secondary_motion: serde_json::from_value(serde_json::json!([
                        {"name":"chain-0","joints":["root","cape"],"gravity":0,"damping":0},
                        {"name":"chain-1","joints":["cape","tail"],"gravity":0,"damping":0},
                        {"name":"chain-2","joints":["tail","root"],"gravity":0,"damping":0}
                    ]))
                    .unwrap(),
                },
            }],
        };
        model.validate().unwrap();
        let (index, clip) = model.parts[0].selected_clip().unwrap().unwrap();
        assert_eq!(
            (index, clip.duration_ticks(), clip.pose_chains.as_slice()),
            (1, 120, &["chain-1".to_owned()][..])
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
