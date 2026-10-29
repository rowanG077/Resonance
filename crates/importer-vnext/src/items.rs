//! Native item rows directly into the named item catalogue.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    items::{
        self, Action, Category, Characters, Description, EncounterAura, Entry, GrowthStat,
        Statistic,
    },
    library::Mappings,
    menu_data::{EquipmentProperties, ItemAttention, ItemText, ItemView},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::{ItemId, KnownId};

const ITEMS: u32 = 0x801fad98;
const COUNT: usize = 528;
const STRIDE: usize = 60;
const DESCRIPTIONS: u32 = 0x80199bd0;

pub fn cook_items(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let document = convert(&exe)?;
    let path = library.join(items::PATH);
    crate::output::json(&path, &document)?;
    Ok(())
}

fn caption(exe: &[u8], row: &[u8], at: usize) -> Result<String> {
    let pointer = dol::u32(row, at)?;
    if pointer == 0 {
        Ok(String::new())
    } else {
        text::string(exe, pointer)
    }
}

fn attention(id: usize) -> Option<ItemAttention> {
    use ItemAttention::*;
    match id {
        1 | 2 | 8 => Some(LowHp),
        3 | 4 | 9 => Some(LowTp),
        5..=7 => Some(LowVitals),
        10 => Some(Ailment),
        11 => Some(Knockout),
        12 => Some(AllAilments),
        13 => Some(MagicalAilment),
        _ => None,
    }
}

fn view(id: usize) -> Option<ItemView> {
    use ItemView::*;
    match id {
        68 => Some(TetheallaMap),
        69 => Some(SylvarantMap),
        70 => Some(CollectorsBook),
        71 => Some(MonsterList),
        72 => Some(FigurineBook),
        73 => Some(TrainingManual),
        _ => None,
    }
}

fn field_use(id: usize, flags: u8) -> Option<Action> {
    if flags & 1 == 0 {
        return None;
    }
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
            Some(Action::Recover { hp, tp, party })
        }
        10 | 12 => Some(Action::Cure),
        11 => Some(Action::Revive),
        22 => Some(Action::Transform),
        23 => Some(Action::EncounterRate {
            aura: EncounterAura::Holy,
        }),
        24 => Some(Action::EncounterRate {
            aura: EncounterAura::Dark,
        }),
        25..=36 => {
            let index = (id - 25) % 6;
            let strong = id >= 31;
            let percent = index < 2;
            let amount = if percent {
                if strong { 10. } else { 5. }
            } else if strong {
                3.
            } else {
                1.
            };
            let stat = [
                GrowthStat::MaxTp,
                GrowthStat::MaxHp,
                GrowthStat::Strength,
                GrowthStat::Defense,
                GrowthStat::Accuracy,
                GrowthStat::Evasion,
            ][index];
            Some(Action::Herb {
                stat,
                amount,
                percent,
            })
        }
        _ => None,
    }
}

fn convert(exe: &[u8]) -> Result<items::Document> {
    let mappings = Mappings::new(
        ItemId::ALL
            .iter()
            .filter_map(|&id| id.resource_name().map(|name| (id.value(), name.to_owned())))
            .collect(),
    )?;
    ensure!(
        mappings.iter().count() == COUNT,
        "item names do not cover native table"
    );
    let mut entries = BTreeMap::new();
    let mut used_descriptions = BTreeSet::new();
    for (id, row) in dol::slice(exe, ITEMS, COUNT * STRIDE)?
        .chunks_exact(STRIDE)
        .enumerate()
    {
        let category = Category::from_index(row[0x1a])?;
        let raw_mask = u16::from(row[0x15]);
        let mut mask = raw_mask | ((raw_mask & 0x20) << 3);
        if [236, 284, 327, 363].contains(&id) {
            mask &= !0x20;
        }
        let properties = properties(id, row)?;
        used_descriptions.extend(properties.descriptions.iter().cloned());
        let stats = Statistic::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(index, stat)| {
                let value = if index < 3 {
                    i16::from_be_bytes([row[8 + index * 2], row[9 + index * 2]])
                } else {
                    i16::from(row[11 + index] as i8)
                };
                (value != 0).then_some((stat, value))
            })
            .collect();
        let transform = dol::u16(row, 0x18)?;
        let price = u32::try_from(dol::u32(row, 4)? as i32).context("negative item price")?;
        let entry = Entry {
            category,
            characters: Characters::from_mask(mask)?,
            stack_limit: None,
            field_usable: row[0x17] & 1 != 0,
            battle_usable: row[0x17] & 2 != 0,
            view: view(id),
            stats,
            properties,
            price,
            transforms_to: (transform != 0)
                .then(|| mappings.target(u32::from(transform)))
                .transpose()?
                .map(str::to_owned),
            field_use: field_use(id, row[0x17]),
            attention: attention(id),
            text: serde_json::to_value(ItemText {
                name: caption(exe, row, 0)?,
                description: caption(exe, row, 0x34)?,
                details: caption(exe, row, 0x38)?,
            })?,
        };
        ensure!(
            entries
                .insert(mappings.target(id as u32)?.to_owned(), entry)
                .is_none(),
            "duplicate item name"
        );
    }
    let equipment_descriptions = descriptions(exe, &used_descriptions)?;
    Ok(items::Document {
        mappings,
        items: entries,
        equipment_descriptions,
    })
}

