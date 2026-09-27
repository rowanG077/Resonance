//! Read the indexed skeleton already carried by cooked GLB nodes. Attachment
//! sampling and rendering therefore use the same rest transforms and hierarchy.
use super::*;
use std::collections::BTreeMap;

impl Skeleton {
    pub fn from_glb(bytes: &[u8]) -> Result<Self> {
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
        let json: serde_json::Value = serde_json::from_slice(
            bytes
                .get(start..start.checked_add(length).context("GLB length overflow")?)
                .context("truncated GLB JSON")?,
        )?;
        let nodes = json["nodes"].as_array().context("GLB lacks nodes")?;
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
