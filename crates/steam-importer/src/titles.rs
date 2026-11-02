//! Native title growth, localized captions and costume/award bindings.
use crate::{output, pe, source::Source, text::Bank};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    library::Mappings,
    overworld::PartyCharacter,
    titles::{Awards, Document, Entry, Statistic},
};
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = Bank::parse(&bytes)?;
    let common: BTreeMap<PartyCharacter, Mappings> =
        serde_json::from_str(include_str!("../../importer/src/title-names.json"))?;
    let mut shared_names = BTreeMap::<String, usize>::new();
    for (_, name) in common.values().flat_map(Mappings::iter) {
        *shared_names.entry(name.to_owned()).or_default() += 1;
    }
    let costume_names: BTreeMap<u8, BTreeMap<u8, CostumeName>> =
        serde_json::from_str(include_str!("../../importer/src/costume-names.json"))?;
    let starts = image.slice(0x8d849c, 9 * 2)?;
    let rows = image.slice(0x8dee50, 177 * 16)?;
    let directory = |slot| pe::word(image.slice(0x8b8800 + slot * 4, 4)?, 0);
    let caption = |slot, index| -> Result<Option<String>> {
        let id = pe::word(image.slice(directory(slot)? + index * 4, 4)?, 0)?;
        if id == 0 {
            return Ok(None);
        }
        Ok(Some(bank.menu(id, encoding_rs::WINDOWS_1252)?))
    };
    let bindings = costumes(&image)?;
    let mut mappings = BTreeMap::new();
    let mut titles = BTreeMap::new();
    for character in 1..=9u8 {
        let member = PartyCharacter::try_from(character)?;
        let start = pe::half(starts, usize::from(character - 1) * 2)? as usize;
        let end = if character == 9 {
            177
        } else {
            pe::half(starts, usize::from(character) * 2)? as usize
        };
        ensure!(start < end && end <= 177, "invalid native title range");
        let known = &common[&member];
        ensure!(
            end - start == known.iter().count() + 2,
            "unexpected added title count"
        );
        let owner = symphonia_constants::PartyCharacterId::try_from(character)?
            .resource_name()
            .context("unnamed party member")?;
        let mut names = BTreeMap::new();
        for (index, row) in rows[start * 16..end * 16].chunks_exact(16).enumerate() {
            let id = index as u32 + 1;
            // Native title IDs can move when Steam inserts an outfit title.
            // Name the actual English row, then retain that ID in the mapping.
            let label = caption(1, pe::word(row, 0)?)?;
            let name = if let Some(label) = label {
                let slug = label
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("-")
                    .to_ascii_lowercase();
                let slug = if slug.is_empty() {
                    format!("title-{id}")
                } else {
                    slug
                };
                if let Some((_, name)) = known.iter().find(|(_, name)| *name == slug) {
                    if shared_names[name] > 1 {
                        format!("{owner}/{name}")
                    } else {
                        name.to_owned()
                    }
                } else {
                    format!("{owner}/{slug}")
                }
            } else {
                format!("unused-{owner}-{id}")
            };
            let costume = bindings
                .get(&(character, id))
                .map(|&variant| -> Result<String> {
                    if variant >= 5 {
                        Ok(format!("costume-{variant}"))
                    } else {
                        Ok(costume_names
                            .get(&character)
                            .and_then(|names| names.get(&variant))
                            .context("unnamed title costume")?
                            .name
                            .clone())
                    }
                })
                .transpose()?;
            let growth = Statistic::ALL
                .into_iter()
                .zip(&row[8..15])
                .filter(|(_, amount)| **amount != 0)
                .map(|(stat, &amount)| {
                    (
                        stat,
                        f64::from(amount)
                            / if matches!(stat, Statistic::Hp | Statistic::Tp) {
                                1.
                            } else {
                                10.
                            },
                    )
                })
                .collect();
            let label = caption(source.language.slot(), pe::word(row, 0)?)?;
            let description = caption(source.language.slot(), pe::word(row, 4)?)?;
            let text = label.map(|name| serde_json::json!({"name":name,"description":description.unwrap_or_default()})).unwrap_or_default();
            let entry = Entry {
                growth,
                costume,
                text,
            };
            if let Some(previous) = titles.insert(name.clone(), entry.clone()) {
                ensure!(
                    previous == entry,
                    "shared title {name} differs between characters"
                );
            }
            names.insert(id, name);
        }
        mappings.insert(member, Mappings::new(names)?);
    }
    let items: resonance_content::items::Document =
        serde_json::from_slice(&fs::read(library.join(resonance_content::items::PATH))?)?;
    let awards = Awards::from_parts(&awards(&image)?, &mappings, &items.mappings)?;
    output::json(
        &library.join(resonance_content::titles::PATH),
        &Document {
            mappings,
            titles,
            awards,
        },
    )
}

#[derive(serde::Deserialize)]
struct CostumeName {
    name: String,
}

