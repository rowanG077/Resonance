//! Complete formation records, shared by physical publication and encounter binding.
use crate::{cooked::Source, read::u16 as half};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(super) const RECORD_SIZE: usize = 96;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FormationTable {
    pub formations: Vec<Record>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub actor_count: u8,
    pub resource_count: u8,
    pub flags: u8,
    pub hidden_names: u8,
    /// Native archive lookup reads signed halfwords; inactive slots retain their values.
    pub resources: [i16; 4],
    pub actors: [Actor; 8],
    /// No meaning is established for the final eight bytes of the record.
    pub storage: [u8; 8],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Actor {
    pub resource: u8,
    pub appearance: u8,
    pub variant: u8,
    pub attachments: [u8; 2],
    pub position: [i16; 2],
}

impl FormationTable {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len().is_multiple_of(RECORD_SIZE),
            "misaligned formation table"
        );
        Ok(Self {
            formations: bytes
                .chunks_exact(RECORD_SIZE)
                .map(Record::read)
                .collect::<Result<_>>()?,
        })
    }

    pub fn bind(output: &Path, disc: u8, sources: &super::all::Sources) -> Result<Self> {
        let (_, bytes) =
            Source::open(output, disc, &sources.usual)?.resolve("battle/all/usual/1.json")?;
        let table: Self = serde_json::from_slice(&bytes)?;
        for row in &table.formations {
            row.validate()?;
        }
        Ok(table)
    }
}

impl Record {
    fn read(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() == RECORD_SIZE && bytes.starts_with(b"gp3\0"),
            "invalid formation row"
        );
        let mut resources = [0; 4];
        for (slot, resource) in resources.iter_mut().enumerate() {
            *resource = half(bytes, 8 + slot * 2)? as i16;
        }
        let mut actors = [Actor::default(); 8];
        for (slot, actor) in actors.iter_mut().enumerate() {
            *actor = Actor {
                resource: bytes[16 + slot],
                appearance: bytes[24 + slot],
                variant: bytes[32 + slot],
                attachments: [bytes[40 + slot], bytes[48 + slot]],
                position: [
                    half(bytes, 56 + slot * 4)? as i16,
                    half(bytes, 58 + slot * 4)? as i16,
                ],
            };
        }
        let row = Self {
            actor_count: bytes[4],
            resource_count: bytes[5],
            flags: bytes[6],
            hidden_names: bytes[7],
            resources,
            actors,
            storage: bytes[88..].try_into()?,
        };
        row.validate()?;
        Ok(row)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.actor_count <= 8 && self.resource_count <= 4,
            "invalid formation slot counts"
        );
        Ok(())
    }
}
