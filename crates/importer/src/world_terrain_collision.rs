//! One native terrain tile's collision section directly into a GLB mesh.
use crate::dol;
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::CollisionGroup,
    overworld::{TerrainSurface, collision_mesh},
};
use std::{fs, path::Path};

pub(crate) fn section(bytes: &[u8], index: usize) -> Result<&[u8]> {
    let count = dol::u32(bytes, 0)? as usize;
    ensure!(
        count == 5 && index < count,
        "unsupported world tile package"
    );
    let offsets = (0..count)
        .map(|id| dol::u32(bytes, 4 + id * 4).map(|value| value as usize))
        .collect::<Result<Vec<_>>>()?;
    let start = offsets[index];
    let end = offsets
        .into_iter()
        .filter(|&offset| offset > start)
        .min()
        .unwrap_or(bytes.len());
    ensure!(
        start >= 4 + count * 4 && start < end,
        "missing tile section {index}"
    );
    bytes.get(start..end).context("tile section exceeds file")
}

fn surface(native: u32) -> Result<TerrainSurface> {
    use TerrainSurface::*;
    Ok(match native {
        0 | 8 | 9 | 12 | 14 => Barrier,
        1 => Grassland,
        2 => Road,
        3 => Wasteland,
        4 => Snowfield,
        5 => Desert,
        6 => Forest,
        7 => DeepForest,
        10 => Beach,
        11 => Water,
        13 => Bridge,
        15 => Pier,
        _ => anyhow::bail!("unknown world collision surface {native}"),
    })
}

fn groups(bytes: &[u8]) -> Result<Vec<CollisionGroup>> {
    let compact = dol::u32(bytes, 0)? != 0;
    let (header, stride, index_bytes, count) = if compact {
        (4, 16, 1, dol::u32(bytes, 0)? as usize)
    } else {
        (8, 20, 2, dol::u32(bytes, 4)? as usize)
    };
    ensure!(
        bytes.len() >= header && count <= (bytes.len() - header) / stride,
        "truncated world collision groups"
    );
    let table_end = header + count * stride;
    (0..count)
        .map(|id| {
            let at = header + id * stride;
            let range = |count_at, offset_at, size| -> Result<&[u8]> {
                let count = usize::from(dol::u16(bytes, at + count_at)?);
                let start = dol::u32(bytes, at + offset_at)? as usize;
                ensure!(start >= table_end, "collision payload overlaps group table");
                let end = start
                    .checked_add(count * size)
                    .context("collision range overflow")?;
                bytes
                    .get(start..end)
                    .context("collision payload exceeds tile")
            };
            let vertices = range(0, 4, 12)?
                .chunks_exact(12)
                .map(|row| {
                    Ok([
                        f32::from_bits(dol::u32(row, 0)?),
                        f32::from_bits(dol::u32(row, 4)?),
                        f32::from_bits(dol::u32(row, 8)?),
                    ])
                })
                .collect::<Result<Vec<_>>>()?;
            let triangles = range(2, 8, 3 * index_bytes)?
                .chunks_exact(3 * index_bytes)
                .map(|row| {
                    if compact {
                        Ok([row[0] as u16, row[1] as u16, row[2] as u16])
                    } else {
                        Ok([dol::u16(row, 0)?, dol::u16(row, 2)?, dol::u16(row, 4)?])
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let group = CollisionGroup {
                surface: surface(dol::u32(bytes, at + 12)?)?.index() as u32,
                vertices,
                triangles,
            };
            group.validate()?;
            Ok(group)
        })
        .collect()
}

pub(crate) fn collision(tile: &[u8]) -> Result<Vec<u8>> {
    Ok(collision_mesh::encode(&groups(section(tile, 4)?)?)?)
}

pub fn cook_world_terrain_collision(source: &Path, output: &Path) -> Result<()> {
    ensure!(!output.exists(), "world collision output already exists");
    let glb = collision(&fs::read(source)?)?;
    crate::output::write(&output, &glb)?;
    Ok(())
}
