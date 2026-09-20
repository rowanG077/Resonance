//! Complete authored item records; gameplay bindings interpret their flags and references.
use crate::{dol, embedded, read::u32 as word};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "item-catalogue";
const ADDRESS: u32 = 0x801fad98;
const COUNT: usize = 528;
const BYTES: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EffectSlot {
    pub(crate) effect: u8,
    pub(crate) storage: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Definition {
    pub(crate) name: Option<String>,
    pub(crate) price: i32,
    pub(crate) slash: i16,
    pub(crate) thrust: i16,
    pub(crate) defense: i16,
    pub(crate) intelligence: i8,
    pub(crate) accuracy: i8,
    pub(crate) evasion: i8,
    pub(crate) luck: i8,
    pub(crate) critical_chance_bonus: u8,
    pub(crate) attack_element: u8,
    pub(crate) primary_effect: u8,
    pub(crate) equipment_owner_mask: u8,
    pub(crate) storage16: u8,
    pub(crate) usage_flags: u8,
    pub(crate) transforms_to: u16,
    pub(crate) category: u8,
    /// Battle aggregates ten slots but converts only the first nine to resistance categories.
    pub(crate) resistance_modifiers: [i8; 10],
    pub(crate) storage25: u8,
    pub(crate) effects: [EffectSlot; 4],
    pub(crate) storage2e: [u8; 3],
    pub(crate) technique_drift: i8,
    pub(crate) storage32: [u8; 2],
    pub(crate) description: Option<String>,
    pub(crate) details: Option<String>,
}

impl Definition {
    pub(crate) fn decode(executable: &[u8], row: &[u8]) -> Result<Self> {
        let row = row.get(..BYTES).context("truncated item definition")?;
        let half = |at| i16::from_be_bytes([row[at], row[at + 1]]);
        let text = |at| dol::optional_text(executable, word(row, at)?);
        Ok(Self {
            name: text(0)?,
            price: word(row, 4)? as i32,
            slash: half(8),
            thrust: half(10),
            defense: half(12),
            intelligence: row[14] as i8,
            accuracy: row[15] as i8,
            evasion: row[16] as i8,
            luck: row[17] as i8,
            critical_chance_bonus: row[0x12],
            attack_element: row[0x13],
            primary_effect: row[0x14],
            equipment_owner_mask: row[0x15],
            storage16: row[0x16],
            usage_flags: row[0x17],
            transforms_to: half(0x18) as u16,
            category: row[0x1a],
            resistance_modifiers: std::array::from_fn(|i| row[0x1b + i] as i8),
            storage25: row[0x25],
            effects: std::array::from_fn(|i| EffectSlot {
                effect: row[0x26 + i * 2],
                storage: row[0x27 + i * 2],
            }),
            storage2e: row[0x2e..0x31].try_into()?,
            technique_drift: row[0x31] as i8,
            storage32: row[0x32..0x34].try_into()?,
            description: text(0x34)?,
            details: text(0x38)?,
        })
    }
}

pub(crate) fn read(executable: &[u8]) -> Result<Vec<Definition>> {
    dol::slice(executable, ADDRESS, COUNT * BYTES)?
        .chunks_exact(BYTES)
        .enumerate()
        .map(|(id, row)| Definition::decode(executable, row).with_context(|| format!("item {id}")))
        .collect()
}

pub(crate) fn cooked(output: &Path) -> Result<Vec<Definition>> {
    let items: Vec<Definition> = embedded::read(output, FAMILY, "main.dol")?;
    ensure!(items.len() == COUNT, "incomplete item catalogue");
    Ok(items)
}

pub(crate) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    embedded::write(
        file,
        output,
        FAMILY,
        &read(executable)?,
        serde_json::json!({"address":ADDRESS,"count":COUNT,"stride":BYTES}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    /// Rebuild every scalar; decoded strings are checked separately against their source pointers.
    fn reconstruct(item: &Definition, source: &[u8]) -> Vec<u8> {
        let mut bytes = source[..4].to_vec();
        bytes.extend(item.price.to_be_bytes());
        for value in [item.slash, item.thrust, item.defense] {
            bytes.extend(value.to_be_bytes());
        }
        bytes.extend([
            item.intelligence as u8,
            item.accuracy as u8,
            item.evasion as u8,
            item.luck as u8,
            item.critical_chance_bonus,
            item.attack_element,
            item.primary_effect,
            item.equipment_owner_mask,
            item.storage16,
            item.usage_flags,
        ]);
        bytes.extend(item.transforms_to.to_be_bytes());
        bytes.push(item.category);
        bytes.extend(item.resistance_modifiers.map(|value| value as u8));
        bytes.push(item.storage25);
        for slot in &item.effects {
            bytes.extend([slot.effect, slot.storage]);
        }
        bytes.extend(item.storage2e);
        bytes.push(item.technique_drift as u8);
        bytes.extend(item.storage32);
        bytes.extend(&source[0x34..0x3c]);
        bytes
    }

    #[test]
    fn item_definition_preserves_signed_storage_and_nullable_text() -> Result<()> {
        let mut executable = vec![0; 0x108];
        executable[..4].copy_from_slice(&0x100_u32.to_be_bytes());
        executable[0x48..0x4c].copy_from_slice(&0x80000000_u32.to_be_bytes());
        executable[0x90..0x94].copy_from_slice(&8_u32.to_be_bytes());
        executable[0x100..].copy_from_slice(&[0, 0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0]);
        let mut row: [u8; BYTES] = std::array::from_fn(|i| i as u8 ^ 0x9b);
        row[..4].copy_from_slice(&0x80000000_u32.to_be_bytes());
        row[0x34..0x38].fill(0);
        row[0x38..].copy_from_slice(&0x80000001_u32.to_be_bytes());
        let item = Definition::decode(&executable, &row)?;
        assert_eq!(item.name.as_deref(), Some(""));
        assert_eq!(item.description, None);
        assert_eq!(item.details.as_deref(), Some("テスト"));
        assert!(item.price < 0 && item.slash < 0 && item.luck < 0 && item.technique_drift < 0);
        assert!(item.resistance_modifiers.iter().all(|&value| value < 0));
        assert_eq!(reconstruct(&item, &row), row);
        let restored: Definition = serde_json::from_slice(&serde_json::to_vec(&item)?)?;
        assert_eq!(restored, item);
        assert!(Definition::decode(&executable, &row[..BYTES - 1]).is_err());
        row[0x38..].copy_from_slice(&0x80000008_u32.to_be_bytes());
        assert!(Definition::decode(&executable, &row).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; only publishes item JSON"]
    fn original_item_catalogue_reconstructs_and_publishes_both_discs() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let source = extracted.join(format!("disc{disc}/sys/main.dol"));
                let executable = fs::read(&source)?;
                let raw = dol::slice(&executable, ADDRESS, COUNT * BYTES)?;
                assert_eq!(
                    crate::digest(raw),
                    "3bbaad3ac14e9d6b0272e4d745f6ff957155146fc1bf019b09048cf18b6c5b6d"
                );
                let destination = output.join(format!("disc{disc}"));
                let paths = cook(&source, &executable, &destination)?;
                let items = cooked(&destination)?;
                assert_eq!(items, read(&executable)?);
                for (item, row) in items.iter().zip(raw.chunks_exact(BYTES)) {
                    assert_eq!(reconstruct(item, row), row);
                    assert_eq!(item.critical_chance_bonus, row[0x12]);
                    assert_eq!(
                        item.resistance_modifiers,
                        std::array::from_fn(|i| row[0x1b + i] as i8)
                    );
                    for (at, text) in [
                        (0, &item.name),
                        (0x34, &item.description),
                        (0x38, &item.details),
                    ] {
                        let pointer = word(row, at)?;
                        if let Some(text) = text {
                            assert_ne!(pointer, 0);
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
                        } else {
                            assert_eq!(pointer, 0);
                        }
                    }
                }
                let drift: Vec<_> = items
                    .iter()
                    .enumerate()
                    .filter_map(|(id, item)| {
                        (item.technique_drift != 0).then_some((id, item.technique_drift))
                    })
                    .collect();
                assert_eq!(
                    drift,
                    [
                        (445, -1),
                        (446, 1),
                        (447, -1),
                        (448, 1),
                        (449, -1),
                        (451, -1),
                        (452, 1),
                        (453, 1),
                        (476, 2),
                        (477, -2)
                    ]
                );
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                assert_eq!(provenance["address"], ADDRESS);
                assert_eq!(provenance["count"], COUNT);
                assert_eq!(provenance["stride"], BYTES);
                assert_eq!(provenance["data"], paths[0]);
                payloads.insert(paths[0].clone());
                embedded::write(
                    &source,
                    &destination,
                    FAMILY,
                    &&items[..COUNT - 1],
                    serde_json::json!({}),
                )?;
                assert!(
                    cooked(&destination).is_err(),
                    "incomplete catalogue accepted"
                );
            }
            assert_eq!(payloads.len(), 1);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