fn descriptions(exe: &[u8], used: &BTreeSet<String>) -> Result<BTreeMap<String, Description>> {
    let overrides = dol::slice(exe, 0x8035c93c, 8)?;
    let mut result = BTreeMap::new();
    for name in used {
        let id = (0..109u8)
            .find(|&id| effect_name(id).ok().flatten() == Some(name.as_str()))
            .with_context(|| format!("missing source equipment description {name}"))?;
        let mut suppresses = Vec::new();
        for target in overrides
            .chunks_exact(2)
            .filter_map(|pair| (pair[0] == id).then_some(pair[1]))
            .chain(
                (id == 12)
                    .then_some([1, 3, 4, 5, 6, 7, 9, 10])
                    .into_iter()
                    .flatten(),
            )
        {
            if let Some(target) = effect_name(target)?
                && used.contains(target)
            {
                suppresses.push(target.to_owned());
            }
        }
        let pointer = dol::u32(dol::slice(exe, DESCRIPTIONS + u32::from(id) * 4, 4)?, 0)?;
        result.insert(
            name.clone(),
            Description::new(text::string(exe, pointer)?, suppresses),
        );
    }
    Ok(result)
}

fn effect_name(id: u8) -> Result<Option<&'static str>> {
    if [0, 6, 33, 38, 39, 84, 85, 86, 87].contains(&id) {
        return Ok(None);
    }
    let names = [
        (1, "poison-immunity"),
        (2, "stun-immunity"),
        (3, "paralysis-immunity"),
        (4, "weakness-immunity"),
        (5, "petrification-immunity"),
        (7, "curse-immunity"),
        (9, "reduced-item-effect-immunity"),
        (10, "heavy-immunity"),
        (12, "physical-ailment-immunity"),
        (13, "physical-ailment-resistance"),
        (14, "shorter-stun"),
        (47, "faster-casting"),
        (48, "third-tp-cost"),
        (49, "half-tp-cost"),
        (50, "small-hp-regeneration"),
        (51, "small-tp-regeneration"),
        (52, "large-hp-regeneration"),
        (53, "large-tp-regeneration"),
        (60, "hp-on-defeat"),
        (61, "tp-on-defeat"),
        (64, "increased-physical-damage"),
        (65, "reduced-physical-damage"),
        (68, "increased-magic-damage"),
        (73, "faster-movement"),
        (74, "slower-movement"),
        (79, "experience-bonus-50-percent"),
        (80, "experience-bonus-100-percent"),
        (81, "gald-bonus-50-percent"),
        (82, "gald-bonus-100-percent"),
        (83, "faster-escape"),
        (97, "plant-damage"),
        (98, "undead-damage"),
        (99, "beast-damage"),
        (100, "magical-damage"),
        (101, "machine-damage"),
        (102, "human-damage"),
        (103, "bird-damage"),
        (104, "insect-damage"),
        (105, "aquatic-damage"),
        (106, "immaterial-damage"),
        (107, "dragon-damage"),
        (108, "angel-damage"),
    ];
    Ok(Some(
        names
            .into_iter()
            .find_map(|(value, name)| (value == id).then_some(name))
            .with_context(|| format!("unidentified equipment description {id}"))?,
    ))
}

