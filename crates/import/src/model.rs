//! Relocatable models: a fixed node table, a linked hierarchy and packed labels.

use crate::read::{u16 as half, u32 as word};
use anyhow::{Context, Result, ensure};
use serde::Serialize;

const HEADER_SIZE: usize = 0x20;
const NODE_SIZE: usize = 0x1c;
const TRANSFORM_WORDS: usize = 13;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Node {
    pub source_offset: usize,
    pub data_offset: u32,
    pub previous_offset: u32,
    pub next_offset: u32,
    pub parent_offset: u32,
    pub child_offset: u32,
    pub object_index: u16,
    pub node_id: u16,
    pub transform_kind: u8,
    pub draw_priority: u8,
    pub flags: u16,
    pub data_words: Vec<u32>,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Model {
    pub field4: u16,
    pub field8: u32,
    pub root_offset: u32,
    pub name_offset: u32,
    pub root_geometry: u16,
    pub field16: u16,
    pub name_table_metadata: u32,
    pub names_offset: u32,
    pub name: Option<String>,
    /// Packed labels and nodes share the native depth-first construction order.
    pub names: Option<Vec<String>>,
    pub nodes: Vec<Node>,
}

impl Model {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(word(bytes, 0)? == 0x007b_7960, "invalid model magic");
        let count = usize::from(half(bytes, 6)?);
        let end = HEADER_SIZE + count * NODE_SIZE;
        ensure!(end <= bytes.len(), "model node table exceeds resource");
        let root_offset = word(bytes, 12)?;
        let name_offset = word(bytes, 16)?;
        let name_table_metadata = word(bytes, 24)?;
        let names_offset = word(bytes, 28)?;
        ensure!(
            names_offset == 0 || name_table_metadata != 0,
            "model names offset has no relocation metadata"
        );
        let node_index = |pointer: u32| -> Result<usize> {
            let offset = pointer as usize;
            ensure!(
                (HEADER_SIZE..end).contains(&offset)
                    && (offset - HEADER_SIZE).is_multiple_of(NODE_SIZE),
                "invalid model node pointer {pointer:#x}"
            );
            Ok((offset - HEADER_SIZE) / NODE_SIZE)
        };
        let mut physical = Vec::with_capacity(count);
        for offset in (HEADER_SIZE..end).step_by(NODE_SIZE) {
            let data_offset = word(bytes, offset)?;
            let data_words = if data_offset == 0 {
                vec![]
            } else {
                let start = data_offset as usize;
                let end = start
                    .checked_add(TRANSFORM_WORDS * 4)
                    .context("model transform extent overflow")?;
                let data = bytes
                    .get(start..end)
                    .context("model transform exceeds resource")?;
                data.chunks_exact(4)
                    .map(|word| u32::from_be_bytes(word.try_into().unwrap()))
                    .collect()
            };
            let mut links = [0; 4];
            for (index, pointer) in links.iter_mut().enumerate() {
                *pointer = word(bytes, offset + 4 + index * 4)?;
                if *pointer != 0 {
                    node_index(*pointer)?;
                }
            }
            physical.push(Some(Node {
                source_offset: offset,
                data_offset,
                previous_offset: links[0],
                next_offset: links[1],
                parent_offset: links[2],
                child_offset: links[3],
                object_index: half(bytes, offset + 20)?,
                node_id: half(bytes, offset + 22)?,
                transform_kind: bytes[offset + 24],
                draw_priority: bytes[offset + 25],
                flags: half(bytes, offset + 26)?,
                data_words,
                parent: None,
            }));
        }
        let mut nodes = Vec::with_capacity(count);
        let mut pending = vec![(root_offset, None)];
        while let Some((pointer, parent)) = pending.pop() {
            if pointer == 0 {
                continue;
            }
            let mut node = physical[node_index(pointer)?]
                .take()
                .context("cyclic or repeated model node")?;
            node.parent = parent;
            pending.push((node.next_offset, parent));
            pending.push((node.child_offset, Some(nodes.len())));
            nodes.push(node);
        }
        ensure!(
            nodes.len() == count,
            "model hierarchy has unreachable nodes"
        );
        let name = names(bytes, name_offset as usize, 1)?.and_then(|mut names| names.pop());
        let names = names(bytes, names_offset as usize, count)?;
        Ok(Self {
            field4: half(bytes, 4)?,
            field8: word(bytes, 8)?,
            root_offset,
            name_offset,
            root_geometry: half(bytes, 20)?,
            field16: half(bytes, 22)?,
            name_table_metadata,
            names_offset,
            name,
            names,
            nodes,
        })
    }
}

