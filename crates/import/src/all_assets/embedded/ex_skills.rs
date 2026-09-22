//! Complete EX definitions, character choices and compound recipes.
use super::text::{TextPool, TextRef};
use crate::{
    dol,
    read::{Field, u16 as half, u32 as word},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
const DEFINITIONS: u32 = 0x80209544;
const DEFINITION_COUNT: usize = 166;
const CHOICES: u32 = 0x80208dd0;
const COMPOUNDS: u32 = 0x80208e60;
const PERSONAL_SKILLS: u32 = 0x8018d1f0;
const LABELS: u32 = 0x801ab080;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct StatBonus {
    pub(crate) selector: u8,
    pub(crate) percent: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Definition {
    pub(crate) id: u32,
    pub(crate) name: Option<TextRef>,
    pub(crate) description: Option<TextRef>,
    /// The stat reader visits both slots, including inactive/unknown selectors.
    pub(crate) stat_bonuses: [StatBonus; 2],
    pub(crate) tendency: i16,
    /// Authored byte between tendency and activation; meaning unresolved.
    pub(crate) storage: u8,
    pub(crate) activation: u8,
}

crate::read::record! {
    #[derive(Debug, PartialEq)]
    pub(crate) struct Compound(8) {
        pub(crate) skill: u16 => 0,
        pub(crate) requirement_count: u16 => 2,
        /// Only the count-selected prefix is consumed; keep all four source slots.
        pub(crate) requirements: [u8; 4] => 4,
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Character {
    pub(crate) levels: [[u8; 4]; 4],
    pub(crate) compound_count: u32,
    pub(crate) compounds: [Compound; 24],
}

#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum Label {
    Title,
    Hp,
    Tp,
    Slash,
    Thrust,
    Defense,
    Accuracy,
    Evasion,
    Intelligence,
    Luck,
    Attack,
    Constant,
    Chance,
    BattleEnd,
    Other,
    SetGem,
    ReplaceGem,
    Yes,
    No,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Formats {
    pub(crate) strike_type: TextRef,
    pub(crate) technical_type: TextRef,
    pub(crate) gem_max: TextRef,
    pub(crate) gem_level: TextRef,
    pub(crate) gem_empty: TextRef,
    pub(crate) stat_arrow: TextRef,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavePointRule {
    /// Zero-based character index, from a signed compare-immediate operand.
    pub(crate) character_index: i16,
    pub(crate) tp_cost: i16,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) definitions: Vec<Definition>,
    pub(crate) characters: [Character; 9],
    pub(crate) personal_skills: [u16; 9],
    pub(crate) labels: [Option<TextRef>; 19],
    pub(crate) formats: Formats,
    pub(crate) save_point_rule: SavePointRule,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }

    pub(crate) fn required_text(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required EX text")?))
    }

    pub(crate) fn label(&self, label: Label) -> Result<&str> {
        self.required_text(self.labels[label as usize])
    }
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let mut texts = TextPool::default();
    let rows = dol::slice(executable, DEFINITIONS, DEFINITION_COUNT * 20)?;
    let definitions = rows
        .chunks_exact(20)
        .map(|row| {
            Ok(Definition {
                id: word(row, 0)?,
                name: texts.reference(executable, word(row, 4)?)?,
                description: texts.reference(executable, word(row, 8)?)?,
                stat_bonuses: std::array::from_fn(|i| StatBonus {
                    selector: row[12 + i * 2],
                    percent: row[13 + i * 2],
                }),
                tendency: half(row, 16)? as i16,
                storage: row[18],
                activation: row[19],
            })
        })
        .collect::<Result<_>>()?;
    let choices = dol::slice(executable, CHOICES, 9 * 16)?;
    let characters: Vec<_> = dol::slice(executable, COMPOUNDS, 9 * 196)?
        .chunks_exact(196)
        .zip(choices.chunks_exact(16))
        .map(|(row, choices)| {
            Ok(Character {
                levels: Field::read(choices, 0)?,
                compound_count: word(row, 0)?,
                compounds: Field::read(row, 4)?,
            })
        })
        .collect::<Result<_>>()?;
    let personal = dol::slice(executable, PERSONAL_SKILLS, 9 * 2)?;
    let labels = texts.array(executable, LABELS)?;
    let formats = Formats {
        strike_type: texts.fixed(executable, 0x8035d648, 8)?,
        technical_type: texts.fixed(executable, 0x8035d650, 8)?,
        gem_max: texts.fixed(executable, 0x8035d658, 4)?,
        gem_level: texts.fixed(executable, 0x8035d65c, 8)?,
        gem_empty: texts.fixed(executable, 0x8035d664, 4)?,
        stat_arrow: texts.fixed(executable, 0x8035d668, 8)?,
    };
    let immediate = |address, opcode| -> Result<i16> {
        let instruction = word(dol::slice(executable, address, 4)?, 0)?;
        ensure!(
            instruction & 0xffff_0000 == opcode,
            "unsupported save-point TP rule instruction"
        );
        Ok(instruction as i16)
    };
    Ok(Catalogue {
        texts: texts.values,
        definitions,
        characters: characters.try_into().unwrap(),
        personal_skills: Field::read(personal, 0)?,
        labels,
        formats,
        save_point_rule: SavePointRule {
            character_index: immediate(0x800cf788, 0x2c00_0000)?,
            tp_cost: immediate(0x800cf7a0, 0x3860_0000)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs, path::Path};

    const FAMILY: &str = "ex-skills";

    #[test]
    #[ignore = "requires both extracted discs; publishes only EX skill JSON"]
    fn original_ex_catalogue_preserves_definitions_recipes_and_rules() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let destination = output.join(format!("disc{disc}"));
                let catalogue = read(&executable)?;
                let paths = crate::embedded::write(&file, &destination, FAMILY, &catalogue)?;
                let restored: Catalogue = crate::embedded::read(&destination, FAMILY, "main.dol")?;
                assert_eq!(restored, catalogue);
                payloads.insert(paths[0].clone());
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                assert_eq!(restored.definitions.len(), DEFINITION_COUNT);
                assert_eq!(
                    restored.required_text(restored.definitions[1].name)?,
                    "Strong"
                );
                assert!(
                    restored
                        .required_text(restored.definitions[3].description)?
                        .contains('\u{b}')
                );
                assert_eq!(
                    restored.personal_skills,
                    [8, 18, 24, 31, 34, 40, 44, 50, 53]
                );
                assert_eq!(
                    restored.characters[0].compounds[0].requirements,
                    [1, 2, 0, 0]
                );

                let name = word(dol::slice(&executable, DEFINITIONS + 24, 4)?, 0)?;
                let label = word(dol::slice(&executable, LABELS + 4, 4)?, 0)?;
                let inactive = DEFINITIONS + 17 * 20;
                for (address, replacement) in [
                    (inactive, 0xdeadbeefu32.to_be_bytes().to_vec()),
                    (inactive + 4, 0u32.to_be_bytes().to_vec()),
                    (inactive + 8, name.to_be_bytes().to_vec()),
                    (inactive + 12, vec![255, 128, 0, 99, 0x80, 0, 0xa5, 255]),
                    (CHOICES, vec![255]),
                    (COMPOUNDS, 25u32.to_be_bytes().to_vec()),
                    (COMPOUNDS + 4, vec![1, 255, 255, 255, 1, 2, 0x81, 0xff]),
                    (LABELS, 0u32.to_be_bytes().to_vec()),
                    (LABELS + 8, label.to_be_bytes().to_vec()),
                    (0x800cf788, 0x2c00fff9u32.to_be_bytes().to_vec()),
                    (0x800cf7a0, 0x3860fffeu32.to_be_bytes().to_vec()),
                ] {
                    let source = dol::slice(&executable, address, replacement.len())?;
                    let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                    executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
                }
                let changed = read(&executable)?;
                let row = &changed.definitions[17];
                assert_eq!(row.id, 0xdeadbeef);
                assert!(row.name.is_none());
                assert_eq!(row.description, changed.definitions[1].name);
                assert_eq!(
                    row.stat_bonuses,
                    [
                        StatBonus {
                            selector: 255,
                            percent: 128
                        },
                        StatBonus {
                            selector: 0,
                            percent: 99
                        }
                    ]
                );
                assert_eq!(
                    (row.tendency, row.storage, row.activation),
                    (i16::MIN, 0xa5, 255)
                );
                assert_eq!(changed.characters[0].compounds.len(), 24);
                assert_eq!(changed.characters[0].compound_count, 25);
                assert_eq!(
                    changed.characters[0].compounds[0].requirement_count,
                    u16::MAX
                );
                assert!(changed.labels[0].is_none());
                assert_eq!(changed.labels[1], changed.labels[2]);
                assert_eq!(changed.save_point_rule.character_index, -7);
                assert_eq!(changed.save_point_rule.tp_cost, -2);
            }
            assert_eq!(
                payloads.len(),
                1,
                "identical EX data duplicated across discs"
            );
            Ok(())
        })();
        let _ = fs::remove_dir_all(&output);
        result
    }
}