fn properties(id: usize, row: &[u8]) -> Result<EquipmentProperties> {
    use resonance_content::{
        battle_profile::Species,
        menu_data::{
            Element, EquipmentAilment as Ailment, EquipmentRescue, ExStat, ExStatBonus, TpDiscount,
        },
    };
    ensure!(row[0x13] <= 8, "invalid weapon element");
    let primary = row[0x14];
    let effects = [row[0x26], row[0x28], row[0x2a], row[0x2c]];
    let species = [
        Species::Plant,
        Species::Undead,
        Species::Beast,
        Species::Magical,
        Species::Machine,
        Species::Human,
        Species::Bird,
        Species::Insect,
        Species::Aquatic,
        Species::Immaterial,
        Species::Dragon,
        Species::Angel,
    ];
    let mut value = EquipmentProperties {
        attack_element: row[0x13]
            .checked_sub(1)
            .map(|index| Element::ALL[index as usize]),
        neutral_resistance: row[0x1b] as i8,
        resistance: Element::ALL
            .into_iter()
            .zip(&row[0x1c..0x24])
            .filter_map(|(element, &raw)| {
                let amount = raw as i8;
                (amount != 0).then_some((element, amount))
            })
            .collect(),
        critical_chance_bonus: row[0x12],
        technique_drift: row[0x31] as i8,
        species_bonus: (97..=108)
            .contains(&primary)
            .then(|| species[(primary - 97) as usize]),
        physical_damage_boost: id == 415,
        physical_damage_reduction: id == 416,
        magic_damage_boost: id == 417,
        kills_damage: [158, 174, 189, 204, 218, 227, 241, 257, 273].contains(&id),
        rescue: match id {
            414 => Some(EquipmentRescue::Chance),
            457 => Some(EquipmentRescue::Consumable),
            _ => None,
        },
        descriptions: std::iter::once(primary)
            .chain(effects)
            .map(effect_name)
            .filter_map(|result| match result {
                Ok(Some(name)) => Some(Ok(name.to_owned())),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>>>()?,
        ..Default::default()
    };
    if let Some((stat, percent)) = match id {
        454 => Some((ExStat::MaxHp, 30)),
        455 => Some((ExStat::MaxTp, 30)),
        419 => Some((ExStat::Defense, 10)),
        399 => Some((ExStat::Defense, 15)),
        398 => Some((ExStat::Defense, 5)),
        418 => Some((ExStat::Strength, 10)),
        _ => None,
    } {
        value.stat_bonuses.push(ExStatBonus { stat, percent });
    }
    // Primary and active effect slots have different meanings for some IDs.
    for (effect, active) in
        std::iter::once((primary, false)).chain(effects.into_iter().map(|effect| (effect, true)))
    {
        match effect {
            0 => {}
            81 => value.gald_one_and_a_half = true,
            82 => value.gald_double = true,
            83 => value.quick_escape = true,
            1..=5 | 7 | 10 | 12..=14 | 39 | 47..=53 | 60..=61 | 86..=87 if !active => {}
            1 => {
                value.immunities.insert(Ailment::Poison);
            }
            2 => {
                value.immunities.insert(Ailment::Stun);
            }
            3 => {
                value.immunities.insert(Ailment::Paralysis);
            }
            4 => {
                value.immunities.insert(Ailment::Weak);
            }
            5 => {
                value.immunities.insert(Ailment::Petrify);
            }
            7 => {
                value.immunities.insert(Ailment::Curse);
            }
            10 => {
                value.immunities.insert(Ailment::Heavy);
            }
            12 => {
                value.immunities.insert(Ailment::PhysicalAilments);
            }
            13 => value.ailment_resistance = true,
            14 => value.short_stun = true,
            39 => {
                value.ailments.insert(Ailment::Heavy);
            }
            47 => value.faster_casting = true,
            48 => value.tp_discount = value.tp_discount.max(TpDiscount::Third),
            49 => value.tp_discount = TpDiscount::Half,
            50 => value.hp_regeneration += 1,
            51 => value.tp_regeneration += 1,
            52 => value.hp_regeneration += 3,
            53 => value.tp_regeneration += 3,
            60 => value.kill_hp_recovery = true,
            61 => value.kill_tp_recovery = true,
            73 if active => value.movement_bonus += 1,
            74 if active => value.movement_bonus -= 1,
            86 => {
                value.ailments.insert(Ailment::Curse);
                value.defense_halved = true;
                value.experience_percent = value.experience_percent.max(50);
            }
            87 => {
                value
                    .ailments
                    .extend([Ailment::Poison, Ailment::Curse, Ailment::PhysicalAilments]);
                value.experience_percent = 100;
            }
            33 | 38 | 64 | 65 | 68 | 73 | 74 | 84 | 85 | 97..=108 => {}
            _ => value.unsupported_modifier = true,
        }
    }
    Ok(value)
}
