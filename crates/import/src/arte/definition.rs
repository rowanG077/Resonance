//! Complete authored arte records; gameplay bindings interpret their flags and references.
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const ADDRESS: u32 = 0x80202f90;
const COUNT: usize = 253;
const BYTES: usize = 88;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Definition {
    pub native_id: i16,
    pub storage02: u16,
    pub auxiliary_text: Option<String>,
    pub tp_cost: u8,
    pub storage09: [u8; 3],
    pub description: Option<String>,
    pub name: Option<String>,
    pub menu_category: u8,
    pub element: u8,
    pub target_preference: u8,
    pub learning_route: u8,
    pub learning_parent: i16,
    pub technical_successor: i16,
    pub strike_successor: i16,
    pub mutually_exclusive: [i16; 4],
    /// A first entry of -1 selects requirements on either successor of each remaining family.
    pub required_learned: [i16; 4],
    pub forbidden_learned: [i16; 2],
    pub storage32: u16,
    pub flags: u32,
    pub cast_time_adjustment: i16,
    pub recovery_ticks: i16,
    pub required_uses: u16,
    pub required_level: u16,
    pub target_condition_mask: u64,
    pub storage48: [u8; 3],
    pub unison_altitude: u8,
    pub unison_distance: i16,
    pub unison_duration: i16,
    pub skill_archive_index: u32,
    pub storage54: u32,
}

impl Definition {
    pub(crate) fn decode(executable: &[u8], row: &[u8]) -> Result<Self> {
        let row = row.get(..BYTES).context("truncated arte definition")?;
        let signed = |at| i16::from_be_bytes([row[at], row[at + 1]]);
        let text = |at| dol::optional_text(executable, word(row, at)?);
        Ok(Self {
            native_id: signed(0),
            storage02: signed(2) as u16,
            auxiliary_text: text(4)?,
            tp_cost: row[8],
            storage09: row[9..12].try_into()?,
            description: text(12)?,
            name: text(16)?,
            menu_category: row[0x14],
            element: row[0x15],
            target_preference: row[0x16],
            learning_route: row[0x17],
            learning_parent: signed(0x18),
            technical_successor: signed(0x1a),
            strike_successor: signed(0x1c),
            mutually_exclusive: std::array::from_fn(|i| signed(0x1e + i * 2)),
            required_learned: std::array::from_fn(|i| signed(0x26 + i * 2)),
            forbidden_learned: std::array::from_fn(|i| signed(0x2e + i * 2)),
            storage32: signed(0x32) as u16,
            flags: word(row, 0x34)?,
            cast_time_adjustment: signed(0x38),
            recovery_ticks: signed(0x3a),
            required_uses: signed(0x3c) as u16,
            required_level: signed(0x3e) as u16,
            target_condition_mask: u64::from(word(row, 0x40)?) << 32 | u64::from(word(row, 0x44)?),
            storage48: row[0x48..0x4b].try_into()?,
            unison_altitude: row[0x4b],
            unison_distance: signed(0x4c),
            unison_duration: signed(0x4e),
            skill_archive_index: word(row, 0x50)?,
            storage54: word(row, 0x54)?,
        })
    }

    /// Approach logic reads the same two timing halfwords as one signed word.
    pub(crate) fn approach_range(&self) -> i32 {
        (i32::from(self.cast_time_adjustment) << 16) | i32::from(self.recovery_ticks as u16)
    }
}