fn names(bytes: &[u8], offset: usize, count: usize) -> Result<Option<Vec<String>>> {
    if offset == 0 {
        return Ok(None);
    }
    let mut remaining = bytes.get(offset..).context("model names exceed resource")?;
    let names = (0..count)
        .map(|_| label(&mut remaining))
        .collect::<Result<_>>()?;
    Ok(Some(names))
}

fn label(remaining: &mut &[u8]) -> Result<String> {
    let bytes = crate::read::c_string(remaining, 0)?;
    let name = crate::read::label(bytes);
    *remaining = &remaining[bytes.len() + 1..];
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_root_and_labels_follow_construction_order_not_storage_order() -> Result<()> {
        let mut bytes = vec![0; HEADER_SIZE + 3 * NODE_SIZE];
        for (at, value) in [
            (0, 0x007b_7960u32),
            (12, 60),
            (24, 1),
            (28, 116),
            (48, 88),
            (68, 32),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[6..8].copy_from_slice(&3u16.to_be_bytes());
        for (at, id) in [(54, 1u16), (82, 0), (110, 2)] {
            bytes[at..at + 2].copy_from_slice(&id.to_be_bytes());
        }
        bytes.extend_from_slice(b"first\0\0\xb5\0");
        let model = Model::parse(&bytes)?;
        assert_eq!(
            model
                .nodes
                .iter()
                .map(|n| n.source_offset)
                .collect::<Vec<_>>(),
            [60, 32, 88]
        );
        assert_eq!(
            model.nodes.iter().map(|n| n.node_id).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(
            model.nodes.iter().map(|n| n.parent).collect::<Vec<_>>(),
            [None, None, Some(1)]
        );
        assert_eq!(model.names.as_ref().unwrap(), &["first", "", "ｵ"]);
        assert_eq!(model.name_table_metadata, 1);
        bytes[24..28].fill(0);
        assert!(Model::parse(&bytes).is_err());
        bytes[27] = 1;
        bytes[96..100].copy_from_slice(&60u32.to_be_bytes());
        assert!(Model::parse(&bytes).is_err());
        Ok(())
    }

    #[test]
    fn names_keep_empty_entries_and_reject_truncated_present_tables() -> Result<()> {
        assert_eq!(
            names(b"x\0\0tail\0", 1, 3)?,
            Some(vec!["".into(), "".into(), "tail".into()])
        );
        assert_eq!(names(b"unused", 0, 3)?, None);
        assert!(names(b"unused", 6, 3).is_err());
        assert!(names(b"x\0tail", 1, 2).is_err());
        assert!(names(b"x\0", 1, 2).is_err());
        Ok(())
    }

    #[test]
    fn names_and_transforms_can_alias() -> Result<()> {
        let mut bytes = vec![0; 160];
        for (at, value) in [
            (0, 0x007b_7960u32),
            (12, 32),
            (16, 148),
            (24, 1),
            (28, 148),
            (32, 96),
            (40, 60),
            (60, 96),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[6..8].copy_from_slice(&2u16.to_be_bytes());
        bytes[90] = 0xa5;
        bytes[96..148].fill(0x3f);
        bytes[148..154].copy_from_slice(b"\xb5\\\0ab\0");
        bytes[158] = 0x82;
        let model = Model::parse(&bytes)?;
        assert_eq!(model.name.as_deref(), Some(r"ｵ\\"));
        assert_eq!(model.names.as_ref().unwrap(), &[r"ｵ\\", "ab"]);
        assert_eq!(model.nodes[0].data_words, model.nodes[1].data_words);
        Ok(())
    }
}
