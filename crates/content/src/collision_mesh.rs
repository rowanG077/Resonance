//! Shared GLB geometry I/O. Domain readers interpret mesh-object custom properties.
use crate::field::CollisionGroup;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use resonance_asset_writer::gltf::{align4, append_floats, pack_glb};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// glTF is Y-up; gameplay stores horizontal X/Y followed by height.
fn to_gltf([x, y, height]: [f32; 3]) -> [f32; 3] {
    [x, height, -y]
}
fn from_gltf([x, height, z]: [f32; 3]) -> [f32; 3] {
    [x, -z, height]
}

pub(crate) fn encode<'a>(
    groups: impl IntoIterator<Item = (&'a CollisionGroup, String, Value)>,
) -> Result<Vec<u8>> {
    let mut binary = Vec::new();
    let mut views = Vec::<Value>::new();
    let mut accessors = Vec::<Value>::new();
    let mut meshes = Vec::<Value>::new();
    let mut nodes = Vec::<Value>::new();
    for (order, (group, name, mut properties)) in groups.into_iter().enumerate() {
        group.validate()?;
        properties["order"] = json!(order);
        let mut node = json!({"name":name,"extras":properties});
        if !group.triangles.is_empty() {
            let positions: Vec<_> = group.vertices.iter().copied().map(to_gltf).collect();
            let offset = append_floats(&mut binary, positions.iter().flatten().copied());
            let view = views.len();
            views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":positions.len()*12,"target":34962}));
            let min: [f32; 3] = std::array::from_fn(|i| {
                positions.iter().map(|p| p[i]).fold(f32::INFINITY, f32::min)
            });
            let max: [f32; 3] = std::array::from_fn(|i| {
                positions
                    .iter()
                    .map(|p| p[i])
                    .fold(f32::NEG_INFINITY, f32::max)
            });
            let position = accessors.len();
            accessors.push(json!({"bufferView":view,"componentType":5126,"count":positions.len(),"type":"VEC3","min":min,"max":max}));
            align4(&mut binary);
            let offset = binary.len();
            binary.extend(
                group
                    .triangles
                    .iter()
                    .flatten()
                    .flat_map(|i| i.to_le_bytes()),
            );
            let view = views.len();
            views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":group.triangles.len()*6,"target":34963}));
            let indices = accessors.len();
            accessors.push(json!({"bufferView":view,"componentType":5123,"count":group.triangles.len()*3,"type":"SCALAR"}));
            node["mesh"] = json!(meshes.len());
            meshes.push(json!({"name":name,"primitives":[{"attributes":{"POSITION":position},"indices":indices,"mode":4}]}));
        } else {
            ensure!(
                group.vertices.is_empty(),
                "collision group has vertices but no faces"
            );
        }
        nodes.push(node);
    }
    let mut document = json!({"asset":{"version":"2.0"},"scene":0,
        "scenes":[{"nodes":(0..nodes.len()).collect::<Vec<_>>()}],"nodes":nodes});
    if !meshes.is_empty() {
        document["meshes"] = json!(meshes);
        document["buffers"] = json!([{"byteLength":binary.len()}]);
        document["bufferViews"] = json!(views);
        document["accessors"] = json!(accessors);
    }
    pack_glb(&document, &binary)
}

