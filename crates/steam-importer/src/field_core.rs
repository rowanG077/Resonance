//! Field scripts, Unicode messages and collision in the GameCube library layout.
use crate::{package::Package, pe, script, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::{CollisionGroup, collision_mesh, document::Collision},
    library::{FIELDS, Mappings},
};
use std::{fs, path::Path};

pub(crate) fn collision_groups(bytes: &[u8], short_indices: bool) -> Result<Vec<CollisionGroup>> {
    // Actor packages use short triangle indices even when the first header
    // word is nonzero. Field ground/region sections encode that choice there.
    let compact = !short_indices && pe::word(bytes, 0)? != 0;
    let (header, stride, index_bytes, count) = if compact {
        (4, 16, 1, pe::word(bytes, 0)? as usize)
    } else {
        (8, 20, 2, pe::word(bytes, 4)? as usize)
    };
    ensure!(
        count <= bytes.len().saturating_sub(header) / stride,
        "truncated field collision groups"
    );
    let table_end = header + count * stride;
    let mut groups = Vec::with_capacity(count);
    for index in 0..count {
        let at = header + index * stride;
        let vertex_count = pe::half(bytes, at)? as usize;
        let triangle_count = pe::half(bytes, at + 2)? as usize;
        let vertices_at = pe::word(bytes, at + 4)? as usize;
        let triangles_at = pe::word(bytes, at + 8)? as usize;
        ensure!(
            vertices_at >= table_end
                && vertices_at
                    .checked_add(vertex_count * 12)
                    .is_some_and(|end| end <= bytes.len())
                && triangles_at >= table_end
                && triangles_at
                    .checked_add(triangle_count * 3 * index_bytes)
                    .is_some_and(|end| end <= bytes.len()),
            "field collision geometry exceeds source"
        );
        let vertices = (0..vertex_count)
            .map(|vertex| {
                let at = vertices_at + vertex * 12;
                Ok([
                    f32::from_bits(pe::word(bytes, at)?),
                    f32::from_bits(pe::word(bytes, at + 4)?),
                    f32::from_bits(pe::word(bytes, at + 8)?),
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        let triangles = (0..triangle_count)
            .map(|triangle| {
                let at = triangles_at + triangle * 3 * index_bytes;
                if compact {
                    Ok([bytes[at] as u16, bytes[at + 1] as u16, bytes[at + 2] as u16])
                } else {
                    Ok([
                        pe::half(bytes, at)?,
                        pe::half(bytes, at + 2)?,
                        pe::half(bytes, at + 4)?,
                    ])
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let group = CollisionGroup {
            surface: pe::word(bytes, at + 12)?,
            vertices,
            triangles,
        };
        group.validate()?;
        groups.push(group);
    }
    Ok(groups)
}

pub(crate) fn cook(source: &Source, library: &Path, selected: Option<u32>) -> Result<()> {
    let fields: Mappings = serde_json::from_slice(&fs::read(library.join(FIELDS))?)?;
    if let Some(id) = selected {
        fields.target(id)?;
    }
    let image = pe::Image::parse(&source.executable)?;
    for (id, name) in fields
        .iter()
        .filter(|(id, _)| selected.is_none_or(|only| only == *id))
    {
        eprintln!("  Field source: {name} ({id})");
        let pointer = pe::word(image.slice(0x8cb908 + id * 24, 4)?, 0)?;
        let bytes = source.require(image.text(pointer)?)?;
        let package = Package::parse(&bytes)?;
        let native = package
            .reference(6)?
            .context("field has no scenario member")?;
        let (bytes, encoding) = source.scenario(native)?;
        let scenario = script::decode(&bytes, encoding)
            .with_context(|| format!("field {id} scenario {native}"))?;
        let base = library.join("fields").join(name);
        let bytes = crate::script_input::normalize(&scenario.bytes)
            .with_context(|| format!("field {id} input conventions"))?;
        crate::output::write(&base.join("scripts/scenario.ssb"), &bytes)?;
        if !scenario.messages.is_empty() {
            crate::output::json(&base.join("messages.json"), &scenario.messages)?;
        }
        let member = |index| -> Result<Vec<CollisionGroup>> {
            package
                .reference(index)?
                .map(|name| {
                    collision_groups(&source.require(name)?, false)
                        .with_context(|| format!("field {id} collision {name}"))
                })
                .transpose()
                .map(Option::unwrap_or_default)
        };
        let collision = Collision {
            ground: member(4)?,
            regions: member(5)?,
        };
        if collision
            .ground
            .iter()
            .chain(&collision.regions)
            .any(|g| !g.triangles.is_empty())
        {
            crate::output::write(
                &base.join("collision.glb"),
                &collision_mesh::encode(&collision)?,
            )?;
        }
    }
    Ok(())
}
