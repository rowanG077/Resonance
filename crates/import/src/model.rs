//! Lossless parser for relocatable field models.
//!
//! A 12-byte wrapper stores the relative blob offset in word two and its size
//! in word three. Blob fields are big-endian; pointers are relative offsets.
//!
//! ```text
//! BlobHeader { magic, u16 field4, u16 node_count, field8,
//!              node_offset, optional_offset, field14,
//!              field18, optional_data_offset }
//! Node { data, field04, next, field0c, child, object_index,
//!        field16, field18, field19, field1a }
//! ```
//!
//! Retain unknown words and payloads verbatim; named fields can be edited
//! without discarding information needed by other consumers.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use thiserror::Error;

const BLOB_HEADER_SIZE: usize = 0x20;
const NODE_SIZE: usize = 0x1C;
const NODE_DATA_WORDS: usize = 13;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("model blob is shorter than its 0x20-byte header")]
    BlobShort,
    #[error("model blob has unexpected magic 0x{0:08X}")]
    Magic(u32),
    #[error("model node table 0x{offset:X}..0x{end:X} exceeds blob size 0x{size:X}")]
    NodeBounds {
        offset: usize,
        end: usize,
        size: usize,
    },
    #[error("model node {node} pointer field {field} points outside blob: 0x{offset:X}")]
    NodePointer {
        node: usize,
        field: &'static str,
        offset: u32,
    },
    #[error("model node {node} data block at 0x{offset:X} is shorter than 13 words")]
    NodeData { node: usize, offset: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelNodeJson {
    pub data_offset: u32,
    pub field04: u32,
    pub next_offset: u32,
    pub field0c: u32,
    pub child_offset: u32,
    pub object_index: u16,
    pub field16: u16,
    pub field18: u8,
    pub field19: u8,
    pub field1a: u16,
    pub data_words: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelBlobJson {
    pub source_offset: usize,
    pub size: usize,
    pub magic: u32,
    pub field4: u16,
    pub node_count: u16,
    pub field8: u32,
    pub node_offset: u32,
    pub optional_offset: u32,
    pub field14: u32,
    pub field18: u32,
    pub optional_data_offset: u32,
    pub nodes: Vec<ModelNodeJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional_data_text: Option<String>,
    pub raw_hex: String,
}

impl ModelBlobJson {
    /// Parse one copied model blob.  `source_offset` is metadata only and is
    /// retained in the JSON for container integration.
    ///
    /// # Errors
    ///
    /// Returns an error if any proven pointer or fixed-size table is outside
    /// the copied blob.
    ///
    /// # Panics
    ///
    /// This function does not panic for malformed input; indexed reads follow
    /// checked range calculations.
    #[allow(clippy::similar_names)]
    pub fn parse(data: &[u8], source_offset: usize) -> Result<Self, ModelError> {
        if data.len() < BLOB_HEADER_SIZE {
            return Err(ModelError::BlobShort);
        }
        let magic = be_u32(data, 0).unwrap();
        if magic != 0x007B_7960_u32 {
            return Err(ModelError::Magic(magic));
        }
        let field4 = be_u16(data, 4).unwrap();
        let node_count = be_u16(data, 6).unwrap();
        let field8 = be_u32(data, 8).unwrap();
        let node_offset = be_u32(data, 12).unwrap();
        let optional_offset = be_u32(data, 16).unwrap();
        let field14 = be_u32(data, 20).unwrap();
        let field18 = be_u32(data, 24).unwrap();
        let optional_data_offset = be_u32(data, 28).unwrap();
        let node_start = node_offset as usize;
        let node_end = node_start
            .checked_add(usize::from(node_count) * NODE_SIZE)
            .ok_or(ModelError::NodeBounds {
                offset: node_start,
                end: usize::MAX,
                size: data.len(),
            })?;
        if node_start < BLOB_HEADER_SIZE || node_end > data.len() {
            return Err(ModelError::NodeBounds {
                offset: node_start,
                end: node_end,
                size: data.len(),
            });
        }
        let mut nodes = Vec::with_capacity(usize::from(node_count));
        for node_index in 0..usize::from(node_count) {
            let offset = node_start + node_index * NODE_SIZE;
            let node = ModelNodeJson {
                data_offset: be_u32(data, offset).unwrap(),
                field04: be_u32(data, offset + 4).unwrap(),
                next_offset: be_u32(data, offset + 8).unwrap(),
                field0c: be_u32(data, offset + 12).unwrap(),
                child_offset: be_u32(data, offset + 16).unwrap(),
                object_index: be_u16(data, offset + 20).unwrap(),
                field16: be_u16(data, offset + 22).unwrap(),
                field18: data[offset + 24],
                field19: data[offset + 25],
                field1a: be_u16(data, offset + 26).unwrap(),
                data_words: Vec::new(),
            };
            for (field, pointer) in [
                ("data", node.data_offset),
                ("field04", node.field04),
                ("next", node.next_offset),
                ("field0c", node.field0c),
                ("child", node.child_offset),
            ] {
                if pointer != 0 && pointer as usize >= data.len() {
                    return Err(ModelError::NodePointer {
                        node: node_index,
                        field,
                        offset: pointer,
                    });
                }
            }
            let mut node = node;
            if node.data_offset != 0 {
                let start = node.data_offset as usize;
                let end = start
                    .checked_add(NODE_DATA_WORDS * 4)
                    .ok_or(ModelError::NodeData {
                        node: node_index,
                        offset: node.data_offset,
                    })?;
                if end > data.len() {
                    return Err(ModelError::NodeData {
                        node: node_index,
                        offset: node.data_offset,
                    });
                }
                node.data_words = (0..NODE_DATA_WORDS)
                    .map(|i| be_u32(data, start + i * 4).unwrap())
                    .collect();
            }
            nodes.push(node);
        }
        Ok(Self {
            source_offset,
            size: data.len(),
            magic,
            field4,
            node_count,
            field8,
            node_offset,
            optional_offset,
            field14,
            field18,
            optional_data_offset,
            nodes,
            optional_text: read_c_string(data, optional_offset as usize),
            optional_data_text: read_c_string(data, optional_data_offset as usize),
            raw_hex: encode_hex(data),
        })
    }
}

fn read_c_string(data: &[u8], offset: usize) -> Option<String> {
    if offset == 0 || offset >= data.len() {
        return None;
    }
    let end = data[offset..]
        .iter()
        .position(|byte| *byte == 0)
        .map_or(data.len(), |n| offset + n);
    std::str::from_utf8(&data[offset..end])
        .ok()
        .map(ToOwned::to_owned)
}
fn be_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)
        .map(|v| u16::from_be_bytes(v.try_into().unwrap()))
}
fn be_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|v| u32::from_be_bytes(v.try_into().unwrap()))
}
#[must_use]
pub fn encode_hex(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
