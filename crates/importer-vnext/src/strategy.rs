//! Native strategy tables directly into named orders and formation choices.
use crate::{dol, text};
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
use std::{collections::BTreeMap, fs, path::Path};

const GROUPS: [(u32, usize); 3] = [(0x80208c3c, 144), (0x80208ccc, 144), (0x80208d5c, 116)];
const PRESETS: u32 = 0x80208b88;
const KEYBOARD: u32 = 0x801ab150;

#[derive(Deserialize)]
struct Names {
    groups: BTreeMap<Group, Mappings>,
    presets: Mappings,
}

pub fn cook_strategy(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let document = convert(&exe)?;
    let path = library.join(strategy::PATH);
    crate::output::json(&path, &document)?;
    Ok(())
}

fn convert(exe: &[u8]) -> Result<Document> {
    let names: Names = serde_json::from_str(include_str!("strategy-names.json"))?;
    let lanes = dol::slice(exe, 0x8035c044, 6)?;
    let mut groups = BTreeMap::new();
    for (index, (address, extent)) in GROUPS.into_iter().enumerate() {
        let group = Group::ALL[index];
        ensure!(
            dol::u32(dol::slice(exe, 0x801ab144 + index as u32 * 4, 4)?, 0)? == address,
            "strategy table reference differs from its source location"
        );
        let count = dol::u32(dol::slice(exe, 0x801ab138 + index as u32 * 4, 4)?, 0)? as usize;
        ensure!(
            count == [9, 9, 7][index] && count * 16 <= extent,
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
        for (id, row) in dol::slice(exe, address, count * 16)?
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
                .map(|offset| text::string(exe, dol::u32(row, offset)?))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect();
            let choice = Choice {
                text: serde_json::to_value(Caption {
                    name: text::string(exe, dol::u32(row, 0)?)?,
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
                name: serde_json::to_value(text::pointer(exe, 0x801ab12c + index as u32 * 4)?)?,
                options,
            },
        );
    }

    let default_ids = dol::slice(exe, 0x8018adf4, 9)?;
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
    for (id, source) in dol::slice(exe, PRESETS, 3 * 60)?
        .chunks_exact(60)
        .enumerate()
    {
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
                name: serde_json::to_value(text::string(exe, PRESETS + id as u32 * 60)?)?,
                default: Orders {
                    action,
                    skill_magic,
                    position,
                },
                characters,
            },
        );
    }

    let keyboard = dol::slice(exe, KEYBOARD, 90)?;
    ensure!(keyboard.is_ascii(), "non-ASCII strategy keyboard");
    let keys = text::table::<9>(exe, KEYBOARD + 92)?;
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
    ] = keys;
    let rows = std::array::from_fn(|row| {
        String::from_utf8(keyboard[row * 10..row * 10 + 10].to_vec()).unwrap()
    });
    let labels = text::table::<6>(exe, 0x801ab0fc)?;
    Ok(Document {
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
    })
}
