//! Bind original preview layers and sparse curves in the shared source-model DAG.
use crate::scene::decoded::Package;
use crate::{
    animation::{AuthoredAnimation, ModelBindings},
    character::Clip,
    scene::{glb::Glb, source::Models},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{CullFace, SceneClip, ScenePart, model_preview::PreviewPart};
use std::{collections::BTreeSet, ops::Range, path::Path};

pub(crate) struct Layer<'a> {
    pub model: &'a [u8],
    pub outline: Option<&'a [u8]>,
    pub animation: Option<&'a AuthoredAnimation>,
    pub attached_to: Option<String>,
    pub additive: bool,
}

/// Aliased pointers borrow the same bounded original member.
pub(crate) struct PointerMembers<'a> {
    bytes: &'a [u8],
    offsets: BTreeSet<usize>,
}

impl<'a> PointerMembers<'a> {
    pub(crate) fn new(bytes: &'a [u8], fields: Range<usize>) -> Result<Self> {
        let mut offsets = BTreeSet::new();
        let header = fields.end;
        for field in fields.step_by(4) {
            let start = crate::read::u32(bytes, field)? as usize;
            ensure!(
                start == 0 || (header..bytes.len()).contains(&start),
                "member exceeds source package"
            );
            if start != 0 {
                offsets.insert(start);
            }
        }
        Ok(Self { bytes, offsets })
    }

    pub(crate) fn model(&self, field: usize) -> Result<Option<&'a [u8]>> {
        let start = crate::read::u32(self.bytes, field)? as usize;
        (start != 0)
            .then(|| {
                ensure!(self.offsets.contains(&start), "undeclared source member");
                let end = self
                    .offsets
                    .range(start + 1..)
                    .next()
                    .copied()
                    .unwrap_or(self.bytes.len());
                Ok(&self.bytes[start..end])
            })
            .transpose()
    }

    pub(crate) fn animation(
        &self,
        field: usize,
        decoded: &mut Package,
    ) -> Result<Option<std::sync::Arc<AuthoredAnimation>>> {
        self.model(field)?
            .map(|bytes| decoded.decode_animation(bytes, || crate::animation::read_member(bytes)))
            .transpose()
    }
}

pub(crate) fn layers(
    layer: Layer<'_>,
    parts: &mut Vec<PreviewPart>,
    name: &str,
    output: &Path,
    decoded: &mut Package,
) -> Result<()> {
    layers_with_clips(layer, parts, name, &[], output, decoded)
}

pub(crate) fn layers_with_clips(
    layer: Layer<'_>,
    parts: &mut Vec<PreviewPart>,
    name: &str,
    clips: &[Clip<'_>],
    output: &Path,
    decoded: &mut Package,
) -> Result<()> {
    for original in [Some(layer.model), layer.outline].into_iter().flatten() {
        decoded.decode_model(original, layer.model, output)?;
    }
    let first_resource = parts.len();
    let mut models = Models::new(output, decoded);
    for (index, (outline, original)) in [(false, Some(layer.model)), (true, layer.outline)]
        .into_iter()
        .enumerate()
    {
        let Some(original) = original else { continue };
        let resource = u16::try_from(first_resource + index)?;
        let additive = layer.additive;
        let animation = layer.animation;
        models.add(
            &format!("{name}/{resource}"),
            original,
            layer.model,
            move |geometry, _, scene, glb| {
                scene.resource = resource;
                style(scene, outline, additive);
                if animation.is_some() || !clips.is_empty() {
                    bind_clips(
                        scene,
                        glb,
                        geometry
                            .bindings
                            .as_ref()
                            .context("animated preview lacks motion bindings")?,
                        animation,
                        clips,
                    )?;
                }
                scene.autoplay = animation.is_some();
                Ok(())
            },
        )?;
    }
    parts.extend(
        models
            .finish()
            .into_iter()
            .map(|cooked| layer.part(cooked.part)),
    );
    Ok(())
}

impl Layer<'_> {
    pub(crate) fn part(&self, scene: ScenePart) -> PreviewPart {
        PreviewPart {
            animation: self
                .animation
                .and(scene.clips.first().map(|clip| clip.resource_slot)),
            scene,
            attached_to: self.attached_to.clone(),
            additive: self.additive,
            uv_offsets: Vec::new(),
        }
    }
}

pub(crate) fn style(scene: &mut ScenePart, outline: bool, additive: bool) {
    for material in &mut scene.materials {
        material.draw_order +=
            u32::from(scene.resource) * resonance_content::field::MODEL_DRAW_SPAN;
        material.depth_write = !additive;
        if outline || additive {
            material.cull = if outline {
                CullFace::Front
            } else {
                CullFace::None
            };
            material.blend = true;
        }
    }
    scene.outline_color = outline.then_some([0, 0, 0, 127]);
}

pub(crate) fn bind_clips(
    scene: &mut ScenePart,
    glb: &mut Glb,
    model: &ModelBindings,
    animation: Option<&AuthoredAnimation>,
    clips: &[Clip<'_>],
) -> Result<()> {
    if let Some(animation) = animation {
        scene.clips.push(glb.animate(animation.motion(model)?)?);
    }
    for clip in clips {
        scene.clips.push(SceneClip {
            resource_slot: clip.slot,
            animation_resource: clip.resource,
            ..glb.animate(clip.animation.motion(model)?)?
        });
    }
    Ok(())
}

#[test]
fn pointer_members_preserve_aliases_and_bound_each_source() -> Result<()> {
    let bytes = [0_u32, 24, 28, 24, 0, 0, 0x12345678, 0xabcdef01]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect::<Vec<_>>();
    let members = PointerMembers::new(&bytes, 4..24)?;
    assert_eq!(members.model(4)?, members.model(12)?);
    assert_eq!(
        members.model(4)?,
        Some(0x12345678_u32.to_be_bytes().as_slice())
    );
    assert_eq!(
        members.model(8)?,
        Some(0xabcdef01_u32.to_be_bytes().as_slice())
    );
    assert!(members.model(16)?.is_none());
    assert!(members.model(24).is_err());
    assert!(PointerMembers::new(&bytes[..26], 4..24).is_err());
    Ok(())
}