fn costumes(image: &pe::Image<'_>) -> Result<BTreeMap<(u8, u32), u8>> {
    let immediate = |at| -> Result<i16> {
        let bytes = image.slice(at, 5)?;
        ensure!(bytes[0] == 0xba, "unknown costume title immediate");
        Ok(i16::try_from(pe::word(bytes, 1)?)?)
    };
    let mut titles = [
        [immediate(0x528366)?; 9],
        [immediate(0x52838f)?; 9],
        [immediate(0x5283bb)?; 9],
        [immediate(0x5283e7)?; 9],
        [0; 9],
    ];
    titles[1][8] = -1;
    titles[2][8] = -1;
    titles[3][8] = immediate(0x52840c)?;
    for (i, address) in [
        0x528415, 0x528421, 0x52842a, 0x52843b, 0x52843b, 0x528448, 0x528451, 0x528451, 0x528464,
    ]
    .into_iter()
    .enumerate()
    {
        titles[4][i] = immediate(address)?;
    }
    let mut variants = image.slice(0x5284d5, 4)?.to_vec();
    variants.push(image.slice(0x5284dc, 1)?[0]);
    ensure!(variants == [1, 2, 4, 5, 6], "unknown title costume slots");
    let mut result = BTreeMap::new();
    for (row, variant) in titles.into_iter().zip(variants) {
        for (index, title) in row.into_iter().enumerate() {
            if title < 0 {
                continue;
            }
            ensure!(
                title > 0
                    && result
                        .insert((index as u8 + 1, title as u32), variant)
                        .is_none(),
                "invalid or repeated title costume binding"
            );
        }
    }
    Ok(result)
}

use resonance_content::menu_data::{
    ComboTitleAward, EquipmentRequirement, EquipmentTitleAward, LevelTitleAward, TitleAwardRules,
};
fn call(image: &pe::Image<'_>, address: u32, target: u32) -> Result<()> {
    let bytes = image.slice(address, 5)?;
    ensure!(
        bytes[0] == 0xe8 && address.wrapping_add(5).wrapping_add(pe::word(bytes, 1)?) == target,
        "Steam title award call changed at {address:#x}"
    );
    Ok(())
}

/// Title arguments pack a zero-based character byte and one-based title byte.
fn title(image: &pe::Image<'_>, address: u32) -> Result<(u8, u8, u32)> {
    let bytes = image.slice(address, 5)?;
    let (packed, size) = match bytes[0] {
        0x6a => (u32::from(bytes[1]), 2),
        0x68 => (pe::word(bytes, 1)?, 5),
        _ => anyhow::bail!("Steam title argument changed at {address:#x}"),
    };
    ensure!(
        packed >> 8 < 9 && (1..32).contains(&(packed & 255)),
        "invalid Steam title argument"
    );
    Ok(((packed >> 8) as u8 + 1, packed as u8, size))
}

fn awards(image: &pe::Image<'_>) -> Result<TitleAwardRules> {
    let mut rules = TitleAwardRules::default();
    let mut address = 0x48475c;
    for _ in 0..22 {
        let bytes = image.slice(address, 16)?;
        ensure!(
            bytes[0] == 0x6a
                && bytes[2] == 0xb2
                && bytes[9..15] == [0x83, 0xc4, 4, 0x85, 0xc0, 0x74],
            "Steam level title condition changed"
        );
        call(image, address + 4, 0x484a00)?;
        let (character, title, size) = title(image, address + 16)?;
        ensure!(
            character == bytes[1]
                && image.slice(address + 16 + size, 2)? == [0x6a, character]
                && u32::from(bytes[15]) == size + 10,
            "Steam level title recipient changed"
        );
        call(image, address + 18 + size, 0x4629f0)?;
        rules.levels.push(LevelTitleAward {
            character,
            title,
            minimum: bytes[3],
        });
        address += size + 26;
    }
    ensure!(
        address == 0x4849fd && image.slice(address, 1)? == [0xc3],
        "Steam level title bounds changed"
    );
    for address in [0x464665, 0x46467e, 0x464697, 0x4646b0] {
        let bytes = image.slice(address, 13)?;
        ensure!(
            bytes[..6] == [0x83, 0xbb, 0xb4, 2, 0, 0]
                && bytes[7..] == [0x7c, 0x10, 0x6a, 0, 0x6a, 0],
            "Steam combo title condition changed"
        );
        let (character, title, size) = title(image, address + 13)?;
        ensure!(
            image.slice(address + 13 + size, 2)? == [0xb2, character],
            "Steam combo title recipient changed"
        );
        call(image, address + 15 + size, 0x462920)?;
        rules.combos.push(ComboTitleAward {
            character,
            title,
            minimum: u16::from(bytes[6]),
        });
    }
    let item = |address, opcode| -> Result<u16> {
        let bytes = image.slice(address, 5)?;
        ensure!(bytes[0] == opcode, "Steam equipment title item changed");
        Ok(pe::word(bytes, 1)?.try_into()?)
    };
    let (character, equipment_title, size) = title(image, 0x48474d)?;
    call(image, 0x48474d + size + 2, 0x462920)?;
    rules.equipment.push(EquipmentTitleAward {
        character,
        title: equipment_title,
        requirements: vec![
            EquipmentRequirement {
                slots: vec![0],
                items: vec![item(0x4846fd, 0xb9)?],
            },
            EquipmentRequirement {
                slots: vec![1],
                items: vec![item(0x484708, 0xba)?],
            },
            EquipmentRequirement {
                slots: vec![2],
                items: vec![item(0x484713, 0xb9)?],
            },
            EquipmentRequirement {
                slots: vec![3, 4],
                items: vec![item(0x48471e, 0xba)?],
            },
            EquipmentRequirement {
                slots: vec![5],
                items: vec![item(0x484735, 0xba)?, item(0x48473f, 0xb9)?],
            },
        ],
    });
    Ok(rules)
}
