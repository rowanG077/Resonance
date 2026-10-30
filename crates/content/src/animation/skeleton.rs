//! Read the indexed skeleton already carried by cooked GLB nodes. Attachment
//! sampling and rendering therefore use the same rest transforms and hierarchy.
use super::*;
use std::collections::BTreeMap;

/// Read gameplay-indexed node labels without decoding mesh buffers or building
/// a pose. Static geometry may have no indexed nodes.
pub fn joint_names_from_glb(bytes: &[u8]) -> Result<Vec<String>> {
    let json = glb_json(bytes)?;
    let nodes = json["nodes"].as_array().context("GLB lacks nodes")?;
    joint_names(nodes, &indexed_nodes(nodes)?)
}

fn joint_names(nodes: &[serde_json::Value], indices: &BTreeMap<u16, usize>) -> Result<Vec<String>> {
    indices
        .values()
        .map(|&node| {
            let name = nodes[node]["name"]
                .as_str()
                .context("unnamed cooked bone")?;
            ensure!(!name.is_empty(), "empty cooked bone name");
            Ok(name.into())
        })
        .collect()
}

/// The ordinary GLB hierarchy owns geometry visibility as well as transforms.
/// Group nodes inserted by an editor do not break the nearest joint binding.
pub(crate) fn scene_nodes_from_glb(bytes: &[u8]) -> Result<(Vec<String>, Vec<Vec<u16>>)> {
    let json = glb_json(bytes)?;
    let nodes = json["nodes"].as_array().context("GLB lacks nodes")?;
    let indices = indexed_nodes(nodes)?;
    let names = joint_names(nodes, &indices)?;
    let parents = node_parents(nodes)?;
    let mut joints = vec![None; nodes.len()];
    for (joint, node) in indices {
        joints[node] = Some(joint);
    }
    let mut bindings = vec![Vec::new(); json["meshes"].as_array().map_or(0, Vec::len)];
    for (index, node) in nodes.iter().enumerate() {
        let Some(mesh) = node.get("mesh") else {
            continue;
        };
        let mesh = usize::try_from(mesh.as_u64().context("invalid GLB mesh")?)?;
        let binding = bindings.get_mut(mesh).context("GLB mesh is missing")?;
        let mut cursor = Some(index);
        let mut depth = 0;
        while let Some(node) = cursor {
            ensure!(depth < nodes.len(), "cyclic GLB hierarchy");
            if let Some(joint) = joints[node] {
                if !binding.contains(&joint) {
                    binding.push(joint);
                }
                break;
            }
            cursor = parents[node];
            depth += 1;
        }
    }
    Ok((names, bindings))
}

pub(crate) fn node_parents(nodes: &[serde_json::Value]) -> Result<Vec<Option<usize>>> {
    let mut parents = vec![None; nodes.len()];
    for (parent, node) in nodes.iter().enumerate() {
        if let Some(children) = node["children"].as_array() {
            for child in children {
                let child = usize::try_from(child.as_u64().context("invalid GLB child")?)?;
                let target = parents.get_mut(child).context("GLB child is missing")?;
                ensure!(
                    target.replace(parent).is_none(),
                    "GLB node has multiple parents"
                );
            }
        }
    }
    Ok(parents)
}

fn indexed_nodes(nodes: &[serde_json::Value]) -> Result<BTreeMap<u16, usize>> {
    let mut indices = BTreeMap::new();
    for (node, value) in nodes.iter().enumerate() {
        if let Some(index) = value["extras"]["resonance_bone"].as_u64() {
            ensure!(
                indices.insert(u16::try_from(index)?, node).is_none(),
                "duplicate cooked bone index"
            );
        }
    }
    ensure!(
        indices.keys().copied().eq(0..u16::try_from(indices.len())?),
        "cooked bone indices are not contiguous"
    );
    Ok(indices)
}

impl Skeleton {
    pub fn from_glb(bytes: &[u8]) -> Result<Self> {
        let json = glb_json(bytes)?;
        Self::from_gltf_nodes(json["nodes"].as_array().context("GLB lacks nodes")?)
    }

