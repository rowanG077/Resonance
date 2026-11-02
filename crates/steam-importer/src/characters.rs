//! Native party defaults directly into named characters and shared growth.
use crate::{pe, source::Source, text::Bank};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    characters::{self, Direction, Entry, Gain, Gem, Growth, GrowthTable, Initial},
    library::Mappings,
    overworld::PartyCharacter,
    titles::{Slot, Statistic},
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::{CharacterId, KnownId};

const CHARACTERS: u32 = 0x8cf128;
const STRIDE: usize = 280;
const EXPERIENCE: u32 = 0x8dffc8;
const GROWTH: u32 = 0x8e03b8;

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = Bank::parse(&bytes)?;
    let items: resonance_content::items::Document =
        serde_json::from_slice(&fs::read(library.join(resonance_content::items::PATH))?)?;
    let techniques: resonance_content::arte::document::Document =
        serde_json::from_slice(&fs::read(library.join(resonance_content::arte::PATH))?)?;
    let ex = serde_json::from_slice(&fs::read(library.join(resonance_content::ex_skills::PATH))?)?;
    let document = convert(
        &image,
        &bank,
        source.language.slot(),
        &items.mappings,
        &techniques.mappings,
        &ex,
    )?;
    crate::output::json(&library.join(characters::PATH), &document)
}

fn signed(row: &[u8], at: usize) -> Result<i16> {
    Ok(pe::half(row, at)? as i16)
}
fn scale(stat: Statistic) -> f64 {
    if matches!(stat, Statistic::Hp | Statistic::Tp) {
        1.
    } else {
        10.
    }
}