pub(crate) fn definitions(executable: &[u8]) -> Result<Vec<Definition>> {
    dol::slice(executable, ADDRESS, COUNT * BYTES)?
        .chunks_exact(BYTES)
        .enumerate()
        .map(|(index, row)| {
            Definition::decode(executable, row).with_context(|| format!("arte {index}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    /// Reconstruct physical scalars in order; text pointers are provenance, not cooked data.
    fn reconstruct(value: &Definition, source: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(BYTES);
        bytes.extend(value.native_id.to_be_bytes());
        bytes.extend(value.storage02.to_be_bytes());
        bytes.extend(&source[4..8]);
        bytes.push(value.tp_cost);
        bytes.extend(value.storage09);
        bytes.extend(&source[12..20]);
        bytes.extend([
            value.menu_category,
            value.element,
            value.target_preference,
            value.learning_route,
        ]);
        for word in [
            value.learning_parent,
            value.technical_successor,
            value.strike_successor,
        ]
        .into_iter()
        .chain(value.mutually_exclusive)
        .chain(value.required_learned)
        .chain(value.forbidden_learned)
        {
            bytes.extend(word.to_be_bytes());
        }
        bytes.extend(value.storage32.to_be_bytes());
        bytes.extend(value.flags.to_be_bytes());
        bytes.extend(value.cast_time_adjustment.to_be_bytes());
        bytes.extend(value.recovery_ticks.to_be_bytes());
        bytes.extend(value.required_uses.to_be_bytes());
        bytes.extend(value.required_level.to_be_bytes());
        bytes.extend(value.target_condition_mask.to_be_bytes());
        bytes.extend(value.storage48);
        bytes.push(value.unison_altitude);
        bytes.extend(value.unison_distance.to_be_bytes());
        bytes.extend(value.unison_duration.to_be_bytes());
        bytes.extend(value.skill_archive_index.to_be_bytes());
        bytes.extend(value.storage54.to_be_bytes());
        bytes
    }

    #[test]
    fn arte_definition_retains_every_scalar_and_nullable_text() -> Result<()> {
        let mut executable = vec![0; 0x108];
        executable[..4].copy_from_slice(&0x100_u32.to_be_bytes());
        executable[0x48..0x4c].copy_from_slice(&0x80000000_u32.to_be_bytes());
        executable[0x90..0x94].copy_from_slice(&8_u32.to_be_bytes());
        executable[0x100..].copy_from_slice(&[0, 0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0]);
        let mut row: [u8; BYTES] = std::array::from_fn(|i| i as u8 ^ 0x9b);
        row[4..8].copy_from_slice(&0x80000000_u32.to_be_bytes());
        row[12..16].fill(0);
        row[16..20].copy_from_slice(&0x80000001_u32.to_be_bytes());
        let value = Definition::decode(&executable, &row)?;
        assert_eq!(value.auxiliary_text.as_deref(), Some(""));
        assert_eq!(value.description, None);
        assert_eq!(value.name.as_deref(), Some("テスト"));
        assert_eq!(reconstruct(&value, &row), row);
        assert_eq!(
            value.approach_range(),
            i32::from_be_bytes(row[0x38..0x3c].try_into()?)
        );
        let restored: Definition = serde_json::from_slice(&serde_json::to_vec(&value)?)?;
        assert_eq!(
            serde_json::to_value(restored)?,
            serde_json::to_value(value)?
        );
        assert!(Definition::decode(&executable, &row[..BYTES - 1]).is_err());
        row[16..20].copy_from_slice(&0x80000008_u32.to_be_bytes());
        assert!(Definition::decode(&executable, &row).is_err());
        row[16..20].copy_from_slice(&0x80000001_u32.to_be_bytes());
        executable[0x107] = 0x83;
        assert!(
            Definition::decode(&executable, &row).is_err(),
            "unterminated text"
        );
        executable[0x102] = 0;
        assert!(
            Definition::decode(&executable, &row).is_err(),
            "incomplete Shift-JIS character"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; reads only the arte catalogue"]
    fn original_arte_definitions_reconstruct_every_record_on_both_discs() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut previous = None;
        for disc in ["disc1", "disc2"] {
            let executable = fs::read(extracted.join(disc).join("sys/main.dol"))?;
            let records = definitions(&executable)?;
            assert_eq!(records.len(), COUNT);
            for (record, row) in records
                .iter()
                .zip(dol::slice(&executable, ADDRESS, COUNT * BYTES)?.chunks_exact(BYTES))
            {
                assert_eq!(reconstruct(record, row), row);
                assert_eq!(record.approach_range(), word(row, 0x38)? as i32);
                for (offset, text) in [
                    (4, &record.auxiliary_text),
                    (12, &record.description),
                    (16, &record.name),
                ] {
                    let pointer = word(row, offset)?;
                    assert_ne!(pointer, 0);
                    let text = text.as_ref().context("missing original arte text")?;
                    let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(text);
                    assert!(!invalid);
                    assert_eq!(
                        encoded.as_ref(),
                        dol::slice(&executable, pointer, encoded.len())?
                    );
                    assert_eq!(
                        dol::slice(&executable, pointer + encoded.len() as u32, 1)?,
                        [0]
                    );
                }
            }
            assert_eq!(records[1].name.as_deref(), Some("Demon Fang"));
            assert!(
                records
                    .iter()
                    .any(|record| record.required_learned[0] == -1)
            );
            let value = serde_json::to_value(records)?;
            if let Some(previous) = previous {
                assert_eq!(value, previous);
            }
            previous = Some(value);
        }
        Ok(())
    }
}
