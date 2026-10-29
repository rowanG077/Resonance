//! World collision uses ordinary GLB meshes with surface and order properties.
use crate::{field::CollisionGroup, overworld::TerrainSurface};
use anyhow::{Context, Result};
use serde_json::json;

pub fn encode(groups: &[CollisionGroup]) -> Result<Vec<u8>> {
    for group in groups {
        TerrainSurface::from_index(group.surface)?;
    }
    crate::collision_mesh::encode(groups.iter().enumerate().map(|(order, group)| {
        (
            group,
            format!(
                "{}-{}",
                TerrainSurface::from_index(group.surface).unwrap().name(),
                order + 1
            ),
            json!({"surface":TerrainSurface::from_index(group.surface).unwrap()}),
        )
    }))
}

pub fn decode(bytes: &[u8]) -> Result<Vec<CollisionGroup>> {
    crate::collision_mesh::decode(bytes, "surface")?
        .into_iter()
        .map(|(properties, mut group)| {
            let surface: TerrainSurface = serde_json::from_value(properties["surface"].clone())
                .context("invalid world collision surface")?;
            group.surface = surface.index() as u32;
            Ok(group)
        })
        .collect()
}

#[cfg(test)]
mod tests;
