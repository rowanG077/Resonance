//! Adapt copies of cooked meshes to Solari's fixed vertex layout.
use anyhow::{Result, ensure};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

pub(super) fn prepare(source: &Mesh) -> Result<Mesh> {
    ensure!(
        source.primitive_topology() == PrimitiveTopology::TriangleList,
        "Solari classroom meshes must contain triangle lists"
    );
    let positions = source
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|values| values.as_float3())
        .ok_or_else(|| anyhow::anyhow!("classroom mesh has no float3 positions"))?;
    ensure!(!positions.is_empty(), "classroom mesh is empty");
    let indices: Vec<u32> = source.indices().map_or_else(
        || (0..positions.len() as u32).collect(),
        |indices| indices.iter().map(|i| i as u32).collect(),
    );
    ensure!(
        !indices.is_empty()
            && indices.len().is_multiple_of(3)
            && indices.iter().all(|&i| (i as usize) < positions.len()),
        "classroom mesh has invalid triangle indices"
    );
    // Build a new mesh: colors, UV_1 and skin attributes are not supported by
    // Solari's 48-byte layout. Never mutate shared raster/oracle geometry.
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.to_vec());
    mesh.insert_indices(Indices::U32(indices));
    if let Some(normals) = source.attribute(Mesh::ATTRIBUTE_NORMAL) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone());
    } else {
        mesh.compute_smooth_normals();
    }
    if let Some(uv) = source.attribute(Mesh::ATTRIBUTE_UV_0) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv.clone());
    } else {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.; 2]; positions.len()]);
    }
    // No normal maps in this prototype, but Solari still requires tangents.
    // A basis from the normal also handles degenerate/missing texture UVs.
    let normals = mesh
        .attribute(Mesh::ATTRIBUTE_NORMAL)
        .unwrap()
        .as_float3()
        .ok_or_else(|| anyhow::anyhow!("classroom mesh has invalid normals"))?;
    let tangents: Vec<[f32; 4]> = normals
        .iter()
        .map(|normal| {
            let normal = Vec3::from_array(*normal).try_normalize().unwrap_or(Vec3::Z);
            normal.any_orthonormal_vector().extend(1.).to_array()
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    mesh.enable_raytracing = true;
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cooked_mesh_gets_exact_solari_layout_without_changing_source() {
        let mut source = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        source.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        );
        source.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.5; 4]; 3]);
        source.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.; 2]; 3]);
        source.insert_indices(Indices::U16(vec![0, 1, 2]));
        let mesh = prepare(&source).unwrap();
        assert_eq!(
            mesh.attributes().map(|(a, _)| a.id).collect::<Vec<_>>(),
            vec![
                Mesh::ATTRIBUTE_POSITION.id,
                Mesh::ATTRIBUTE_NORMAL.id,
                Mesh::ATTRIBUTE_UV_0.id,
                Mesh::ATTRIBUTE_TANGENT.id,
            ]
        );
        assert!(matches!(mesh.indices(), Some(Indices::U32(v)) if v == &[0, 1, 2]));
        assert!(mesh.enable_raytracing);
        assert_eq!(mesh.count_vertices(), 3);
        assert!(source.contains_attribute(Mesh::ATTRIBUTE_COLOR));
        assert!(source.contains_attribute(Mesh::ATTRIBUTE_UV_1));
        assert!(matches!(source.indices(), Some(Indices::U16(_))));
    }
    #[test]
    fn malformed_mesh_is_rejected() {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        assert!(prepare(&mesh).is_err());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.; 3]; 3]);
        mesh.insert_indices(Indices::U32(vec![0, 1, 9]));
        assert!(prepare(&mesh).is_err());
    }
}
