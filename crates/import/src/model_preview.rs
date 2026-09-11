//! Shared conversion of preview mesh layers, outlines and idle animation.
use crate::{
    character::texture_palette,
    scene::{PartSource, cook_part},
};
use anyhow::{Context, Result};
use resonance_content::{CullFace, model_preview::PreviewPart};
use std::path::Path;

pub(crate) struct Layer<'a> {
    pub model: &'a [u8],
    pub outline: Option<&'a [u8]>,
    pub animation: Option<&'a [u8]>,
    pub attached_to: Option<String>,
    pub additive: bool,
}

pub(crate) fn layers(
    layer: Layer<'_>,
    parts: &mut Vec<PreviewPart>,
    name: &str,
    output: &Path,
    ktx: &Path,
) -> Result<()> {
    for (outline, source) in [(false, Some(layer.model)), (true, layer.outline)] {
        let Some(source) = source else { continue };
        let normalized = texture_palette(layer.model, source)?;
        let (mut scene, gltf, _) = cook_part(
            PartSource {
                name: &format!("{name}/{}", parts.len()),
                source: &normalized,
                resource: parts.len() as u16,
                draw_order: parts.len() as u32,
                depth_write: !layer.additive,
                translation: [0.; 3],
                autoplay: layer.animation,
                animation_slots: &[],
                clip_prefix: name,
                extra_clips: &[],
                texture_animations: Vec::new(),
            },
            output,
            ktx,
        )
        .with_context(|| format!("preview layer {}", parts.len()))?;
        let model = &normalized[crate::read::u32(&normalized, 4)? as usize..];
        let name_offset = crate::read::u32(model, 16)? as usize;
        let name = model.get(name_offset..).context("preview model name")?;
        let name = std::str::from_utf8(name.split(|b| *b == 0).next().unwrap())?;
        scene.secondary_motion =
            crate::secondary_motion::cook(&gltf, &scene.bone_names, name.contains("llo00"))?;
        if name.contains("col00") {
            crate::secondary_motion::colette(&mut scene.secondary_motion, &scene.bone_names)?;
        } else if name.contains("ref00") {
            crate::secondary_motion::raine(&mut scene.secondary_motion, &scene.bone_names)?;
        }
        if let Some(clip) = scene.clips.first_mut() {
            clip.secondary_pose_nodes = serde_json::from_value(
                gltf["animations"][0]["extras"]["secondary_pose_nodes"].clone(),
            )?;
        }
        if outline {
            scene.outline_color = Some([0, 0, 0, 127]);
            for material in &mut scene.materials {
                material.cull = CullFace::Front;
                material.blend = true;
            }
        } else if layer.additive {
            for material in &mut scene.materials {
                material.cull = CullFace::None;
                material.blend = true;
            }
        }
        parts.push(PreviewPart {
            scene,
            attached_to: layer.attached_to.clone(),
            additive: layer.additive,
            uv_offsets: Vec::new(),
        });
    }
    Ok(())
}
