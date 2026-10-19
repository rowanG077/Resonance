//! Read the indexed skeleton already carried by cooked GLB nodes. Attachment
//! sampling and rendering therefore use the same rest transforms and hierarchy.
use super::*;
use std::collections::BTreeMap;

impl Skeleton {
    pub fn from_glb(bytes: &[u8]) -> Result<Self> {
        let json = glb_json(bytes)?;
        Self::from_nodes(json["nodes"].as_array().context("GLB lacks nodes")?)
    }

    /// Read the skeleton and rest-pose bounds of its rigid mesh parts in one pass.
    pub fn from_glb_with_bounds(bytes: &[u8]) -> Result<(Self, Option<[[f32; 3]; 2]>)> {
        let json = glb_json(bytes)?;
        let nodes = json["nodes"].as_array().context("GLB lacks nodes")?;
        let skeleton = Self::from_nodes(nodes)?;
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
                let child = &nodes[child.as_u64().context("invalid mesh node")? as usize];
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

    fn from_nodes(nodes: &[serde_json::Value]) -> Result<Self> {
        let mut indices = BTreeMap::new();
        let mut bones = BTreeMap::new();
        for (node_index, node) in nodes.iter().enumerate() {
            let Some(index) = node["extras"]["resonance_bone"].as_u64() else {
                continue;
            };
            let index = u16::try_from(index)?;
            indices.insert(node_index, index);
            let bone = Bone {
                name: node["name"].as_str().context("unnamed cooked bone")?.into(),
                parent: None,
                bind_channels: TransformChannels(0),
                bind: serde_json::from_value(node.clone())?,
            };
            ensure!(
                bones.insert(index, bone).is_none(),
                "duplicate cooked bone index"
            );
        }
        for (&node, &parent) in &indices {
            if let Some(children) = nodes[node]["children"].as_array() {
                for child in children {
                    let child = usize::try_from(child.as_u64().context("invalid GLB child")?)?;
                    ensure!(child < nodes.len(), "GLB child is missing");
                    if let Some(index) = indices.get(&child) {
                        ensure!(
                            bones
                                .get_mut(index)
                                .unwrap()
                                .parent
                                .replace(parent)
                                .is_none(),
                            "cooked bone has multiple parents"
                        );
                    }
                }
            }
        }
        ensure!(
            bones.keys().copied().eq(0..u16::try_from(bones.len())?),
            "cooked bone indices are not contiguous"
        );
        let skeleton = Self {
            bones: bones.into_values().collect(),
        };
        skeleton.validate()?;
        Ok(skeleton)
    }
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
