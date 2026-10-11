//! Authored character defaults and progression; gameplay chooses which records to activate.
#[cfg(test)]
use crate::embedded;
use crate::{dol, read::c_string};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
const FAMILY: &str = "character-catalogue";
const ADDRESS: u32 = 0x801f9fc8;
const COUNT: usize = 11;
const BYTES: usize = 280;
const NAME_BYTES: usize = 13;
const USES: usize = 40;
const EXPERIENCE: u32 = 0x80202958;
const LEVELS: usize = 252;
const GROWTH: u32 = 0x80202d48;
const GROWTH_COUNT: usize = 9;
const GROWTH_BYTES: usize = 14;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Growth {
    pub(crate) base: u8,
    pub(crate) random: u8,
}

crate::read::record! {
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub(crate) struct Definition {
        pub(crate) name: String => text.into_owned(),
        /// Bytes after the name terminator within the 13-byte editable name buffer.
        pub(crate) name_storage: Vec<u8> => row[name.len() + 1..NAME_BYTES].to_vec(),
        pub(crate) storage0d: u8 => row[0x0d],
        pub(crate) character_index: u8 => row[0x0e],
        pub(crate) title: u8 => row[0x0f],
        pub(crate) level: u8 => row[0x10],
        pub(crate) appearance_variant: u8 => row[0x11],
        pub(crate) hp: i16 => half(0x12),
        pub(crate) tp: i16 => half(0x14),
        pub(crate) storage16: [u8; 2] => row[0x16..0x18].try_into()?,
        pub(crate) experience: u32 => word(0x18),
        pub(crate) conditions: u32 => word(0x1c),
        pub(crate) title_mask: u32 => word(0x20),
        pub(crate) technique_drift: i8 => row[0x24] as i8,
        pub(crate) storage25: u8 => row[0x25],
        pub(crate) base_hp: i16 => half(0x26),
        pub(crate) base_tp: i16 => half(0x28),
        pub(crate) base_attack: i16 => half(0x2a),
        pub(crate) base_defense: i16 => half(0x2c),
        pub(crate) base_luck: i16 => half(0x2e),
        pub(crate) base_accuracy: i16 => half(0x30),
        pub(crate) base_evasion: i16 => half(0x32),
        pub(crate) base_intelligence: i16 => half(0x34),
        pub(crate) max_hp: i16 => half(0x36),
        pub(crate) max_tp: i16 => half(0x38),
        pub(crate) attack: i16 => half(0x3a),
        pub(crate) slash: i16 => half(0x3c),
        pub(crate) thrust: i16 => half(0x3e),
        pub(crate) defense: i16 => half(0x40),
        pub(crate) luck: i16 => half(0x42),
        pub(crate) accuracy: i16 => half(0x44),
        pub(crate) evasion: i16 => half(0x46),
        pub(crate) intelligence: i16 => half(0x48),
        /// Weapon, armor, head, shield, then two accessories.
        pub(crate) equipment: [i16; 6] => std::array::from_fn(|i| half(0x4a + i * 2)),
        pub(crate) overlimit: u8 => row[0x56],
        pub(crate) storage57: u8 => row[0x57],
        pub(crate) affinity: i32 => word(0x58) as i32,
        pub(crate) storage5c: [u8; 4] => row[0x5c..0x60].try_into()?,
        pub(crate) storage60: [u8; 8] => row[0x60..0x68].try_into()?,
        pub(crate) learning_history: u64 => mask(0x68),
        pub(crate) learned_techniques: u64 => mask(0x70),
        pub(crate) enabled_techniques: u64 => mask(0x78),
        pub(crate) available_techniques: u64 => mask(0x80),
        pub(crate) technique_uses: Vec<i16> => (0..USES).map(|i| half(0x88 + i * 2)).collect(),
        pub(crate) shortcuts: [i16; 4] => std::array::from_fn(|i| half(0xd8 + i * 2)),
        pub(crate) linked_shortcuts: [i16; 2] => [half(0xe0), half(0xe2)],
        pub(crate) linked_characters: [u8; 2] => row[0xe4..0xe6].try_into()?,
        pub(crate) strategy: [u8; 3] => row[0xe6..0xe9].try_into()?,
        pub(crate) storagee9: u8 => row[0xe9],
        pub(crate) ex_gems: [u8; 4] => row[0xea..0xee].try_into()?,
        pub(crate) ex_skills: [u8; 4] => row[0xee..0xf2].try_into()?,
        pub(crate) storagef2: [u8; 2] => row[0xf2..0xf4].try_into()?,
        pub(crate) cooking: [u8; 24] => row[0xf4..0x10c].try_into()?,
        pub(crate) technique_balance: i8 => row[0x10c] as i8,
        pub(crate) storage10d: [u8; 3] => row[0x10d..0x110].try_into()?,
        pub(crate) compound_ex_skills: u32 => word(0x110),
        pub(crate) recent_compound_ex_skills: u32 => word(0x114),
    } decode(row: &[u8]) {
        let row = row.get(..BYTES).context("truncated character definition")?;
        let name = c_string(&row[..NAME_BYTES], 0)?;
        let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(name);
        ensure!(!invalid, "invalid character name encoding");
        let (encoded, _, _) = encoding_rs::SHIFT_JIS.encode(&text);
        ensure!(
            encoded.as_ref() == name,
            "non-roundtrippable character name"
        );
        let half = |at| i16::from_be_bytes([row[at], row[at + 1]]);
        let word = |at| u32::from_be_bytes(row[at..at + 4].try_into().unwrap());
        let mask = |at| u64::from_be_bytes(row[at..at + 8].try_into().unwrap());
    }
}