pub(crate) fn decode(bytes: &[u8], property_key: &str) -> Result<Vec<(Value, CollisionGroup)>> {
    ensure!(bytes.starts_with(b"glTF"), "collision must be a GLB");
    let glb = gltf::Gltf::from_slice(bytes).context("decode collision GLB")?;
    ensure!(
        glb.extensions_required().next().is_none(),
        "unsupported required collision extension"
    );
    ensure!(
        glb.animations().len() == 0 && glb.skins().len() == 0,
        "collision must be static"
    );
    ensure!(
        glb.buffers().len() <= 1
            && glb
                .buffers()
                .all(|b| matches!(b.source(), gltf::buffer::Source::Bin)),
        "collision needs an embedded GLB buffer"
    );
    let binary = glb.blob.as_deref().unwrap_or_default();
    let scene = glb
        .default_scene()
        .or_else(|| glb.scenes().next())
        .context("collision has no scene")?;
    let mut groups = Vec::new();
    let mut visited = BTreeSet::new();
    for node in scene.nodes() {
        read_node(
            node,
            Mat4::IDENTITY,
            binary,
            property_key,
            &mut visited,
            &mut groups,
        )?;
    }
    // Imported order survives editor node sorting and arbitrary object renames.
    // Newly added meshes without an order follow existing groups in scene order.
    groups.sort_by_key(|(order, _, _)| *order);
    Ok(groups
        .into_iter()
        .map(|(_, properties, group)| (properties, group))
        .collect())
}

fn read_node(
    node: gltf::Node<'_>,
    parent: Mat4,
    binary: &[u8],
    property_key: &str,
    visited: &mut BTreeSet<usize>,
    groups: &mut Vec<(u32, Value, CollisionGroup)>,
) -> Result<()> {
    ensure!(
        visited.insert(node.index()),
        "collision node is repeated or cyclic"
    );
    let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    ensure!(transform.is_finite(), "nonfinite collision transform");
    let properties: Option<Value> = node
        .extras()
        .as_ref()
        .map(|raw| serde_json::from_str(raw.get()))
        .transpose()?;
    let properties = properties.filter(|value| value.get(property_key).is_some());
    let order = properties
        .as_ref()
        .and_then(|v| v.get("order"))
        .map(|v| serde_json::from_value::<u32>(v.clone()))
        .transpose()?
        .unwrap_or(u32::MAX);
    if let Some(mesh) = node.mesh() {
        let properties = properties
            .with_context(|| format!("collision mesh needs a {property_key} custom property"))?;
        let mut group = CollisionGroup {
            surface: 0,
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        for primitive in mesh.primitives() {
            ensure!(
                primitive.mode() == gltf::mesh::Mode::Triangles,
                "collision primitives must be triangles"
            );
            let reader = primitive.reader(|b| (b.index() == 0).then_some(binary));
            let positions: Vec<_> = reader
                .read_positions()
                .context("collision mesh lacks positions")?
                .collect();
            let indices: Vec<_> = match reader.read_indices() {
                Some(values) => values.into_u32().collect(),
                None => (0..u32::try_from(positions.len())?).collect(),
            };
            ensure!(
                indices.len().is_multiple_of(3),
                "incomplete collision triangle"
            );
            let base = group.vertices.len();
            for position in positions.iter().copied() {
                let position = if transform == Mat4::IDENTITY {
                    position
                } else {
                    transform.transform_point3(Vec3::from(position)).to_array()
                };
                group.vertices.push(from_gltf(position));
            }
            for triangle in indices.chunks_exact(3) {
                let mut face = [0; 3];
                for (slot, &index) in triangle.iter().enumerate() {
                    ensure!(
                        (index as usize) < positions.len(),
                        "collision index outside mesh"
                    );
                    face[slot] = u16::try_from(base + index as usize)
                        .context("collision group exceeds 65536 vertices")?;
                }
                // A reflected object transform reverses winding; preserve the
                // authored outward side for solid-volume and floor queries.
                if transform.determinant() < 0. {
                    face.swap(1, 2);
                }
                group.triangles.push(face);
            }
        }
        group.validate()?;
        groups.push((order, properties, group));
    } else if let Some(properties) = properties {
        groups.push((
            order,
            properties,
            CollisionGroup {
                surface: 0,
                vertices: Vec::new(),
                triangles: Vec::new(),
            },
        ));
    }
    for child in node.children() {
        read_node(child, transform, binary, property_key, visited, groups)?;
    }
    Ok(())
}
