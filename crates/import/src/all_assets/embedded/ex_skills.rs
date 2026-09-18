//! Complete EX definitions, character choices and compound recipes.
use super::text::{FixedText, TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{Field, u16 as half, u32 as word},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
const FAMILY: &str = "ex-skills";
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
    pub(crate) strike_type: FixedText,
    pub(crate) technical_type: FixedText,
    pub(crate) gem_max: FixedText,
    pub(crate) gem_level: FixedText,
    pub(crate) gem_empty: FixedText,
    pub(crate) stat_arrow: FixedText,
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
    pub(crate) definition_storage: u32,
    pub(crate) characters: [Character; 9],
    pub(crate) personal_skills: [u16; 9],
    pub(crate) personal_skill_storage: [u8; 6],
    pub(crate) labels: [Option<TextRef>; 19],
    /// Final word of the UI table has no established text consumer.
    pub(crate) label_storage: u32,
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

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let mut texts = TextPool::default();
    let rows = dol::slice(executable, DEFINITIONS, DEFINITION_COUNT * 20 + 4)?;
    let definitions = rows[..DEFINITION_COUNT * 20]
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
    let personal = dol::slice(executable, PERSONAL_SKILLS, 24)?;
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
    Ok((
        Catalogue {
            texts: texts.values,
            definitions,
            definition_storage: word(rows, DEFINITION_COUNT * 20)?,
            characters: characters.try_into().unwrap(),
            personal_skills: Field::read(personal, 0)?,
            personal_skill_storage: personal[18..24].try_into()?,
            labels,
            label_storage: word(dol::slice(executable, LABELS + 76, 4)?, 0)?,
            formats,
            save_point_rule: SavePointRule {
                character_index: immediate(0x800cf788, 0x2c00_0000)?,
                tp_cost: immediate(0x800cf7a0, 0x3860_0000)?,
            },
        },
        texts.sources,
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

#[cfg(test)]
pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, _) = parse(executable)?;
    crate::embedded::write(file, output, FAMILY, &catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    fn reconstruct(catalogue: &Catalogue, sources: &[TextSource]) -> Result<Vec<(u32, Vec<u8>)>> {
        let pointer = |reference: Option<TextRef>| reference.map_or(0, |id| sources[id.0].address);
        let mut definitions = Vec::new();
        for row in &catalogue.definitions {
            for value in [row.id, pointer(row.name), pointer(row.description)] {
                definitions.extend(value.to_be_bytes());
            }
            for bonus in row.stat_bonuses {
                definitions.extend([bonus.selector, bonus.percent]);
            }
            definitions.extend(row.tendency.to_be_bytes());
            definitions.extend([row.storage, row.activation]);
        }
        definitions.extend(catalogue.definition_storage.to_be_bytes());
        let mut choices = Vec::new();
        let mut compounds = Vec::new();
        for character in &catalogue.characters {
            choices.extend(character.levels.into_iter().flatten());
            compounds.extend(character.compound_count.to_be_bytes());
            for row in &character.compounds {
                compounds.extend(row.skill.to_be_bytes());
                compounds.extend(row.requirement_count.to_be_bytes());
                compounds.extend(row.requirements);
            }
        }
        let mut personal: Vec<_> = catalogue
            .personal_skills
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect();
        personal.extend(catalogue.personal_skill_storage);
        let mut labels: Vec<_> = catalogue
            .labels
            .into_iter()
            .flat_map(|id| pointer(id).to_be_bytes())
            .collect();
        labels.extend(catalogue.label_storage.to_be_bytes());
        let mut regions = vec![
            (DEFINITIONS, definitions),
            (CHOICES, choices),
            (COMPOUNDS, compounds),
            (PERSONAL_SKILLS, personal),
            (LABELS, labels),
            (
                0x800cf788,
                (0x2c00_0000u32 | u32::from(catalogue.save_point_rule.character_index as u16))
                    .to_be_bytes()
                    .to_vec(),
            ),
            (
                0x800cf7a0,
                (0x3860_0000u32 | u32::from(catalogue.save_point_rule.tp_cost as u16))
                    .to_be_bytes()
                    .to_vec(),
            ),
        ];
        // Encode inline control operands as bytes, independently of Shift-JIS text.
        for (source, text) in sources.iter().zip(&catalogue.texts) {
            let mut bytes = Vec::new();
            let mut chars = text.chars();
            while let Some(character) = chars.next() {
                if matches!(character, '\u{b}' | '\u{c}') {
                    bytes.extend([
                        character as u8,
                        u8::try_from(u32::from(chars.next().context("missing control operand")?))?,
                    ]);
                } else {
                    let character = character.to_string();
                    let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(&character);
                    ensure!(!invalid, "unencodable EX source text");
                    bytes.extend(encoded.as_ref());
                }
            }
            bytes.push(0);
            assert_eq!(bytes.len() as u32, source.source_size);
            regions.push((source.address, bytes));
        }
        for format in [
            &catalogue.formats.strike_type,
            &catalogue.formats.technical_type,
            &catalogue.formats.gem_max,
            &catalogue.formats.gem_level,
            &catalogue.formats.gem_empty,
            &catalogue.formats.stat_arrow,
        ] {
            let source = &sources[format.text.0];
            regions.push((source.address + source.source_size, format.storage.clone()));
        }
        Ok(regions)
    }

    fn assert_reconstruction(
        executable: &[u8],
        catalogue: &Catalogue,
        sources: &[TextSource],
    ) -> Result<()> {
        for (address, bytes) in reconstruct(catalogue, sources)? {
            assert_eq!(
                bytes,
                dol::slice(executable, address, bytes.len())?,
                "EX source {address:08x}"
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; publishes only EX skill JSON"]
    fn original_ex_catalogue_preserves_definitions_recipes_aliases_and_storage() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let destination = output.join(format!("disc{disc}"));
                let (catalogue, sources) = parse(&executable)?;
                let paths = cook(&file, &executable, &destination)?;
                let restored: Catalogue = crate::embedded::read(&destination, FAMILY, "main.dol")?;
                assert_eq!(restored, catalogue);
                payloads.insert(paths[0].clone());
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                assert_reconstruction(&executable, &restored, &sources)?;
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

                // Unselected records, inactive slots and unresolved storage remain authored data.
                let name = sources[restored.definitions[1]
                    .name
                    .context("missing skill name")?
                    .0]
                    .address;
                let label = sources[restored.labels[1].context("missing label")?.0].address;
                let inactive = DEFINITIONS + 17 * 20;
                for (address, replacement) in [
                    (inactive, 0xdeadbeefu32.to_be_bytes().to_vec()),
                    (inactive + 4, 0u32.to_be_bytes().to_vec()),
                    (inactive + 8, name.to_be_bytes().to_vec()),
                    (inactive + 12, vec![255, 128, 0, 99, 0x80, 0, 0xa5, 255]),
                    (
                        DEFINITIONS + DEFINITION_COUNT as u32 * 20,
                        0x12345678u32.to_be_bytes().to_vec(),
                    ),
                    (CHOICES, vec![255]),
                    (COMPOUNDS, 25u32.to_be_bytes().to_vec()),
                    (COMPOUNDS + 4, vec![1, 255, 255, 255, 1, 2, 0x81, 0xff]),
                    (PERSONAL_SKILLS + 18, vec![1, 2, 3, 4, 5, 6]),
                    (LABELS, 0u32.to_be_bytes().to_vec()),
                    (LABELS + 8, label.to_be_bytes().to_vec()),
                    (LABELS + 76, 0xdeadbeefu32.to_be_bytes().to_vec()),
                    (0x8035d64f, vec![0xa5]),
                    (0x8035d657, vec![0x5a]),
                    (0x8035d661, vec![0x81, 0xff, 0x7f]),
                    (0x8035d66b, vec![1, 2, 3, 4, 5]),
                    (0x800cf788, 0x2c00fff9u32.to_be_bytes().to_vec()),
                    (0x800cf7a0, 0x3860fffeu32.to_be_bytes().to_vec()),
                ] {
                    let source = dol::slice(&executable, address, replacement.len())?;
                    let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                    executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
                }
                let (changed, sources) = parse(&executable)?;
                assert_reconstruction(&executable, &changed, &sources)?;
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
                assert_eq!(changed.label_storage, 0xdeadbeef);
                assert_eq!(changed.save_point_rule.character_index, -7);
                assert_eq!(changed.save_point_rule.tp_cost, -2);
                let modified = destination.join("modified.dol");
                fs::write(&modified, &executable)?;
                cook(&modified, &executable, &destination)?;
                let published: Catalogue =
                    crate::embedded::read(&destination, FAMILY, "modified.dol")?;
                assert_eq!(published, changed);
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