impl Definition {
    fn validate(&self) -> Result<()> {
        let (name, _, invalid) = encoding_rs::SHIFT_JIS.encode(&self.name);
        ensure!(
            !invalid
                && !name.contains(&0)
                && name.len() + 1 + self.name_storage.len() == NAME_BYTES,
            "invalid character name storage"
        );
        ensure!(
            self.technique_uses.len() == USES,
            "incomplete technique use counters"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Catalogue {
    pub(crate) definitions: Vec<Definition>,
    pub(crate) experience: Vec<u32>,
    pub(crate) growth: Vec<[Growth; 7]>,
    pub(crate) growth_storage: [u8; 2],
}

impl Catalogue {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.definitions.len() == COUNT
                && self.experience.len() == LEVELS
                && self.growth.len() == GROWTH_COUNT,
            "incomplete character catalogue"
        );
        self.definitions.iter().try_for_each(Definition::validate)
    }
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let catalogue = Catalogue {
        definitions: dol::slice(executable, ADDRESS, COUNT * BYTES)?
            .chunks_exact(BYTES)
            .enumerate()
            .map(|(id, row)| Definition::decode(row).with_context(|| format!("character {id}")))
            .collect::<Result<_>>()?,
        experience: dol::slice(executable, EXPERIENCE, LEVELS * 4)?
            .chunks_exact(4)
            .map(|row| u32::from_be_bytes(row.try_into().unwrap()))
            .collect(),
        growth: dol::slice(executable, GROWTH, GROWTH_COUNT * GROWTH_BYTES)?
            .chunks_exact(GROWTH_BYTES)
            .map(|row| {
                std::array::from_fn(|i| Growth {
                    base: row[i * 2],
                    random: row[i * 2 + 1],
                })
            })
            .collect(),
        growth_storage: dol::slice(executable, GROWTH + (GROWTH_COUNT * GROWTH_BYTES) as u32, 2)?
            .try_into()?,
    };
    catalogue.validate()?;
    Ok(catalogue)
}

#[cfg(test)]
pub(crate) fn cooked(output: &Path) -> Result<Catalogue> {
    let catalogue: Catalogue = embedded::read(output, FAMILY, "main.dol")?;
    catalogue.validate()?;
    Ok(catalogue)
}

#[cfg(test)]
pub(crate) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    embedded::write(file, output, FAMILY, &read(executable)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    fn reconstruct(row: &Definition) -> Vec<u8> {
        let mut bytes = encoding_rs::SHIFT_JIS.encode(&row.name).0.into_owned();
        bytes.push(0);
        bytes.extend(&row.name_storage);
        bytes.extend([
            row.storage0d,
            row.character_index,
            row.title,
            row.level,
            row.appearance_variant,
        ]);
        bytes.extend(row.hp.to_be_bytes());
        bytes.extend(row.tp.to_be_bytes());
        bytes.extend(row.storage16);
        for value in [row.experience, row.conditions, row.title_mask] {
            bytes.extend(value.to_be_bytes());
        }
        bytes.extend([row.technique_drift as u8, row.storage25]);
        for value in [
            row.base_hp,
            row.base_tp,
            row.base_attack,
            row.base_defense,
            row.base_luck,
            row.base_accuracy,
            row.base_evasion,
            row.base_intelligence,
            row.max_hp,
            row.max_tp,
            row.attack,
            row.slash,
            row.thrust,
            row.defense,
            row.luck,
            row.accuracy,
            row.evasion,
            row.intelligence,
        ]
        .into_iter()
        .chain(row.equipment)
        {
            bytes.extend(value.to_be_bytes());
        }
        bytes.extend([row.overlimit, row.storage57]);
        bytes.extend(row.affinity.to_be_bytes());
        bytes.extend(row.storage5c);
        bytes.extend(row.storage60);
        for value in [
            row.learning_history,
            row.learned_techniques,
            row.enabled_techniques,
            row.available_techniques,
        ] {
            bytes.extend(value.to_be_bytes());
        }
        for value in row
            .technique_uses
            .iter()
            .chain(&row.shortcuts)
            .chain(&row.linked_shortcuts)
        {
            bytes.extend(value.to_be_bytes());
        }
        bytes.extend(row.linked_characters);
        bytes.extend(row.strategy);
        bytes.push(row.storagee9);
        bytes.extend(row.ex_gems);
        bytes.extend(row.ex_skills);
        bytes.extend(row.storagef2);
        bytes.extend(row.cooking);
        bytes.push(row.technique_balance as u8);
        bytes.extend(row.storage10d);
        bytes.extend(row.compound_ex_skills.to_be_bytes());
        bytes.extend(row.recent_compound_ex_skills.to_be_bytes());
        bytes
    }

    #[test]
    fn character_definition_preserves_signed_fields_and_storage() -> Result<()> {
        let mut raw: [u8; BYTES] = std::array::from_fn(|i| 0x80 | (i % 0x80) as u8);
        raw[..7].copy_from_slice(&[0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0]);
        let row = Definition::decode(&raw)?;
        row.validate()?;
        assert_eq!(row.name, "テスト");
        assert!(row.hp < 0 && row.base_luck < 0 && row.affinity < 0 && row.technique_drift < 0);
        assert!(row.technique_uses.iter().all(|&value| value < 0));
        assert_eq!(reconstruct(&row), raw);
        let catalogue = Catalogue {
            definitions: vec![row.clone(); COUNT],
            experience: (0..LEVELS).map(|i| 0x80000000 | i as u32).collect(),
            growth: vec![
                std::array::from_fn(|i| Growth {
                    base: 0x80 | i as u8,
                    random: 0xff - i as u8
                });
                GROWTH_COUNT
            ],
            growth_storage: [0x81, 0xff],
        };
        catalogue.validate()?;
        let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&catalogue)?)?;
        assert_eq!(restored, catalogue);
        assert!(Definition::decode(&raw[..BYTES - 1]).is_err());
        raw[..NAME_BYTES].fill(b'a');
        assert!(Definition::decode(&raw).is_err());
        raw[..2].copy_from_slice(&[0x81, 0]);
        assert!(Definition::decode(&raw).is_err());
        let mut malformed = row.clone();
        malformed.technique_uses.pop();
        assert!(malformed.validate().is_err());
        malformed = row;
        malformed.name_storage.pop();
        assert!(malformed.validate().is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; only publishes character JSON"]
    fn original_character_catalogue_reconstructs_and_publishes_both_discs() -> Result<()> {
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
                    "65064736999fbd8b6dc5d73f7f7c01757a97f3af0a8e19d6f77579c7a675153a"
                );
                let destination = output.join(format!("disc{disc}"));
                let paths = cook(&source, &executable, &destination)?;
                let catalogue = cooked(&destination)?;
                assert_eq!(catalogue, read(&executable)?);
                for (row, raw) in catalogue.definitions.iter().zip(raw.chunks_exact(BYTES)) {
                    assert_eq!(reconstruct(row), raw);
                }
                assert_eq!(catalogue.definitions[9].name, "ルーティ");
                assert_eq!(catalogue.definitions[9].learned_techniques, 3);
                assert_eq!(catalogue.definitions[10].name, "クレス");
                assert_eq!(catalogue.definitions[10].hp, 9999);
                let experience: Vec<_> = catalogue
                    .experience
                    .iter()
                    .flat_map(|value| value.to_be_bytes())
                    .collect();
                assert_eq!(experience, dol::slice(&executable, EXPERIENCE, LEVELS * 4)?);
                let mut growth: Vec<_> = catalogue
                    .growth
                    .iter()
                    .flatten()
                    .flat_map(|row| [row.base, row.random])
                    .collect();
                growth.extend(catalogue.growth_storage);
                assert_eq!(
                    growth,
                    dol::slice(&executable, GROWTH, GROWTH_COUNT * GROWTH_BYTES + 2)?
                );
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                assert_eq!(provenance["data"], paths[0]);
                payloads.insert(paths[0].clone());
                let mut incomplete = catalogue;
                incomplete.definitions.pop();
                embedded::write(&source, &destination, FAMILY, &incomplete)?;
                assert!(cooked(&destination).is_err());
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
