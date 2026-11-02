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
    let descriptor = source.require(name)?;
    let dependencies = package::dependencies(&descriptor)?;
    let payload = source.require(dependencies.first().context("model has no payload")?)?;
    let model = Model::read(&descriptor, &payload)?;
    let stem = name.rsplit_once('.').context("model has no extension")?.0;
    let (tints, blended) =
        material_opacity(&source.require(&format!("{stem}.mxd"))?, &model.materials)?;
    let mut textures = Vec::new();
    let mut transparent = Vec::new();
    let mut translucent = Vec::new();
    for (index, native) in model.materials.textures.iter().enumerate() {
        let descriptor =
            texture::Descriptor::read(&source.require(&format!("{}.totexb_d", native.name))?)?;
        let image = descriptor.decode(&source.require(&descriptor.payload)?)?;
        let pixels = &image.levels[0].rgba;
        transparent.push(pixels.chunks_exact(4).any(|p| p[3] != 255));
        translucent.push(pixels.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
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

/// MXD owns initial material opacity. Its shader programs are not cooked into a
/// second rendering system; geometry colors and the shared GC pass supply RGB.
fn material_opacity(bytes: &[u8], materials: &Materials) -> Result<(Vec<[f32; 4]>, Vec<bool>)> {
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
                values.push(Some((
                    if blended { opacity.clamp(0., 1.) } else { 1. },
                    blended,
                )));
            }
            entries = Some(values);
        }
        at = at.checked_add(length).context("material chunk overflow")?;
    }
    let entries = entries.context("missing material parameters")?;
    let values = materials
        .records
        .iter()
        .map(|material| {
            let Some(index) = material.parameter else {
                return Ok((1., false));
            };
            entries
                .get(index)
                .copied()
                .flatten()
                .context("missing material parameter entry")
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((
        values
            .iter()
            .map(|&(alpha, _)| [1., 1., 1., alpha])
            .collect(),
        values.into_iter().map(|(_, blend)| blend).collect(),
    ))
}