    /// Read the skeleton and rest-pose bounds of its rigid mesh parts in one pass.
    pub fn from_glb_with_bounds(bytes: &[u8]) -> Result<(Self, Option<[[f32; 3]; 2]>)> {
        let json = glb_json(bytes)?;
        let nodes = json["nodes"].as_array().context("GLB lacks nodes")?;
        let skeleton = Self::from_gltf_nodes(nodes)?;
        let pose = skeleton.bind_pose()?;
        let mut bounds: Option<[[f32; 3]; 2]> = None;
        for node in nodes {
            let Some(bone) = node["extras"]["resonance_bone"].as_u64() else {
                continue;
            };
            let matrix = *pose
                .global
                .get(bone as usize)
                .context("missing mesh bone")?;
            for child in node["children"].as_array().into_iter().flatten() {
                let child = nodes
                    .get(usize::try_from(
                        child.as_u64().context("invalid mesh node")?,
                    )?)
                    .context("missing mesh node")?;
                let Some(mesh) = child["mesh"].as_u64().filter(|_| child["skin"].is_null()) else {
                    continue;
                };
                for primitive in json["meshes"][mesh as usize]["primitives"]
                    .as_array()
                    .context("missing mesh primitives")?
                {
                    let index = primitive["attributes"]["POSITION"]
                        .as_u64()
                        .context("missing mesh positions")?
                        as usize;
                    let accessor = &json["accessors"][index];
                    let low: [f32; 3] = serde_json::from_value(accessor["min"].clone())?;
                    let high: [f32; 3] = serde_json::from_value(accessor["max"].clone())?;
                    ensure!(
                        (0..3).all(|axis| low[axis].is_finite()
                            && high[axis].is_finite()
                            && low[axis] <= high[axis]),
                        "invalid rigid mesh bounds"
                    );
                    for corner in 0..8 {
                        let point = transform_point(
                            matrix,
                            std::array::from_fn(|axis| {
                                if corner & (1 << axis) == 0 {
                                    low[axis]
                                } else {
                                    high[axis]
                                }
                            }),
                        );
                        let bounds = bounds.get_or_insert([point; 2]);
                        for axis in 0..3 {
                            bounds[0][axis] = bounds[0][axis].min(point[axis]);
                            bounds[1][axis] = bounds[1][axis].max(point[axis]);
                        }
                    }
                }
            }
        }
        Ok((skeleton, bounds))
    }

    pub fn from_gltf_nodes(nodes: &[serde_json::Value]) -> Result<Self> {
        let parents = node_parents(nodes)?;
        let mut indices = BTreeMap::new();
        let mut bones = BTreeMap::new();
        for (index, node_index) in indexed_nodes(nodes)? {
            let node = &nodes[node_index];
            indices.insert(node_index, index);
            let bone = Bone {
                name: node["name"].as_str().context("unnamed cooked bone")?.into(),
                shadow_bounds: node["extras"]
                    .get("shadow_bounds")
                    .map(|value| value.as_bool().context("shadow_bounds must be a boolean"))
                    .transpose()?
                    .unwrap_or(true),
                parent: None,
                parent_transform: None,
                bind: rest_transform(node)?,
            };
            bones.insert(index, bone);
        }
        for (&node, index) in &indices {
            let bone = bones.get_mut(index).unwrap();
            let mut cursor = parents[node];
            let identity = Transform::default().matrix();
            let mut transform = identity;
            let mut depth = 0;
            while let Some(parent) = cursor {
                ensure!(depth < nodes.len(), "cyclic GLB hierarchy");
                if let Some(&index) = indices.get(&parent) {
                    bone.parent = Some(index);
                    break;
                }
                transform = multiply(rest_transform(&nodes[parent])?.matrix(), transform);
                cursor = parents[parent];
                depth += 1;
            }
            bone.parent_transform = (transform != identity).then_some(transform);
        }
        let skeleton = Self {
            bones: bones.into_values().collect(),
        };
        skeleton.validate()?;
        Ok(skeleton)
    }
}

pub(crate) fn rest_transform(node: &serde_json::Value) -> Result<Transform> {
    let Some(matrix) = node.get("matrix") else {
        // glTF omits identity TRS fields routinely, including on imported rigs.
        let transform: Transform = serde_json::from_value(node.clone())?;
        transform.validate()?;
        return Ok(transform);
    };
    ensure!(
        ["translation", "rotation", "scale"]
            .iter()
            .all(|key| node.get(key).is_none()),
        "GLB node combines matrix and TRS transforms"
    );
    let columns: [f32; 16] = serde_json::from_value(matrix.clone())?;
    let matrix = glam::Mat4::from_cols_array(&columns);
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    let transform = Transform {
        scale: scale.to_array(),
        rotation: rotation.to_array(),
        translation: translation.to_array(),
    };
    transform.validate()?;
    ensure!(
        transform
            .matrix()
            .iter()
            .flatten()
            .zip(columns)
            .all(|(value, original)| (*value - original).abs() <= 0.0001 * (1. + original.abs())),
        "GLB gameplay node matrix is not a TRS transform"
    );
    Ok(transform)
}

fn glb_json(bytes: &[u8]) -> Result<serde_json::Value> {
    const GLB_HEADER: usize = 12;
    const CHUNK_HEADER: usize = 8;
    const VERSION: u32 = 2;
    let word = |offset| -> Result<u32> {
        let bytes = bytes
            .get(offset..offset + 4)
            .context("truncated GLB header")?;
        Ok(u32::from_le_bytes(bytes.try_into()?))
    };
    ensure!(
        bytes.get(..4) == Some(b"glTF") && word(4)? == VERSION,
        "unsupported skeleton container"
    );
    ensure!(word(8)? as usize == bytes.len(), "invalid GLB length");
    ensure!(
        bytes.get(GLB_HEADER + 4..GLB_HEADER + CHUNK_HEADER) == Some(b"JSON"),
        "GLB lacks leading JSON chunk"
    );
    let start = GLB_HEADER + CHUNK_HEADER;
    let length = word(GLB_HEADER)? as usize;
    Ok(serde_json::from_slice(
        bytes
            .get(start..start.checked_add(length).context("GLB length overflow")?)
            .context("truncated GLB JSON")?,
    )?)
}