fn convert(
    image: &pe::Image<'_>,
    bank: &Bank<'_>,
    slot: u32,
    items: &Mappings,
    techniques: &Mappings,
    ex: &resonance_content::ex_skills::Document,
) -> Result<characters::Document> {
    let mappings = Mappings::new(
        CharacterId::ALL
            .iter()
            .filter_map(|&id| id.resource_name().map(|name| (id.value(), name.to_owned())))
            .collect(),
    )?;
    ensure!(mappings.iter().count() == 10, "missing character names");
    let recipes: Mappings =
        serde_json::from_str(include_str!("../../importer/src/recipe-names.json"))?;
    let rows = image.slice(CHARACTERS, 11 * STRIDE)?;
    let learning = image.slice(0x8d82c0, 11 * 41)?;
    let growth_rows = image.slice(GROWTH, 9 * 14)?;
    let names = pe::word(image.slice(0x8cf10c + slot * 4, 4)?, 0)?;
    let formats = pe::word(image.slice(0x8b87a0 + slot * 4, 4)?, 0)?;
    let caption = |table: u32, id: usize| {
        bank.menu(
            pe::word(image.slice(table + id as u32 * 4, 4)?, 0)?,
            encoding_rs::WINDOWS_1252,
        )
    };
    let mut characters = BTreeMap::new();
    for id in 1..=10usize {
        let owner = mappings.target(id as u32)?;
        let (name, full_name, initial, growth) = if id == 10 {
            (
                image.text(0x87094c)?.to_owned(),
                serde_json::Value::Null,
                None,
                None,
            )
        } else {
            let index = id - 1;
            let row = &rows[index * STRIDE..(index + 1) * STRIDE];
            let name = caption(names, index)?;
            let format = caption(formats, index)?.replace("%s", "{name}");
            let member = PartyCharacter::try_from(id as u8)?;
            let initial = initial(
                row,
                &learning[index * 41..(index + 1) * 41],
                member,
                items,
                techniques,
                &recipes,
                ex,
            )?;
            let growth = growth(&growth_rows[index * 14..(index + 1) * 14]);
            (
                name,
                format.into(),
                Some(initial),
                Some(Growth::Inline(growth)),
            )
        };
        ensure!(
            !name.is_empty() && !name.chars().any(char::is_control),
            "invalid character name {id}"
        );
        characters.insert(
            owner.to_owned(),
            Entry {
                name,
                full_name,
                initial,
                growth,
            },
        );
    }
    let shared = characters["zelos"]
        .growth
        .as_ref()
        .context("Zelos has no growth")?;
    ensure!(
        Some(shared) == characters["kratos"].growth.as_ref(),
        "magic swordsmen have different growth"
    );
    let Growth::Inline(shared) = shared else {
        anyhow::bail!("unexpected shared growth")
    };
    let mut growth_profiles = BTreeMap::new();
    growth_profiles.insert("magic-swordsman".to_owned(), shared.clone());
    for name in ["zelos", "kratos"] {
        characters.get_mut(name).unwrap().growth = Some(Growth::Shared("magic-swordsman".into()));
    }
    let experience = image
        .slice(EXPERIENCE, 252 * 4)?
        .chunks_exact(4)
        .enumerate()
        .skip(1)
        .map(|(level, row)| Ok((level as u8, pe::word(row, 0)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(characters::Document {
        mappings,
        characters,
        experience,
        growth_profiles,
    })
}

fn growth(row: &[u8]) -> GrowthTable {
    Statistic::ALL
        .into_iter()
        .enumerate()
        .filter_map(|(index, stat)| {
            let base = row[index * 2];
            let random = row[index * 2 + 1];
            if base == 0 && random == 0 {
                return None;
            }
            let scale = scale(stat);
            let gain = if random == 0 {
                Gain::Fixed(f64::from(base) / scale)
            } else {
                Gain::Range([
                    f64::from(base) / scale,
                    f64::from(u16::from(base) + u16::from(random)) / scale,
                ])
            };
            Some((stat, gain))
        })
        .collect()
}

fn initial(
    row: &[u8],
    learning: &[u8],
    character: PartyCharacter,
    items: &Mappings,
    techniques: &Mappings,
    recipes: &Mappings,
    ex: &resonance_content::ex_skills::Document,
) -> Result<Initial> {
    let base = [0x26, 0x28, 0x2a, 0x2c, 0x34, 0x32, 0x30];
    let stats = Statistic::ALL
        .into_iter()
        .zip(base)
        .map(|(stat, offset)| Ok((stat, signed(row, offset)? as u16)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|(_, value)| *value != 0)
        .map(|(stat, value)| (stat, f64::from(value) / scale(stat)))
        .collect();
    // Native order: weapon, body, head, shield, accessory 1, accessory 2.
    let mut equipment = BTreeMap::new();
    for (slot, index) in Slot::ALL.into_iter().zip([0, 1, 2, 4, 5, 3]) {
        let id = signed(row, 0x4a + index * 2)? as u16;
        if id != 0 {
            equipment.insert(slot, items.target(u32::from(id))?.to_owned());
        }
    }
    let allowed = learning
        .get(1..1 + usize::from(learning[0]))
        .context("character learning list exceeds native capacity")?;
    let learned = u64::from_le_bytes(row[0x70..0x78].try_into()?);
    ensure!(
        allowed.len() <= 64,
        "character learning mask exceeds 64 bits"
    );
    let learned_techniques = allowed
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(slot, id)| {
            (learned & (1 << slot) != 0)
                .then(|| Ok(crate::techniques::source_name(techniques, u32::from(id))?.to_owned()))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut shortcuts = BTreeMap::new();
    for (index, direction) in [
        Direction::Neutral,
        Direction::Up,
        Direction::Down,
        Direction::Side,
    ]
    .into_iter()
    .enumerate()
    {
        let id = signed(row, 0xd8 + index * 2)? as u16;
        if id != 0 {
            shortcuts.insert(
                direction,
                crate::techniques::source_name(techniques, u32::from(id))?.to_owned(),
            );
        }
    }
    let cooking = row[0xf4..0x10c]
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, mastery)| *mastery != 0)
        .map(|(id, mastery)| Ok((recipes.target(id as u32)?.to_owned(), mastery)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut ex_gems = BTreeMap::new();
    for slot in 0..4 {
        let level = row[0xea + slot];
        let skill = row[0xee + slot];
        if level == 0 && skill == 0 {
            continue;
        }
        ex_gems.insert(
            slot as u8 + 1,
            Gem {
                level,
                skill: (skill != 0)
                    .then(|| ex.mappings.target(u32::from(skill)).map(str::to_owned))
                    .transpose()?,
            },
        );
    }
    let compound = ex.compounds(character)?;
    let compound_names = |at| -> Result<Vec<String>> {
        let bits = pe::word(row, at)?;
        (0..24)
            .filter(|&id| bits & (1 << id) != 0)
            .map(|id| Ok(compound.target(id)?.to_owned()))
            .collect()
    };
    Ok(Initial {
        stats,
        equipment,
        level: row[0x10],
        luck: (signed(row, 0x2e)? as u16 / 10).min(255) as u8,
        experience: pe::word(row, 0x18)?,
        affinity: pe::word(row, 0x58)? as i32,
        overlimit: row[0x56],
        technique_balance: row[0x10c] as i8,
        techniques: learned_techniques,
        shortcuts,
        cooking,
        ex_gems,
        compound_ex_skills: compound_names(0x110)?,
        recent_compound_ex_skills: compound_names(0x114)?,
    })
}
