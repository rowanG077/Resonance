//! Parse original databases into validated, editable JSON records.
//! Resolve pointers and decode packed fields here; runtime schemas use their meaning.
use super::*;
use resonance_content::menu_data::{
    Item, ItemAttention, ItemUse, ItemView, MenuData, Technique, TechniqueUse, Title,
};
mod cooking;
mod customize;
mod ex_skills;
mod manual;
mod rename;
mod status;
mod strategy;
mod synopsis;
mod text;
mod world_map;

pub(super) fn cook(executable: &[u8], output: &Path) -> Result<()> {
    let half = |row: &[u8], at| u16::from_be_bytes(row[at..at + 2].try_into().unwrap());
    let string = |row: &[u8], at| -> Result<String> {
        let pointer = u32::from_be_bytes(row[at..at + 4].try_into()?);
        dol::text(executable, pointer)
    };
    let items: Vec<Item> = dol::slice(executable, 0x801fad98, 528 * 60)?
        .chunks_exact(60)
        .enumerate()
        .map(|(id, row)| {
            Ok(Item {
                attention: match id {
                    1 | 2 | 8 => Some(ItemAttention::LowHp),
                    3 | 4 | 9 => Some(ItemAttention::LowTp),
                    5..=7 => Some(ItemAttention::LowVitals),
                    10 => Some(ItemAttention::Ailment),
                    11 => Some(ItemAttention::Knockout),
                    _ => None,
                },
                field_usable: row[0x17] & 1 != 0,
                view: match id {
                    68 => Some(ItemView::TetheallaMap),
                    69 => Some(ItemView::SylvarantMap),
                    70 => Some(ItemView::CollectorsBook),
                    71 => Some(ItemView::MonsterList),
                    72 => Some(ItemView::FigurineBook),
                    73 => Some(ItemView::TrainingManual),
                    _ => None,
                },
                properties: status::properties(row)?,
                price: u32::from_be_bytes(row[4..8].try_into()?),
                transforms_to: half(row, 0x18),
                field_use: if row[0x17] & 1 == 0 {
                    None
                } else {
                    match id {
                        1..=9 => {
                            let (hp, tp, party) = [
                                (30, 0, false),
                                (60, 0, false),
                                (0, 30, false),
                                (0, 60, false),
                                (30, 30, false),
                                (60, 60, false),
                                (100, 100, false),
                                (30, 0, true),
                                (0, 30, true),
                            ][id - 1];
                            Some(ItemUse::Recover { hp, tp, party })
                        }
                        10 | 12 => Some(ItemUse::Cure),
                        11 => Some(ItemUse::Revive),
                        22 => Some(ItemUse::Transform),
                        23 | 24 => Some(ItemUse::EncounterRate {
                            rate: (id - 22) as u8,
                        }),
                        25..=36 => {
                            let index = (id - 25) % 6;
                            let strong = id >= 31;
                            Some(ItemUse::Herb {
                                stat: [1, 0, 2, 3, 6, 5][index],
                                amount: if index < 2 {
                                    if strong { 10 } else { 5 }
                                } else if strong {
                                    30
                                } else {
                                    10
                                },
                                percent: index < 2,
                            })
                        }
                        _ => None,
                    }
                },
                name: string(row, 0)?,
                description: string(row, 0x34)?,
                details: string(row, 0x38)?,
                category: row[0x1a],
                equipment_stats: [
                    half(row, 8) as i16,
                    half(row, 10) as i16,
                    half(row, 12) as i16,
                    row[14] as i8 as i16,
                    row[15] as i8 as i16,
                    row[16] as i8 as i16,
                    row[17] as i8 as i16,
                ],
            })
        })
        .collect::<Result<_>>()?;
    let techniques = dol::slice(
        executable,
        0x80202f90,
        resonance_content::menu_data::TECHNIQUE_COUNT * 0x58,
    )?
    .chunks_exact(0x58)
    .enumerate()
    .map(|(id, row)| {
        let (hp, party) = match id {
            98 | 221 => (30, false),
            99 | 117 => (45, true),
            100 => (60, false),
            118 => (30, true),
            119 => (100, false),
            120 => (60, true),
            122 => (70, true),
            192 => (25, false),
            193 => (35, false),
            194 => (45, false),
            _ => (0, false),
        };
        Ok(Technique {
            name: string(row, 0x10)?,
            description: string(row, 0x0c)?,
            tp: row[8],
            tp_percent: half(row, 0) == 34,
            unison_usable: u32::from_be_bytes(row[0x34..0x38].try_into()?) & 0x100 != 0,
            rank: row[0x14],
            element: row[0x15],
            route: row[0x17],
            level: half(row, 0x3e),
            prerequisite: half(row, 0x18),
            alternatives: std::array::from_fn(|i| half(row, 0x1e + i * 2)),
            field_use: if hp != 0 {
                Some(TechniqueUse::Recover { hp, party })
            } else {
                match id {
                    101 | 102 => Some(TechniqueUse::Cure { party: id == 102 }),
                    121 => Some(TechniqueUse::Revive),
                    _ => None,
                }
            },
        })
    })
    .collect::<Result<_>>()?;
    let starts: Vec<_> = dol::slice(executable, 0x80210920, 18)?
        .chunks_exact(2)
        .map(|b| half(b, 0))
        .chain([159])
        .collect();
    let titles = starts
        .windows(2)
        .map(|range| {
            (range[0]..range[1])
                .map(|index| {
                    let row = dol::slice(executable, 0x80210934 + u32::from(index) * 16, 16)?;
                    Ok(Title {
                        name: string(row, 0)?,
                        description: string(row, 4)?,
                        growth: row[8..15].try_into()?,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<_>>()?;
    let full_names = dol::slice(executable, 0x80211fb8, 36)?
        .chunks_exact(4)
        .map(|row| string(row, 0).map(|s| s.replace("%s", "{name}")))
        .collect::<Result<_>>()?;
    let mut labels: std::collections::BTreeMap<String, String> = [
        "status",
        "next",
        "strength",
        "defense",
        "slash",
        "accuracy",
        "attack",
        "thrust",
        "evasion",
        "intelligence",
        "luck",
        "weapon",
        "body",
        "head",
        "arm",
        "accessory_1",
        "accessory_2",
        "element_attack",
        "element_defense",
        "weak",
        "absorb",
        "invalid",
        "reduce",
        "growth",
        "growth_hp",
        "growth_tp",
        "growth_strength",
        "growth_defense",
        "growth_intelligence",
        "growth_evasion",
        "growth_accuracy",
    ]
    .into_iter()
    .zip(dol::slice(executable, 0x80199398, 31 * 4)?.chunks_exact(4))
    .map(|(key, row)| Ok((key.into(), string(row, 0)?)))
    .collect::<Result<_>>()?;
    for (key, index) in [
        ("item_slash", 4),
        ("item_thrust", 5),
        ("item_defense", 6),
        ("item_accuracy", 7),
        ("item_evasion", 8),
        ("item_intelligence", 9),
        ("item_luck", 10),
        ("item_attack", 11),
        ("discard", 1),
        ("transformed", 2),
        ("discarded", 3),
        ("confirm_discard", 26),
        ("select_item", 29),
        ("select_target", 31),
        ("equip_target", 33),
        ("transform_full", 34),
        ("transform_empty", 35),
        ("collectors_book", 73),
        ("holy_aura", 36),
        ("dark_aura", 37),
    ] {
        labels.insert(
            key.into(),
            string(dol::slice(executable, 0x8019d650 + index * 4, 4)?, 0)?,
        );
    }
    for (key, index) in [
        ("optimal", 7),
        ("remove", 8),
        ("change_order", 9),
        ("optimal_selection", 10),
        ("optimal_slash", 11),
        ("optimal_thrust", 12),
        ("alphabetical", 21),
        ("parameter", 22),
    ] {
        labels.insert(
            key.into(),
            string(dol::slice(executable, 0x8019d370 + index * 4, 4)?, 0)?,
        );
    }
    labels.insert(
        "stat_arrow".into(),
        string(&0x8035cee4u32.to_be_bytes(), 0)?,
    );
    labels.insert("preview_loading".into(), dol::text(executable, 0x801aa9f8)?);
    for (key, index) in [
        ("tech_usage", 2),
        ("tech_remove", 3),
        ("tech_auto", 4),
        ("tech_execute", 5),
        ("tech_forget", 6),
        ("tech_unison", 7),
        ("tech_control", 8),
        ("tech_manual", 9),
        ("tech_semi_auto", 10),
        ("tech_auto_mode", 11),
        ("tech_select", 12),
        ("tech_shortcut", 13),
        ("tech_unison_title", 14),
        ("tech_strength", 15),
        ("tech_slash", 16),
        ("tech_thrust", 17),
        ("tech_defense", 18),
        ("tech_luck", 19),
        ("tech_accuracy", 20),
        ("tech_evasion", 21),
        ("tech_intelligence", 22),
        ("tech_attack", 23),
        ("tech_target", 24),
        ("tech_target_all", 25),
        ("tech_cannot_forget", 26),
        ("tech_related", 27),
        ("tech_forget_warning", 28),
        ("tech_forget_confirm", 29),
    ] {
        labels.insert(
            key.into(),
            string(dol::slice(executable, 0x801aaf34 + index * 4, 4)?, 0)?,
        );
    }
    let item_categories = dol::slice(executable, 0x801ab2e0, 48 * 4)?
        .chunks_exact(4)
        .map(|row| string(row, 0))
        .collect::<Result<_>>()?;
    for (key, address) in [
        ("strategy_title", 0x801ab0d0u32),
        ("strategy_orders", 0x8035d680u32),
        ("strategy_rename", 0x8035d670),
        ("strategy_default", 0x8035d678),
        ("unison_title", 0x801aaf28),
        ("unison_player", 0x8035d5e0),
    ] {
        labels.insert(key.into(), string(&address.to_be_bytes(), 0)?);
    }
    for (key, offset) in [
        ("party_slash", 0x5c),
        ("party_thrust", 0x60),
        ("party_attack", 0x64),
        ("party_defense", 0x68),
        ("party_luck", 0x6c),
        ("party_accuracy", 0x70),
        ("party_evasion", 0x74),
        ("party_swap_target", 0x78),
        ("party_leader", 0x7c),
        ("party_swap", 0x80),
    ] {
        labels.insert(
            key.into(),
            string(dol::slice(executable, 0x801aaa28 + offset, 4)?, 0)?,
        );
    }
    let data = MenuData {
        version: MenuData::VERSION,
        item_group_prompt: text::paragraph(
            executable,
            u32::from_be_bytes(dol::slice(executable, 0x8019d6d0, 4)?.try_into()?),
        )?
        .0,
        item_bottle_count: text::paragraph_color(
            executable,
            u32::from_be_bytes(dol::slice(executable, 0x8019d6c8, 4)?.try_into()?),
            8,
        )?
        .0,
        ex_skills: ex_skills::cook(executable)?,
        figurines: crate::figurines::book(executable, output)?,
        monsters: crate::monsters::book(executable, output)?,
        manual: manual::cook(executable, &string)?,
        world_map: world_map::cook(executable, &string)?,
        status: status::cook(executable, &items, &string)?,
        strategy: strategy::cook(executable, &string)?,
        synopsis: synopsis::cook(executable, &string)?,
        cooking: cooking::cook(executable, &string)?,
        customize: customize::cook(executable, &string)?,
        techniques,
        items,
        titles,
        full_names,
        rename: rename::cook(executable)?,
        labels,
        item_categories,
        inventory_categories: dol::slice(executable, 0x801ab22c + 9 * 4, 9 * 4)?
            .chunks_exact(4)
            .map(|row| string(row, 0))
            .collect::<Result<_>>()?,
    };
    data.validate()?;
    write_atomic(
        &output.join("game/menu-data.json"),
        &serde_json::to_vec_pretty(&data)?,
    )
}
