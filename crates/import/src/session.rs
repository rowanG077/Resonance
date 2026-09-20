//! Extract fresh-game definitions from the executable into ordinary JSON.
use crate::{digest, dol, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::session::{CharacterDefinition, ItemDefinition, SessionData, StatGrowth};
use std::{collections::BTreeMap, fs, path::Path};

/// Localized display text stays separate from save-compatible item statistics.
pub(crate) fn cook_text(extracted: &Path, output: &Path) -> Result<String> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let defaults = crate::character_data::read(&executable)?;
    let characters = (1..=10)
        .map(|id| {
            let name = if id == 10 {
                // The companion's name is initialized separately from party records.
                dol::text(&executable, 0x8035bb80)?
            } else {
                defaults.definitions[(id - 1) as usize].name.clone()
            };
            ensure!(
                !name.is_empty() && !name.chars().any(char::is_control),
                "invalid character name {id}"
            );
            Ok((id, name))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut names = BTreeMap::new();
    for (id, row) in crate::item::read(&executable)?.into_iter().enumerate() {
        let name = row.name.context("missing item name")?;
        ensure!(
            !name.chars().any(char::is_control),
            "invalid item name {id}"
        );
        names.insert(id as u16, name);
    }
    let mut titles = BTreeMap::new();
    let starts: Vec<_> = dol::slice(&executable, 0x80210920, 18)?
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes(b.try_into().unwrap()))
        .chain([159])
        .collect();
    ensure!(
        starts.windows(2).all(|w| w[0] < w[1] && w[1] - w[0] < 32),
        "invalid title table ranges"
    );
    for character in 0..9 {
        for (title, index) in (starts[character]..starts[character + 1]).enumerate() {
            let pointer = u32::from_be_bytes(
                dol::slice(&executable, 0x80210934 + u32::from(index) * 16, 4)?.try_into()?,
            );
            let bytes = dol::slice(&executable, pointer, 128)?;
            let end = bytes
                .iter()
                .position(|&b| b == 0)
                .context("unterminated title name")?;
            let (name, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes[..end]);
            ensure!(
                !invalid && !name.chars().any(char::is_control),
                "invalid title name"
            );
            titles.insert(
                (character as u16) << 8 | (title + 1) as u16,
                name.into_owned(),
            );
        }
    }
    let text = resonance_content::session::GameText {
        characters,
        items: names,
        titles,
    };
    let path = "game/text.json";
    write_atomic(&output.join(path), &serde_json::to_vec_pretty(&text)?)?;
    Ok(path.into())
}

/// Nine logical party bits, including shared swordsman gear and its source exceptions.
pub(crate) fn equipment_owners(
    executable: &[u8],
    items: &[crate::item::Definition],
) -> Result<Vec<u16>> {
    let kratos_only = [0x800eba48, 0x800eba54, 0x800eba60, 0x800eba6c]
        .into_iter()
        .map(|address| {
            let instruction = crate::read::u32(dol::slice(executable, address, 4)?, 0)?;
            ensure!(
                instruction >> 16 == 0x2c05,
                "unsupported equipment owner check"
            );
            Ok(usize::from(instruction as u16))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(items
        .iter()
        .enumerate()
        .map(|(item, row)| {
            expand_equipment_owners(row.equipment_owner_mask, kratos_only.contains(&item))
        })
        .collect())
}

fn expand_equipment_owners(raw: u8, kratos_only: bool) -> u16 {
    let mask = u16::from(raw);
    let mask = mask | ((mask & 0x20) << 3);
    if kratos_only { mask & !0x20 } else { mask }
}

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<String> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let arte_catalogue = crate::arte::read(&executable)?;
    let defaults = crate::character_data::read(&executable)?;
    let items = crate::item::read(&executable)?;
    let owners = equipment_owners(&executable, &items)?;
    let items = items
        .iter()
        .enumerate()
        .map(|(id, row)| ItemDefinition {
            equipment_kind: match row.category {
                13..=22 => Some(0),
                23..=26 => Some(1),
                27..=30 => Some(2),
                31..=34 => Some(3),
                35..=42 => Some(4),
                _ => None,
            },
            allowed_characters: owners[id],
            stack_limit: resonance_content::session::item_stack_limit(row.category),
        })
        .collect();
    let mut characters = Vec::new();
    for (index, row) in defaults.definitions.iter().take(9).enumerate() {
        let list = arte_catalogue.learned_by(index as u8 + 1)?;
        let mut techniques = Vec::new();
        let mut level_techniques = BTreeMap::<u8, Vec<u16>>::new();
        for (slot, &id) in list.iter().enumerate() {
            // Owner masks number bits from the least significant bit.
            if row.learned_techniques & (1u64 << slot) != 0 {
                techniques.push(u16::from(id));
            }
            let tech = arte_catalogue.definition(usize::from(id))?;
            let required = tech.required_level;
            if required != 0
                && required <= 250
                && tech.learning_route == 0
                && tech.learning_parent == 0
                && tech.required_learned[0] == 0
            {
                level_techniques
                    .entry(required as u8)
                    .or_default()
                    .push(u16::from(id));
            }
        }
        let gains = &defaults.growth[index];
        let title_start = crate::read::u16(
            dol::slice(&executable, 0x80210920 + index as u32 * 2, 2)?,
            0,
        )?;
        // Members start with title 1 before setup scripts run.
        let title = dol::slice(&executable, 0x80210934 + u32::from(title_start) * 16, 16)?;
        let stats = [
            row.base_hp,
            row.base_tp,
            row.base_attack,
            row.base_defense,
            row.base_intelligence,
            row.base_evasion,
            row.base_accuracy,
        ]
        .map(|value| value as u16);
        characters.push(CharacterDefinition {
            cooking: row.cooking,
            ex_skills: row.ex_skills,
            ex_gems: row.ex_gems,
            compound_ex_skills: (0..24u8)
                .filter(|i| row.compound_ex_skills & (1 << i) != 0)
                .collect(),
            recent_compound_ex_skills: (0..24u8)
                .filter(|i| row.recent_compound_ex_skills & (1 << i) != 0)
                .collect(),
            technique_balance: row.technique_balance,
            affinity: row.affinity,
            level: row.level,
            experience: row.experience,
            base_stats: stats,
            luck: (row.base_luck as u16 / 10).min(255) as u8,
            overlimit: row.overlimit,
            equipment: [0, 1, 2, 4, 5, 3].map(|slot| row.equipment[slot] as u16),
            techniques,
            allowed_techniques: list.iter().map(|id| u16::from(*id)).collect(),
            shortcuts: row.shortcuts.map(|id| id as u16),
            growth: std::array::from_fn(|i| StatGrowth {
                base: gains[i].base,
                random: gains[i].random,
                title_bonus: title[8 + i],
            }),
            level_techniques,
        });
    }
    let data = SessionData {
        ex_skills: None,
        version: 1,
        executable_sha256: digest(&executable),
        items,
        characters,
        experience: defaults.experience,
    };
    data.validate()?;
    let path = "game/session-data.json";
    write_atomic(&output.join(path), &serde_json::to_vec_pretty(&data)?)?;
    Ok(path.into())
}

#[cfg(test)]
mod owner_tests {
    use super::*;

    #[test]
    #[ignore = "requires both original extracted discs; only exports session JSON"]
    fn original_session_preserves_allowed_and_initially_learned_techniques() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("session-catalogue"));
        let result = (|| -> Result<()> {
            for disc in [1, 2] {
                let source = extracted.join(format!("disc{disc}"));
                let executable = fs::read(source.join("sys/main.dol"))?;
                let path = cook(&source, &output)?;
                let data: SessionData = serde_json::from_slice(&fs::read(output.join(path))?)?;
                assert_eq!(data.characters.len(), 9);
                for (index, character) in data.characters.iter().enumerate() {
                    let row = dol::slice(&executable, 0x80202dc8 + index as u32 * 41, 41)?;
                    let expected: Vec<_> = row[1..=usize::from(row[0])]
                        .iter()
                        .map(|&id| u16::from(id))
                        .collect();
                    assert_eq!(character.allowed_techniques, expected);
                    let learned = u64::from_be_bytes(
                        dol::slice(&executable, 0x801f9fc8 + index as u32 * 0x118 + 0x70, 8)?
                            .try_into()?,
                    );
                    assert_eq!(
                        character.techniques,
                        expected
                            .into_iter()
                            .enumerate()
                            .filter_map(|(slot, id)| (learned & (1 << slot) != 0).then_some(id))
                            .collect::<Vec<_>>()
                    );
                }
            }
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }

    #[test]
    fn equipment_masks_preserve_all_nine_owners_and_kratos_exclusive_gear() {
        for (raw, exclusive, expected) in [
            (0xff, false, &[1, 2, 3, 4, 5, 6, 7, 8, 9][..]),
            (0x20, false, &[6, 9]),
            (0x20, true, &[9]),
            (0x24, true, &[3, 9]),
            (0x80, false, &[8]),
            (0, true, &[]),
        ] {
            let mask = super::expand_equipment_owners(raw, exclusive);
            let owners = (1..=9)
                .filter(|character| mask & (1_u16 << (character - 1)) != 0)
                .collect::<Vec<_>>();
            assert_eq!(owners, expected);
        }
    }
}
