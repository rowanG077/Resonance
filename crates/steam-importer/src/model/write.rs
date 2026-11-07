//! Direct model/image writers. Rendering uses the common GameCube material pass.
use super::*;
use crate::{output, source::Source, texture};
use resonance_content::{CullFace, SceneMaterial, ScenePart, TextureBinding};
use std::path::Path;

pub(crate) fn cook(
    source: &Source,
    library: &Path,
    name: &str,
    owner: &str,
    label: &str,
) -> Result<(ScenePart, Model)> {
    cook_on_rig(source, library, name, owner, label, None)
}

/// Synchronized equipment layers keep their own geometry but consume the
/// primary layer's indexed pose. Native outline labels can differ.
pub(crate) fn cook_on_rig(
    source: &Source,
    library: &Path,
    name: &str,
    owner: &str,
    label: &str,
    rig: Option<&resonance_content::animation::Skeleton>,
) -> Result<(ScenePart, Model)> {
    let descriptor = source.require(name)?;
    let dependencies = package::dependencies(&descriptor)?;
    let payload = source.require(dependencies.first().context("model has no payload")?)?;
    let mut model = Model::read(&descriptor, &payload)?;
    if let Some(rig) = rig {
        let original = resonance_content::animation::Skeleton::from_glb(
            &model.glb(&vec![[1.; 4]; model.materials.records.len()])?,
        )?;
        ensure!(
            original.bones.len() == rig.bones.len(),
            "outline rig node count differs"
        );
        for ((node, bone), target) in model
            .rig
            .nodes
            .iter_mut()
            .zip(&original.bones)
            .zip(&rig.bones)
        {
            // Body and outline exporters round rest transforms independently.
            // Check indexed topology and pose before adopting the body labels.
            let close = bone
                .bind
                .translation
                .into_iter()
                .chain(bone.bind.rotation)
                .chain(bone.bind.scale)
                .zip(
                    target
                        .bind
                        .translation
                        .into_iter()
                        .chain(target.bind.rotation)
                        .chain(target.bind.scale),
                )
                .all(|(a, b)| (a - b).abs() <= 0.0001 * a.abs().max(b.abs()).max(1.));
            ensure!(
                bone.parent == target.parent
                    && bone.parent_transform == target.parent_transform
                    && close,
                "outline rig transform differs at {}",
                bone.name
            );
            node.name = target.name.clone();
        }
    }
    let stem = name.rsplit_once('.').context("model has no extension")?.0;
    let (tints, blended, _) =
        material_parameters(&source.require(&format!("{stem}.mxd"))?, &model.materials)?;
    let mut textures = Vec::new();
    let mut transparent = Vec::new();
    let mut translucent = Vec::new();
    for (index, native) in model.materials.textures.iter().enumerate() {
        let descriptor =
            texture::Descriptor::read(&source.require(&format!("{}.totexb_d", native.name))?)?;
        let mut image = descriptor.decode(&source.require(&descriptor.payload)?)?;
        let pixels = &image.levels[0].rgba;
        transparent.push(pixels.chunks_exact(4).any(|p| p[3] != 255));
        translucent.push(pixels.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
        gc_cloud_alpha(name, &model.materials, index, &mut image)?;
        let path = format!("{owner}/textures/{label}-texture-{index}.ktx2");
        output::write(&library.join(&path), &image.ktx2()?)?;
        textures.push(path);
    }
    let mut materials = Vec::new();
    for mesh in model.meshes.iter().filter(|m| !m.indices.is_empty()) {
        let index = usize::from(mesh.material);
        let native = &model.materials.records[index];
        ensure!(
            native.textures.len() <= 2 && native.textures.len() <= mesh.texcoords.len(),
            "model {} requires unsupported texture stages",
            native.name
        );
        let bindings = native
            .textures
            .iter()
            .map(|slot| {
                let texture = model
                    .materials
                    .textures
                    .get(usize::from(slot.texture))
                    .context("missing material image")?;
                Ok(TextureBinding {
                    texture: usize::from(slot.texture),
                    wrap_u: texture.wrap[0],
                    wrap_v: texture.wrap[1],
                    nearest_min: false,
                    nearest_mag: false,
                    mipmapped: false,
                    scale: TextureBinding::unscaled(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let blend = blended[index]
            || mesh.colors.iter().any(|c| c[3] < 1.)
            || native
                .textures
                .iter()
                .any(|slot| transparent[usize::from(slot.texture)]);
        let partial = tints[index][3] < 1.
            || native
                .textures
                .iter()
                .any(|slot| translucent[usize::from(slot.texture)]);
        materials.push(SceneMaterial {
            image: Default::default(),
            color: bindings.first().cloned(),
            multiply: bindings.get(1).cloned(),
            vertex_color: true,
            blend,
            alpha_cutoff: 1,
            depth_write: native.variation_flags & 0x1000 != 0 && !partial,
            cull: if native.variation_flags & 0x4000 == 0 {
                CullFace::Back
            } else {
                CullFace::None
            },
            draw_order: materials.len() as u32,
        });
    }
    let path = format!("{owner}/meshes/{label}.glb");
    let bytes = model.glb(&tints)?;
    resonance_content::animation::Skeleton::from_glb(&bytes)?.bind_pose()?;
    output::write(&library.join(&path), &bytes)?;
    let mut part = ScenePart {
        resource: 0,
        mesh: path,
        textures,
        materials,
        appearance: None,
        translation: [0.; 3],
        clips: Vec::new(),
        prepared_motions: None,
        prepared_geometry: None,
        autoplay: false,
        texture_animations: Vec::new(),
        bone_names: Vec::new(),
        material_nodes: Vec::new(),
        outline_color: None,
        secondary_motion: Default::default(),
    };
    part.prepare_bones(&bytes)?;
    Ok((part, model))
}

/// The original GC Derris-Kharlan cloud image has opaque alpha. Steam's matching
/// cloud image adds luminance alpha; multiplying it into the shared GC pass
/// cuts holes through both the sky sphere and its lower gradient cylinder.
/// Retain Steam RGB, mip levels and independent MXD/vertex fades, and lower
/// only this source-qualified counterpart to the GC stage's opaque alpha.
fn gc_cloud_alpha(
    descriptor: &str,
    materials: &Materials,
    texture: usize,
    image: &mut texture::Decoded,
) -> Result<()> {
    if !descriptor.eq_ignore_ascii_case("DEL_D07_OBJ000.TOMDLB_D") || texture != 3 {
        return Ok(());
    }
    ensure!(
        materials.records.len() == 21
            && materials.textures.len() == 7
            && materials.textures[3]
                .name
                .eq_ignore_ascii_case("DEL_D07_PIC000_PIC003")
            && materials.textures[5]
                .name
                .eq_ignore_ascii_case("DEL_D07_PIC000_PIC005"),
        "Derris-Kharlan cloud texture directory changed"
    );
    for slot in [0, 18] {
        let material = &materials.records[slot];
        ensure!(
            material
                .name
                .eq_ignore_ascii_case(&format!("DEL_D07_OBJ000_MT{slot:02}"))
                && matches!(material.textures.as_slice(), [color, gradient]
                    if color.kind == 0 && color.texture == 3
                        && gradient.kind == 3 && gradient.texture == 5),
            "Derris-Kharlan cloud material binding changed"
        );
    }
    ensure!(
        materials
            .records
            .iter()
            .enumerate()
            .all(|(slot, material)| {
                matches!(slot, 0 | 18)
                    || material.textures.iter().all(|binding| binding.texture != 3)
            }),
        "Derris-Kharlan cloud image gained another material use"
    );
    for level in &mut image.levels {
        for pixel in level.rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Parameters {
    opacity: f32,
    blended: bool,
    role: u8,
    alpha_compare: u32,
    alpha_reference: u32,
}

fn parameters(bytes: &[u8], materials: &Materials) -> Result<Vec<Parameters>> {
    let input = Reader(bytes);
    ensure!(
        input.slice(0, 4)? == b"MXD " && input.word(4)? == 100,
        "unsupported material parameters"
    );
    let mut at = input.word(8)? as usize;
    let count = input.word(12)? as usize;
    ensure!(
        at >= 16 && count <= bytes.len().saturating_sub(at) / 16,
        "invalid material chunk directory"
    );
    let mut entries = None;
    for _ in 0..count {
        let length = input.word(at + 4)? as usize;
        ensure!(length >= 16, "invalid material chunk length");
        let chunk = Reader(input.slice(at, length)?);
        if chunk.slice(0, 4)? == b"CTAM" {
            ensure!(
                entries.is_none() && chunk.word(8)? == 100,
                "duplicate material chunk"
            );
            let start = chunk.word(16)? as usize;
            let count = chunk.word(20)? as usize;
            ensure!(
                start >= 32 && count <= length.saturating_sub(start) / 16,
                "invalid material directory"
            );
            chunk.slice(start, count * 16)?;
            let mut values = Vec::new();
            for index in 0..count {
                let row = start + index * 16;
                let size = chunk.word(row + 12)?;
                if size == 0 {
                    values.push(None);
                    continue;
                }
                ensure!(
                    chunk.word(row)? == 3 && size == 192,
                    "unsupported material entry"
                );
                let offset = chunk.word(row + 8)? as usize;
                ensure!(
                    offset >= start + count * 16,
                    "material data overlaps directory"
                );
                let data = Reader(chunk.slice(offset, 192)?);
                let blended = data.word(0)? != 0 && !(data.word(4)? == 1 && data.word(8)? == 0);
                let opacity = data.floats::<1>(0x40)?[0];
                ensure!(opacity.is_finite(), "invalid material opacity");
                values.push(Some(Parameters {
                    opacity: if blended { opacity.clamp(0., 1.) } else { 1. },
                    blended,
                    role: ((data.word(0x60)? >> 16) & 255) as u8,
                    alpha_compare: data.word(12)?,
                    alpha_reference: data.word(16)?,
                }));
            }
            entries = Some(values);
        }
        at = at.checked_add(length).context("material chunk overflow")?;
    }
    let entries = entries.context("missing material parameters")?;
    materials
        .records
        .iter()
        .map(|material| {
            let Some(index) = material.parameter else {
                return Ok(Parameters {
                    opacity: 1.,
                    blended: false,
                    role: 0,
                    alpha_compare: 4,
                    alpha_reference: 0,
                });
            };
            entries
                .get(index)
                .copied()
                .flatten()
                .context("missing material parameter entry")
        })
        .collect()
}

/// MXD initial opacity and atlas roles lower to the shared material pass.
pub(crate) fn material_parameters(
    bytes: &[u8],
    materials: &Materials,
) -> Result<(Vec<[f32; 4]>, Vec<bool>, Vec<u8>)> {
    let values = parameters(bytes, materials)?;
    Ok((
        values
            .iter()
            .map(|value| [1., 1., 1., value.opacity])
            .collect(),
        values.iter().map(|value| value.blended).collect(),
        values.into_iter().map(|value| value.role).collect(),
    ))
}

fn alpha_cutoff(compare: u32, reference: u32) -> Result<u8> {
    // 0x8a9030 selects native comparisons. The shared pass discards below
    // its cutoff, so Greater with reference 128 first accepts byte 129.
    Ok(match compare {
        0 | 1 => 0,
        4 => u8::try_from(
            reference
                .checked_add(1)
                .context("alpha reference overflow")?,
        )
        .context("alpha comparison rejects every fragment")?,
        8 => u8::try_from(reference).context("alpha reference exceeds byte range")?,
        _ => anyhow::bail!("alpha comparison {compare} cannot lower to a minimum cutoff"),
    })
}

pub(crate) fn material_alpha_cutoffs(bytes: &[u8], materials: &Materials) -> Result<Vec<u8>> {
    parameters(bytes, materials)?
        .into_iter()
        .map(|value| alpha_cutoff(value.alpha_compare, value.alpha_reference))
        .collect()
}

#[cfg(test)]
mod alpha_tests {
    use super::*;
    #[test]
    fn native_alpha_comparisons_keep_the_first_accepted_byte() -> Result<()> {
        assert_eq!(alpha_cutoff(4, 128)?, 129);
        assert_eq!(alpha_cutoff(4, 0)?, 1);
        assert_eq!(alpha_cutoff(8, 128)?, 128);
        assert_eq!(alpha_cutoff(0, 0)?, 0);
        assert!(alpha_cutoff(3, 128).is_err());
        assert!(alpha_cutoff(4, 255).is_err());
        Ok(())
    }
}
