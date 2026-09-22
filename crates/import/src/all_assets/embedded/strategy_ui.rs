//! Authored strategy choices, presets, lane selection and rename/formation bindings.
use super::text::{TextPool, TextRef};
use crate::{
    dol,
    read::{Field, u32 as word},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
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

crate::read::record! {
    #[derive(Debug, PartialEq)]
    pub(crate) struct Preferences(3) {
        pub action: u8 => 0,
        pub skill_magic: u8 => 1,
        /// Out-of-group selectors use the authored invalid-choice format.
        pub position: u8 => 2,
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Choice {
    pub name: Option<TextRef>,
    pub description: Option<TextRef>,
    pub details: Option<TextRef>,
    /// Native eight-bit availability mask; Kratos uses the same bit as Zelos.
    pub characters: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Group {
    pub category: Category,
    pub label: Option<TextRef>,
    pub choices: Vec<Choice>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Preset {
    pub name: TextRef,
    pub members: [Preferences; 9],
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
    pub lanes: [Lane; 6],
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

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
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
                })
            })
            .collect::<Result<_>>()?;
        groups.push(Group {
            category: [Category::Action, Category::SkillMagic, Category::Position][index],
            label: texts.array::<1>(executable, GROUP_LABELS + index as u32 * 4)?[0],
            choices,
        });
    }
    let presets = dol::slice(executable, PRESETS, 180)?
        .chunks_exact(60)
        .enumerate()
        .map(|(index, row)| {
            let name = texts.fixed(executable, PRESETS + index as u32 * 60, 32)?;
            Ok(Preset {
                name,
                members: Field::read(row, 32)?,
            })
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let defaults = dol::slice(executable, DEFAULT_POSITIONS, 9)?;
    let default_positions = defaults
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
    let lanes = dol::slice(executable, LANES, 6)?;
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
    let catalogue = Catalogue {
        groups: groups.try_into().unwrap(),
        presets,
        default_positions,
        lanes: lanes
            .iter()
            .copied()
            .map(lane)
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        labels: Labels {
            title,
            invalid_choice_format,
            rename,
            default,
            orders,
            preset_name_format,
            party_position_format: texts.required(executable, 0x8035d6c0)?,
            choice_format: texts.required(executable, 0x8035d6c4)?,
        },
        keyboard: Keyboard {
            cells: cells(&keyboard[..90])?,
            input_cells: cells(dol::slice(executable, input_address, 90)?)?,
            keys: texts.array(executable, KEYBOARD + 92)?,
            preset_formation_x: Field::read(keyboard, 128)?,
            current_formation_x: Field::read(keyboard, 140)?,
        },
        texts: texts.values,
    };
    Ok(catalogue)
}

#[cfg(test)]
pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let catalogue = read(executable)?;
    crate::embedded::write(file, output, FAMILY, &catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn patch(executable: &mut [u8], address: u32, bytes: &[u8]) -> Result<()> {
        let at = dol::slice(executable, address, bytes.len())?.as_ptr() as usize
            - executable.as_ptr() as usize;
        executable[at..at + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    #[test]
    #[ignore = "requires both original executables; no codecs or devices"]
    fn original_strategy_ui_preserves_choices_and_publishes_shared_data() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("strategy-ui"));
        fs::create_dir(&output)?;
        let result = (|| -> Result<()> {
            let mut first = None;
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let c = read(&executable)?;
                let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&c)?)?;
                assert_eq!(c, restored);
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

                // Preserve optional labels, independent input grids and signed positions.
                for (address, bytes) in [
                    (GROUPS[0].0, vec![0; 4]),
                    (PRESETS + 32, vec![99, 98, 97]),
                    (INPUT_KEYBOARD, (KEYBOARD + 1).to_be_bytes().to_vec()),
                    (KEYBOARD + 128, (-300i32).to_be_bytes().to_vec()),
                    (0x8035d6c0, b"\x0b\0X\0".to_vec()),
                ] {
                    patch(&mut executable, address, &bytes)?;
                }
                let changed = read(&executable)?;
                assert!(changed.groups[0].choices[0].name.is_none());
                assert_eq!(
                    changed.presets[0].members[0],
                    Preferences {
                        action: 99,
                        skill_magic: 98,
                        position: 97
                    }
                );
                assert_ne!(changed.keyboard.cells, changed.keyboard.input_cells);
                assert_eq!(changed.keyboard.preset_formation_x[0], -300);
                assert_eq!(
                    changed.text(changed.labels.party_position_format),
                    "\x0b\0X"
                );
                for invalid in [0, 7] {
                    patch(&mut executable, DEFAULT_POSITIONS, &[invalid])?;
                    assert!(read(&executable).is_err());
                }
                patch(
                    &mut executable,
                    DEFAULT_POSITIONS,
                    &[c.default_positions[0] as u8],
                )?;
                patch(&mut executable, COUNTS, &u32::MAX.to_be_bytes())?;
                assert!(read(&executable).is_err());
            }
            Ok(())
        })();
        fs::remove_dir_all(output)?;
        result
    }
}
