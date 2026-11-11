//! One GPU vertex interface for imported surfaces. Preserve absent channels as
//! data instead of specializing a new shader for every glTF attribute subset.
use bevy::{
    asset::RenderAssetUsages,
    gltf::gltf::{self, mesh::util::ReadIndices},
    mesh::{Indices, MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
    render::render_resource::VertexFormat,
};

pub(super) const FLAGS: MeshVertexAttribute =
    MeshVertexAttribute::new("SurfaceChannels", 0x7265736f6e616e63, VertexFormat::Uint32);

/// Padded GPU inputs do not supply UVs missing from the authored geometry.
pub(crate) fn has_uv(mesh: &Mesh, channel: usize) -> bool {
    let attribute = match channel {
        0 => Mesh::ATTRIBUTE_UV_0,
        1 => Mesh::ATTRIBUTE_UV_1,
        _ => return false,
    };
    match mesh.attribute(FLAGS) {
        Some(VertexAttributeValues::Uint32(flags)) => {
            // The loader writes one authored mask across the whole primitive.
            flags
                .first()
                .is_some_and(|flags| flags & (2 << channel) != 0)
        }
        _ => mesh.contains_attribute(attribute),
    }
}

pub(super) fn load(
    primitive: &gltf::Primitive<'_>,
    buffers: &[Vec<u8>],
    usage: RenderAssetUsages,
    rotate: bool,
    skinned: bool,
) -> Option<Mesh> {
    // Leave unsupported topologies and morph processing to the original loader.
    let topology = match primitive.mode() {
        gltf::mesh::Mode::Points => PrimitiveTopology::PointList,
        gltf::mesh::Mode::Lines => PrimitiveTopology::LineList,
        gltf::mesh::Mode::LineStrip => PrimitiveTopology::LineStrip,
        gltf::mesh::Mode::Triangles => PrimitiveTopology::TriangleList,
        gltf::mesh::Mode::TriangleStrip => PrimitiveTopology::TriangleStrip,
        _ => return None,
    };
    if primitive.morph_targets().len() != 0 {
        return None;
    }
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
    // Authored surfaces use these channels and optional skinning. They have no
    // tangent/normal-map stage; unused glTF attributes do not vary this layout.
    let convert = |[x, y, z]: [f32; 3]| if rotate { [-x, y, -z] } else { [x, y, z] };
    let positions: Vec<_> = reader.read_positions()?.map(convert).collect();
    let mut mesh = Mesh::new(topology, usage);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    if let Some(normals) = reader.read_normals() {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            normals.map(convert).collect::<Vec<_>>(),
        );
    }
    if let Some(uv) = reader.read_tex_coords(0) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv.into_f32().collect::<Vec<_>>());
    }
    if let Some(uv) = reader.read_tex_coords(1) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv.into_f32().collect::<Vec<_>>());
    }
    if let Some(colors) = reader.read_colors(0) {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_COLOR,
            colors.into_rgba_f32().collect::<Vec<_>>(),
        );
    }
    if skinned {
        if let Some(joints) = reader.read_joints(0) {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_JOINT_INDEX,
                VertexAttributeValues::Uint16x4(joints.into_u16().collect()),
            );
        }
        if let Some(weights) = reader.read_weights(0) {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_JOINT_WEIGHT,
                weights.into_f32().collect::<Vec<_>>(),
            );
        }
    }
    if let Some(indices) = reader.read_indices() {
        mesh.insert_indices(match indices {
            ReadIndices::U8(indices) => Indices::U16(indices.map(u16::from).collect()),
            ReadIndices::U16(indices) => Indices::U16(indices.collect()),
            ReadIndices::U32(indices) => Indices::U32(indices.collect()),
        });
    }
    // Match Bevy's glTF fallback before filling the canonical layout.
    if !mesh.contains_attribute(Mesh::ATTRIBUTE_NORMAL)
        && topology == PrimitiveTopology::TriangleList
    {
        mesh.duplicate_vertices();
        mesh.compute_flat_normals();
    }
    let count = mesh.count_vertices();
    let mut flags = 0u32;
    for (attribute, bit) in [
        (Mesh::ATTRIBUTE_NORMAL, 1),
        (Mesh::ATTRIBUTE_UV_0, 2),
        (Mesh::ATTRIBUTE_UV_1, 4),
        (Mesh::ATTRIBUTE_COLOR, 8),
    ] {
        if mesh.contains_attribute(attribute) {
            flags |= bit;
        }
    }
    if flags & 1 == 0 {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.; 3]; count]);
    }
    if flags & 2 == 0 {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.; 2]; count]);
    }
    if flags & 4 == 0 {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.; 2]; count]);
    }
    if flags & 8 == 0 {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.; 4]; count]);
    }
    mesh.insert_attribute(FLAGS, vec![flags; count]);
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_channels_keep_their_identity_in_one_vertex_layout() -> anyhow::Result<()> {
        let mut layouts = bevy::mesh::MeshVertexBufferLayouts::default();
        let mut expected = None;
        for channels in 0..4 {
            let mut attributes = serde_json::json!({"POSITION":0});
            if channels & 1 != 0 {
                attributes["TEXCOORD_0"] = serde_json::json!(1);
            }
            if channels & 2 != 0 {
                attributes["TEXCOORD_1"] = serde_json::json!(1);
            }
            let document = serde_json::json!({
                "asset":{"version":"2.0"},
                "buffers":[{"byteLength":60}],
                "bufferViews":[{"buffer":0,"byteLength":36},
                    {"buffer":0,"byteOffset":36,"byteLength":24}],
                "accessors":[{"bufferView":0,"componentType":5126,"count":3,
                    "type":"VEC3","min":[0,0,0],"max":[1,1,0]},
                    {"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}],
                "meshes":[{"primitives":[{"attributes":attributes}]}]
            });
            let source = gltf::Gltf::from_slice(&serde_json::to_vec(&document)?)?;
            let data: Vec<u8> = [0f32, 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 1.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect();
            let mesh = load(
                &source.meshes().next().unwrap().primitives().next().unwrap(),
                &[data],
                RenderAssetUsages::default(),
                false,
                false,
            )
            .unwrap();
            assert!(
                matches!(mesh.attribute(FLAGS), Some(VertexAttributeValues::Uint32(flags))
                if flags == &vec![1 | (channels << 1);3])
            );
            assert!(mesh.contains_attribute(Mesh::ATTRIBUTE_UV_0));
            assert!(mesh.contains_attribute(Mesh::ATTRIBUTE_UV_1));
            assert_eq!(has_uv(&mesh, 0), channels & 1 != 0);
            assert_eq!(has_uv(&mesh, 1), channels & 2 != 0);
            assert!(matches!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
                Some(VertexAttributeValues::Float32x3(normals)) if normals == &vec![[0.,0.,1.];3]));
            let layout = mesh.get_mesh_vertex_buffer_layout(&mut layouts);
            if let Some(expected) = &expected {
                assert_eq!(&layout, expected);
            } else {
                expected = Some(layout);
            }
        }
        Ok(())
    }
}
