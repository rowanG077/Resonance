//! Shared conversion of preview mesh layers, outlines and idle animation.
use crate::{
    character::texture_palette,
    scene::{PartSource, SourceClip, cook_part},
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

#[cfg(test)]
pub(crate) fn preflight(layer: Layer<'_>) -> Result<()> {
    let mut errors = Vec::new();
    for (outline, source) in [(false, Some(layer.model)), (true, layer.outline)] {
        let Some(source) = source else { continue };
        let result = texture_palette(layer.model, source)
            .and_then(|source| crate::geometry::preflight_section(&source).map_err(Into::into));
        if let Err(error) = result {
            errors.push(format!(
                "{} layer: {error:#}",
                if outline { "outline" } else { "primary" }
            ));
        }
    }
    anyhow::ensure!(errors.is_empty(), "{}", errors.join("\n"));
    Ok(())
}

pub(crate) fn layers(
    layer: Layer<'_>,
    parts: &mut Vec<PreviewPart>,
    name: &str,
    output: &Path,
) -> Result<()> {
    layers_with_clips(layer, parts, name, &[], output)
}

pub(crate) fn layers_with_clips(
    layer: Layer<'_>,
    parts: &mut Vec<PreviewPart>,
    name: &str,
    clips: &[SourceClip<'_>],
    output: &Path,
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
                extra_clips: clips,
                shared_clips: &[],
                texture_animations: Vec::new(),
            },
            output,
        )
        .with_context(|| format!("preview layer {}", parts.len()))?;
        scene.secondary_motion =
            crate::secondary_motion::cook(&normalized, &gltf, &scene.bone_names)?;
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
            animation: layer
                .animation
                .and(scene.clips.first().map(|clip| clip.resource_slot)),
            scene,
            attached_to: layer.attached_to.clone(),
            additive: layer.additive,
            uv_offsets: Vec::new(),
        });
    }
    Ok(())
}
