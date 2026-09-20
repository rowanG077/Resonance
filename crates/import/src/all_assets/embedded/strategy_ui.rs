//! Authored strategy choices, presets, lane selection and rename/formation bindings.
use super::text::{TextPool, TextRef, TextSource};
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "strategy-ui";
const PRESETS: u32 = 0x80208b88;
const GROUPS: [(u32, usize); 3] = [(0x80208c3c, 144), (0x80208ccc, 144), (0x80208d5c, 116)];
const COUNTS: u32 = 0x801ab138;
const REFERENCES: u32 = 0x801ab144;
const LABELS: u32 = 0x801ab0fc;
const GROUP_LABELS: u32 = 0x801ab12c;
const KEYBOARD: u32 = 0x801ab150;
const INPUT_KEYBOARD: u32 = 0x8035d690;
const DEFAULT_POSITIONS: u32 = 0x8018adf4;
const LANES: u32 = 0x8035c044;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Lane {
    Front,
    Middle,
    Rear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Category {
    Action,
    SkillMagic,
    Position,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Position {
    MoveFreely,
    Frontlines,
    DontPursue,
    #[serde(rename = "hold_position")]
    Hold,
    LongRangeSkills,
    LongRangeMagic,
    SkillsMagic,
}

impl Position {
    fn read(value: u8) -> Result<Self> {
        Ok(match value {
            0 => Self::MoveFreely,
            1 => Self::Frontlines,
            2 => Self::DontPursue,
            3 => Self::Hold,
            4 => Self::LongRangeSkills,
            5 => Self::LongRangeMagic,
            6 => Self::SkillsMagic,
            _ => anyhow::bail!("invalid strategy position selector {value}"),
        })
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Preferences {
    pub action: u8,
    pub skill_magic: u8,
    /// Out-of-group selectors use the authored invalid-choice format.
    pub position: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Choice {
    pub name: Option<TextRef>,
    pub description: Option<TextRef>,
    pub details: Option<TextRef>,
    /// Native eight-bit availability mask; Kratos uses the same bit as Zelos.
    pub characters: u8,
    pub storage: [u8; 3],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Group {
    pub category: Category,
    pub label: Option<TextRef>,
    pub choices: Vec<Choice>,
    pub storage: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Preset {
    pub name: TextRef,
    /// Bytes after the terminator in the fixed 32-byte name buffer.
    pub name_storage: Vec<u8>,
    pub members: [Preferences; 9],
    pub storage: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Labels {
    pub title: Option<TextRef>,
    pub invalid_choice_format: Option<TextRef>,
    pub rename: Option<TextRef>,
    pub default: Option<TextRef>,
    pub orders: Option<TextRef>,
    pub preset_name_format: Option<TextRef>,
    pub party_position_format: TextRef,
    pub choice_format: TextRef,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Keyboard {
    pub cells: String,
    /// Native input reads through a separate pointer; retain that binding independently.
    pub input_cells: String,
    pub storage: [u8; 2],
    pub keys: [Option<TextRef>; 9],
    pub preset_formation_x: [i32; 3],
    pub current_formation_x: [i32; 3],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub groups: [Group; 3],
    pub presets: [Preset; 3],
    pub default_positions: [Position; 9],
    pub default_position_storage: [u8; 3],
    pub lanes: [Lane; 6],
    pub lane_storage: [u8; 6],
    pub labels: Labels,
    pub keyboard: Keyboard,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }
    pub(crate) fn required_text(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required strategy text")?))
    }
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let mut texts = TextPool::default();
    let mut groups = Vec::new();
    for (index, (address, size)) in GROUPS.into_iter().enumerate() {
        ensure!(
            word(dol::slice(executable, REFERENCES + index as u32 * 4, 4)?, 0)? == address,
            "strategy table declaration does not match physical extent"
        );
        let count = word(dol::slice(executable, COUNTS + index as u32 * 4, 4)?, 0)? as usize;
        let bytes = dol::slice(executable, address, size)?;
        let used = count
            .checked_mul(16)
            .filter(|&n| n <= bytes.len())
            .context("strategy table exceeds physical extent")?;
        let choices = bytes[..used]
            .chunks_exact(16)
            .map(|row| {
                Ok(Choice {
                    name: texts.reference(executable, word(row, 0)?)?,
                    description: texts.reference(executable, word(row, 4)?)?,
                    details: texts.reference(executable, word(row, 8)?)?,
                    characters: row[12],
                    storage: row[13..16].try_into()?,
                })
            })
            .collect::<Result<_>>()?;
        groups.push(Group {
            category: [Category::Action, Category::SkillMagic, Category::Position][index],
            label: texts.array::<1>(executable, GROUP_LABELS + index as u32 * 4)?[0],
            choices,
            storage: bytes[used..].to_vec(),
        });
    }
    let presets = dol::slice(executable, PRESETS, 180)?
        .chunks_exact(60)
        .enumerate()
        .map(|(index, row)| {
            let name = texts
                .reference(executable, PRESETS + index as u32 * 60)?
                .unwrap();
            let length = texts.sources[name.0].source_size as usize;
            ensure!(length <= 32, "strategy preset name exceeds fixed buffer");
            let members = row[32..59]
                .chunks_exact(3)
                .map(|row| {
                    Ok(Preferences {
                        action: row[0],
                        skill_magic: row[1],
                        position: row[2],
                    })
                })
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap();
            Ok(Preset {
                name,
                name_storage: row[length..32].to_vec(),
                members,
                storage: row[59],
            })
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let defaults = dol::slice(executable, DEFAULT_POSITIONS, 12)?;
    let default_positions = defaults[..9]
        .iter()
        .map(|&v| {
            ensure!(
                v != 0,
                "default strategy position is outside the lane lookup"
            );
            Position::read(v)
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let lanes = dol::slice(executable, LANES, 12)?;
    let lane = |value| -> Result<Lane> {
        Ok(match value {
            0 => Lane::Front,
            1 => Lane::Middle,
            2 => Lane::Rear,
            _ => anyhow::bail!("invalid strategy lane {value}"),
        })
    };
    let [
        title,
        invalid_choice_format,
        rename,
        default,
        orders,
        preset_name_format,
    ] = texts.array(executable, LABELS)?;
    let keyboard = dol::slice(executable, KEYBOARD, 152)?;
    let input_address = word(dol::slice(executable, INPUT_KEYBOARD, 4)?, 0)?;
    let cells = |bytes: &[u8]| -> Result<String> {
        ensure!(bytes.is_ascii(), "non-ASCII strategy keyboard");
        Ok(String::from_utf8(bytes.to_vec())?)
    };
    let coordinates = |offset| -> Result<[i32; 3]> {
        (offset..offset + 12)
            .step_by(4)
            .map(|at| Ok(word(keyboard, at)? as i32))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .map_err(|_| anyhow::anyhow!("strategy formation length"))
    };
    let catalogue = Catalogue {
        groups: groups.try_into().unwrap(),
        presets,
        default_positions,
        default_position_storage: defaults[9..].try_into()?,
        lanes: lanes[..6]
            .iter()
            .copied()
            .map(lane)
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        lane_storage: lanes[6..].try_into()?,
        labels: Labels {
            title,
            invalid_choice_format,
            rename,
            default,
            orders,
            preset_name_format,
            party_position_format: texts.reference(executable, 0x8035d6c0)?.unwrap(),
            choice_format: texts.reference(executable, 0x8035d6c4)?.unwrap(),
        },
        keyboard: Keyboard {
            cells: cells(&keyboard[..90])?,
            input_cells: cells(dol::slice(executable, input_address, 90)?)?,
            storage: keyboard[90..92].try_into()?,
            keys: texts.array(executable, KEYBOARD + 92)?,
            preset_formation_x: coordinates(128)?,
            current_formation_x: coordinates(140)?,
        },
        texts: texts.values,
    };
    Ok((catalogue, texts.sources))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, texts) = parse(executable)?;
    crate::embedded::write(
        file,
        output,
        FAMILY,
        &catalogue,
        serde_json::json!({
            "presets":{"address":PRESETS,"count":3,"stride":60},
            "groups":GROUPS.map(|(address, source_size)| serde_json::json!({"address":address,"source_size":source_size})),
            "counts":{"address":COUNTS,"count":3,"stride":4},
            "references":{"address":REFERENCES,"count":3,"stride":4},
            "labels":{"address":LABELS,"count":6,"stride":4},
            "group_labels":{"address":GROUP_LABELS,"count":3,"stride":4},
            "keyboard":{"address":KEYBOARD,"source_size":152},
            "input_keyboard":{"pointer_address":INPUT_KEYBOARD,"address":word(dol::slice(executable,INPUT_KEYBOARD,4)?,0)?,"source_size":90},
            "default_positions":{"address":DEFAULT_POSITIONS,"source_size":12},
            "lanes":{"address":LANES,"source_size":12},"texts":texts,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn reconstruct(c: &Catalogue, sources: &[TextSource]) -> Result<Vec<(u32, Vec<u8>)>> {
        let pointers = |references: &[Option<TextRef>]| {
            references
                .iter()
                .flat_map(|r| r.map_or(0, |id| sources[id.0].address).to_be_bytes())
                .collect::<Vec<_>>()
        };
        let text_bytes = |id: TextRef| -> Result<Vec<u8>> {
            let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(c.text(id));
            ensure!(!invalid, "strategy text cannot reconstruct source encoding");
            Ok([bytes.as_ref(), &[0]].concat())
        };
        let mut spans = Vec::new();
        for (group, &(address, _)) in c.groups.iter().zip(&GROUPS) {
            let mut bytes = Vec::new();
            for choice in &group.choices {
                bytes.extend(pointers(&[choice.name, choice.description, choice.details]));
                bytes.push(choice.characters);
                bytes.extend(choice.storage);
            }
            bytes.extend(&group.storage);
            spans.push((address, bytes));
        }
        let mut presets = Vec::new();
        for preset in &c.presets {
            presets.extend(text_bytes(preset.name)?);
            presets.extend(&preset.name_storage);
            for p in &preset.members {
                presets.extend([p.action, p.skill_magic, p.position]);
            }
            presets.push(preset.storage);
        }
        spans.push((PRESETS, presets));
        spans.push((
            COUNTS,
            c.groups
                .iter()
                .flat_map(|g| (g.choices.len() as u32).to_be_bytes())
                .collect(),
        ));
        spans.push((
            REFERENCES,
            GROUPS
                .iter()
                .flat_map(|(address, _)| address.to_be_bytes())
                .collect(),
        ));
        spans.push((
            GROUP_LABELS,
            pointers(&c.groups.each_ref().map(|g| g.label)),
        ));
        let l = &c.labels;
        spans.push((
            LABELS,
            pointers(&[
                l.title,
                l.invalid_choice_format,
                l.rename,
                l.default,
                l.orders,
                l.preset_name_format,
            ]),
        ));
        let mut keyboard = c.keyboard.cells.as_bytes().to_vec();
        keyboard.extend(c.keyboard.storage);
        keyboard.extend(pointers(&c.keyboard.keys));
        keyboard.extend(
            c.keyboard
                .preset_formation_x
                .into_iter()
                .chain(c.keyboard.current_formation_x)
                .flat_map(i32::to_be_bytes),
        );
        spans.push((KEYBOARD, keyboard));
        spans.push((
            DEFAULT_POSITIONS,
            c.default_positions
                .map(|v| v as u8)
                .into_iter()
                .chain(c.default_position_storage)
                .collect(),
        ));
        spans.push((
            LANES,
            c.lanes
                .map(|v| v as u8)
                .into_iter()
                .chain(c.lane_storage)
                .collect(),
        ));
        for (index, source) in sources.iter().enumerate() {
            let bytes = text_bytes(TextRef(index))?;
            assert_eq!(bytes.len() as u32, source.source_size);
            spans.push((source.address, bytes));
        }
        Ok(spans)
    }

    fn patch(executable: &mut [u8], address: u32, bytes: &[u8]) -> Result<()> {
        let at = dol::slice(executable, address, bytes.len())?.as_ptr() as usize
            - executable.as_ptr() as usize;
        executable[at..at + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    #[test]
    #[ignore = "requires both original executables; no codecs or devices"]
    fn original_strategy_ui_reconstructs_complete_tables_and_publishes_shared_data() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("strategy-ui"));
        fs::create_dir(&output)?;
        let result = (|| -> Result<()> {
            let mut first = None;
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let (c, sources) = parse(&executable)?;
                let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&c)?)?;
                assert_eq!(c, restored);
                for (address, bytes) in reconstruct(&restored, &sources)? {
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, bytes.len())?,
                        "span {address:#x}"
                    );
                }
                assert_eq!(c.groups.each_ref().map(|g| g.choices.len()), [9, 9, 7]);
                assert_eq!(c.keyboard.cells, c.keyboard.input_cells);
                assert_eq!(c.keyboard.preset_formation_x, [280, 160, 40]);
                assert_eq!(c.keyboard.current_formation_x, [280, 160, 40]);
                assert_eq!(c.keyboard.keys[0], c.keyboard.keys[1]);
                assert_eq!(c.keyboard.keys[1], c.keyboard.keys[2]);
                assert_eq!(c.required_text(c.labels.invalid_choice_format)?, "Error:%d");
                assert_eq!(
                    c.required_text(c.labels.preset_name_format)?,
                    "User Command 0%c"
                );
                let paths = cook(&file, &executable, &output)?;
                assert_eq!(
                    crate::embedded::read::<Catalogue>(&output, FAMILY, "main.dol")?,
                    c
                );
                if let Some(expected) = &first {
                    assert_eq!(&paths[0], expected);
                } else {
                    first = Some(paths[0].clone());
                }
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                assert_eq!(provenance["groups"][2]["source_size"], 116);

                // Nonzero storage, null/aliased text, out-of-group preset selectors
                // and signed positions survive independently of runtime admission.
                for (address, bytes) in [
                    (GROUPS[0].0 + 13, vec![1, 2, 3]),
                    (GROUPS[2].0 + 112, vec![4, 5, 6, 7]),
                    (GROUPS[0].0, vec![0; 4]),
                    (PRESETS + 20, vec![8]),
                    (PRESETS + 59, vec![9]),
                    (PRESETS + 32, vec![99, 98, 97]),
                    (DEFAULT_POSITIONS + 9, vec![10, 11, 12]),
                    (LANES + 6, vec![13, 14, 15, 16, 17, 18]),
                    (KEYBOARD + 90, vec![19, 20]),
                    (KEYBOARD + 128, (-300i32).to_be_bytes().to_vec()),
                    (
                        sources[c.labels.party_position_format.0].address,
                        b"\x0b\0X\0".to_vec(),
                    ),
                ] {
                    patch(&mut executable, address, &bytes)?;
                }
                let (changed, changed_sources) = parse(&executable)?;
                assert!(changed.groups[0].choices[0].name.is_none());
                assert_eq!(changed.keyboard.preset_formation_x[0], -300);
                assert_eq!(
                    changed.text(changed.labels.party_position_format),
                    "\x0b\0X"
                );
                for (address, bytes) in reconstruct(&changed, &changed_sources)? {
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, bytes.len())?,
                        "changed span {address:#x}"
                    );
                }
                patch(&mut executable, COUNTS, &u32::MAX.to_be_bytes())?;
                assert!(read(&executable).is_err());
            }
            Ok(())
        })();
        fs::remove_dir_all(output)?;
        result
    }
}
