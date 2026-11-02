//! Native strategy tables directly into named orders and formation choices.
use crate::{output, pe, source::Source, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    library::Mappings,
    overworld::PartyCharacter,
    strategy::{
        self, Actions, Caption, Choice, Choices, Document, Group, Keyboard, Lane, Orders,
        Overrides, Preset,
    },
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
struct Names {
    groups: BTreeMap<Group, Mappings>,
    presets: Mappings,
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = text::Bank::parse(&bytes)?;
    let slot = source.language.slot();
    let word = |address| pe::word(image.slice(address, 4)?, 0);
    let directory = |address| word(address + slot * 4);
    let caption = |table, index| bank.menu(word(table + index * 4)?, encoding_rs::WINDOWS_1252);
    let names: Names =
        serde_json::from_str(include_str!("../../importer/src/strategy-names.json"))?;
    let (default_ids, lanes) = formation(&image, 0x52aff5)?;
    ensure!(
        (default_ids, lanes) == formation(&image, 0x52c3bb)?,
        "strategy preview formation rules differ"
    );
    let descriptions = directory(0x8b877c)?;
    let group_labels = directory(0x8b8704)?;
    let mut groups = BTreeMap::new();
    for (index, group) in Group::ALL.into_iter().enumerate() {
        let address = word(0x74ef44 + index as u32 * 4)?;
        let count = word(0x74ee04 + index as u32 * 4)? as usize;
        ensure!(
            count == [9, 9, 7][index],
            "unexpected strategy choice count"
        );
        let mappings = names
            .groups
            .get(&group)
            .context("missing strategy group names")?;
        ensure!(
            mappings.iter().count() == count,
            "strategy names do not cover choices"
        );
        let mut options = BTreeMap::new();
        for (id, row) in image
            .slice(address, count * 16)?
            .chunks_exact(16)
            .enumerate()
        {
            let mask = u16::from(row[12]);
            // The source shares Zelos's availability bit with Kratos.
            let mask = mask | ((mask & 0x20) << 3);
            ensure!(mask > 0 && mask < 512, "invalid strategy character mask");
            let characters = if mask == 511 {
                None
            } else {
                Some(
                    (1..=9)
                        .filter(|member| mask & (1 << (member - 1)) != 0)
                        .map(|member| PartyCharacter::try_from(member as u8).map_err(Into::into))
                        .collect::<Result<Vec<_>>>()?,
                )
            };
            let description = [4, 8]
                .into_iter()
                .map(|offset| caption(descriptions, pe::word(row, offset)?))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect();
            let choice = Choice {
                text: serde_json::to_value(Caption {
                    name: caption(descriptions, pe::word(row, 0)?)?,
                    description,
                })?,
                characters,
                lane: if group == Group::Position && id > 0 {
                    Some(Lane::from_index(lanes[id - 1])?)
                } else {
                    None
                },
            };
            ensure!(
                options
                    .insert(mappings.target(id as u32)?.to_owned(), choice)
                    .is_none(),
                "duplicate strategy choice name"
            );
        }
        groups.insert(
            group,
            Choices {
                mappings: mappings.clone(),
                name: serde_json::to_value(caption(group_labels, index as u32)?)?,
                options,
            },
        );
    }

    let mut default_positions = BTreeMap::new();
    for (index, &id) in default_ids.iter().enumerate() {
        ensure!(
            (1..=6).contains(&id),
            "invalid default strategy position {id}"
        );
        default_positions.insert(
            PartyCharacter::try_from(index as u8 + 1)?,
            Lane::from_index(lanes[usize::from(id - 1)])?,
        );
    }

    let order_names = |row: &[u8]| -> Result<[String; 3]> {
        Group::ALL
            .into_iter()
            .enumerate()
            .map(|(index, group)| {
                Ok(names.groups[&group]
                    .target(u32::from(row[index]))?
                    .to_owned())
            })
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid strategy order width"))
    };
    let mut presets = BTreeMap::new();
    for (id, source) in image.slice(0x8debe8, 3 * 60)?.chunks_exact(60).enumerate() {
        let members: Vec<&[u8]> = source[32..59].chunks_exact(3).collect();
        let mut default = members[0];
        let mut best = 0;
        for candidate in &members {
            let count = members.iter().filter(|other| *other == candidate).count();
            if count > best {
                default = candidate;
                best = count;
            }
        }
        let [action, skill_magic, position] = order_names(default)?;
        let mut characters = BTreeMap::new();
        for (index, row) in members.iter().enumerate() {
            if *row == default {
                continue;
            }
            let [member_action, member_skill, member_position] = order_names(row)?;
            characters.insert(
                PartyCharacter::try_from(index as u8 + 1)?,
                Overrides {
                    action: (row[0] != default[0]).then_some(member_action),
                    skill_magic: (row[1] != default[1]).then_some(member_skill),
                    position: (row[2] != default[2]).then_some(member_position),
                },
            );
        }
        presets.insert(
            names.presets.target(id as u32)?.to_owned(),
            Preset {
                name: serde_json::to_value(caption(directory(0x8d8484)?, id as u32)?)?,
                default: Orders {
                    action,
                    skill_magic,
                    position,
                },
                characters,
            },
        );
    }

    let keyboard = caption(directory(0x8b871c)?, 0)?;
    ensure!(
        keyboard.chars().count() == 90 && !keyboard.chars().any(char::is_control),
        "unsupported strategy keyboard"
    );
    let keys_table = directory(0x8b8734)?;
    let mut keys = (0..9)
        .map(|i| caption(keys_table, i))
        .collect::<Result<Vec<_>>>()?;
    let labels_table = directory(0x8b874c)?;
    let labels = (0..5)
        .map(|i| caption(labels_table, i))
        .collect::<Result<Vec<_>>>()?;
    // The Western port leaves navigation/restore captions blank. Its French
    // Delete/Confirm/Cancel entries are shifted; normalize semantic actions here.
    if slot == 2 {
        ensure!(
            keys[5] == "＝N" && keys[6] == "←" && keys[7] == "Effacer",
            "unrecognized French strategy keyboard"
        );
        keys[5] = "Effacer".into();
        keys[6] = "Confirmer".into();
        keys[7] = "Annuler".into();
    }
    keys[3] = "→".into();
    keys[4] = "←".into();
    if keys[8].is_empty() {
        keys[8] = labels[3].clone();
    }
    let [
        a,
        b,
        c,
        move_right,
        move_left,
        delete,
        confirm,
        cancel,
        initial,
    ]: [String; 9] = keys.try_into().unwrap();
    let cells: Vec<_> = keyboard.chars().collect();
    let rows = std::array::from_fn(|row| cells[row * 10..row * 10 + 10].iter().collect::<String>());
    let document = Document {
        groups,
        preset_mappings: names.presets,
        presets,
        default_positions,
        keyboard: serde_json::to_value(Keyboard {
            rows,
            actions: Actions {
                move_right,
                move_left,
                delete,
                confirm,
                cancel,
                initial,
            },
            mode_labels: [a, b, c],
        })?,
        labels: serde_json::json!({
            "title": labels[0], "rename": labels[2],
            "default": labels[3], "orders": labels[4],
        }),
    };
    document.prepare()?;
    output::json(&library.join(strategy::PATH), &document)
}

// Both native previews build these selector/lane arrays with x86 immediates.
fn formation(image: &pe::Image<'_>, address: u32) -> Result<([u8; 9], [u8; 6])> {
    let b = image.slice(address, 36)?;
    ensure!(
        b[..4] == [0xc7, 0x44, 0x24, 0x58]
            && b[8..12] == [0xc7, 0x44, 0x24, 0x5c]
            && b[16..20] == [0xc6, 0x44, 0x24, 0x60]
            && b[21..25] == [0xc7, 0x44, 0x24, 0x48]
            && b[29..34] == [0x66, 0xc7, 0x44, 0x24, 0x4c],
        "unsupported strategy formation initializer"
    );
    let defaults = b[4..8]
        .iter()
        .chain(&b[12..16])
        .chain(&b[20..21])
        .copied()
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let lanes = b[25..29]
        .iter()
        .chain(&b[34..36])
        .copied()
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    Ok((defaults, lanes))
}
