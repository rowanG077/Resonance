//! Field script, dialogue and collision directly from each native archive.
use crate::{compression, dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::field::{CollisionGroup, collision_mesh, document::Collision};
use std::{fs, path::Path};
use symphonia_constants::FieldId;
use symphonia_script::{Program, message, scenario};

pub(crate) const PHASES: u32 = 0x801e4060;
pub(crate) const FIELD_COUNT: usize = 547;

pub(crate) fn optional_section(bytes: &[u8], index: usize) -> Result<Option<&[u8]>> {
    let count = dol::u32(bytes, 0)? as usize;
    ensure!(
        count > index && count <= bytes.len().saturating_sub(4) / 4,
        "invalid field section table"
    );
    let table_end = 4 + count * 4;
    let start = dol::u32(bytes, 4 + index * 4)? as usize;
    if start == 0 {
        return Ok(None);
    }
    ensure!(
        start >= table_end && start < bytes.len(),
        "invalid field section offset"
    );
    let end = (0..count)
        .map(|slot| dol::u32(bytes, 4 + slot * 4).map(|at| at as usize))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|&at| at > start && at <= bytes.len())
        .min()
        .unwrap_or(bytes.len());
    Ok(Some(&bytes[start..end]))
}

fn section(bytes: &[u8], index: usize) -> Result<&[u8]> {
    optional_section(bytes, index)?.context("missing field section")
}

pub(crate) fn field_name(id: usize) -> String {
    u16::try_from(id)
        .ok()
        .and_then(|raw| FieldId::try_from(raw).ok())
        .and_then(FieldId::resource_name)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("field-{id:04}"))
}

pub(crate) fn native_archive(
    extracted: &Path,
    exe: &[u8],
    row: &[u8],
    id: usize,
) -> Result<Option<Vec<u8>>> {
    let pointer = dol::u32(row, 0)?;
    if pointer == 0 {
        return Ok(None);
    }
    let source = text::string(exe, pointer)?;
    ensure!(
        !source.is_empty() && !source.contains(['/', '\\']) && source != "." && source != "..",
        "invalid native field source {source:?}"
    );
    let native = extracted
        .join("disc1/files/MAP")
        .join(source.to_ascii_lowercase());
    if !native.is_file() {
        return Ok(None);
    }
    Ok(Some(
        compression::cabinet(&fs::read(&native)?)
            .with_context(|| format!("expand field {id} from {}", native.display()))?,
    ))
}

pub(crate) fn read(extracted: &Path, id: u32) -> Result<(Vec<u8>, Vec<u8>)> {
    ensure!(
        (id as usize) < FIELD_COUNT,
        "field ID {id} is outside native catalogue"
    );
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let row = dol::slice(&exe, PHASES + id * 24, 24)?;
    let archive = native_archive(extracted, &exe, row, id as usize)?
        .with_context(|| format!("field {id} has no native archive"))?;
    Ok((exe, archive))
}

pub(crate) fn collision_groups(bytes: &[u8], short_indices: bool) -> Result<Vec<CollisionGroup>> {
    // Actor packages use short triangle indices even when the first header
    // word is nonzero. Field ground/region sections encode that choice there.
    let compact = !short_indices && dol::u32(bytes, 0)? != 0;
    let (header, stride, index_bytes, count) = if compact {
        (4, 16, 1, dol::u32(bytes, 0)? as usize)
    } else {
        (8, 20, 2, dol::u32(bytes, 4)? as usize)
    };
    ensure!(
        count <= bytes.len().saturating_sub(header) / stride,
        "truncated field collision groups"
    );
    let table_end = header + count * stride;
    let mut groups = Vec::with_capacity(count);
    for index in 0..count {
        let at = header + index * stride;
        let vertex_count = dol::u16(bytes, at)? as usize;
        let triangle_count = dol::u16(bytes, at + 2)? as usize;
        let vertices_at = dol::u32(bytes, at + 4)? as usize;
        let triangles_at = dol::u32(bytes, at + 8)? as usize;
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
                    f32::from_bits(dol::u32(bytes, at)?),
                    f32::from_bits(dol::u32(bytes, at + 4)?),
                    f32::from_bits(dol::u32(bytes, at + 8)?),
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
                        dol::u16(bytes, at)?,
                        dol::u16(bytes, at + 2)?,
                        dol::u16(bytes, at + 4)?,
                    ])
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let group = CollisionGroup {
            surface: dol::u32(bytes, at + 12)?,
            vertices,
            triangles,
        };
        group.validate()?;
        groups.push(group);
    }
    Ok(groups)
}

/// `selected` converts one field for a focused check; `None` converts all fields.
pub fn cook_field_core(extracted: &Path, library: &Path, selected: Option<u32>) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let phases = dol::slice(&exe, PHASES, FIELD_COUNT * 24)?;
    if let Some(id) = selected {
        ensure!(
            (id as usize) < FIELD_COUNT,
            "field ID {id} exceeds native catalogue"
        );
    }
    for (id, row) in phases.chunks_exact(24).enumerate() {
        if selected.is_some_and(|only| id != only as usize) {
            continue;
        }
        let Some(archive) = native_archive(extracted, &exe, row, id)? else {
            ensure!(selected.is_none(), "field {id} has no native source");
            continue;
        };
        let native = section(&archive, 6).with_context(|| format!("field {id}"))?;
        let script = crate::field_gameplay::prepare(id, native)?;
        let script = script.as_ref();
        let header =
            scenario::parse_header(script).with_context(|| format!("field {id} script"))?;
        Program::decode(script).with_context(|| format!("field {id} script"))?;
        let messages = message::parse(
            script
                .get(header.auxiliary_offset()..)
                .context("field message offset exceeds script")?,
        )
        .with_context(|| format!("field {id} messages"))?;
        let base = library.join("fields").join(field_name(id));
        crate::output::write(&base.join("scripts/scenario.ssb"), script)?;
        if !messages.is_empty() {
            crate::output::json(&base.join("messages.json"), &messages)?;
        }
        let collision = Collision {
            ground: optional_section(&archive, 4)?
                .map(|bytes| collision_groups(bytes, false))
                .transpose()?
                .unwrap_or_default(),
            regions: optional_section(&archive, 5)?
                .map(|bytes| collision_groups(bytes, false))
                .transpose()?
                .unwrap_or_default(),
        };
        if collision
            .ground
            .iter()
            .chain(&collision.regions)
            .any(|group| !group.triangles.is_empty())
        {
            crate::output::write(
                &base.join("collision.glb"),
                &collision_mesh::encode(&collision)?,
            )?;
        }
    }
    Ok(())
}
