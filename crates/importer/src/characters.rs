//! Native party defaults directly into named characters and shared growth.
use crate::{dol, ex_skills, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    characters::{self, Direction, Entry, Gain, Gem, Growth, GrowthTable, Initial},
    library::Mappings,
    overworld::PartyCharacter,
    titles::{Slot, Statistic},
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::{CharacterId, ItemId, KnownId};

const CHARACTERS: u32 = 0x801f9fc8;
const STRIDE: usize = 280;
const EXPERIENCE: u32 = 0x80202958;
const GROWTH: u32 = 0x80202d48;

pub fn cook_characters(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let document = convert(&exe)?;
    let path = library.join(characters::PATH);
    crate::output::json(&path, &document)?;
    Ok(())
}

fn signed(row: &[u8], at: usize) -> Result<i16> {
    Ok(dol::u16(row, at)? as i16)
}
fn scale(stat: Statistic) -> f64 {
    if matches!(stat, Statistic::Hp | Statistic::Tp) {
        1.
    } else {
        10.
    }
}

fn source_name(row: &[u8]) -> Result<String> {
    let end = row[..13]
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated character name")?;
    let bytes = &row[..end];
    let (name, _, invalid) = encoding_rs::SHIFT_JIS.decode(bytes);
    ensure!(!invalid, "invalid character name encoding");
    let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(&name);
    ensure!(
        !invalid && encoded.as_ref() == bytes,
        "non-roundtrippable character name"
    );
    Ok(name.into_owned())
}

fn convert(exe: &[u8]) -> Result<characters::Document> {
    let mappings = Mappings::new(
        CharacterId::ALL
            .iter()
            .filter_map(|&id| id.resource_name().map(|name| (id.value(), name.to_owned())))
            .collect(),
    )?;
    ensure!(mappings.iter().count() == 10, "missing character names");
    let items = Mappings::new(
        ItemId::ALL
            .iter()
            .filter_map(|&id| id.resource_name().map(|name| (id.value(), name.to_owned())))
            .collect(),
    )?;
    let techniques: Mappings = serde_json::from_str(include_str!("technique-names.json"))?;
    let recipes: Mappings = serde_json::from_str(include_str!("recipe-names.json"))?;
    let ex = ex_skills::convert(exe)?;
    let rows = dol::slice(exe, CHARACTERS, 11 * STRIDE)?;
    let learning = dol::slice(exe, 0x80202dc8, 11 * 41)?;
    let growth_rows = dol::slice(exe, GROWTH, 9 * 14)?;
    let full_name_formats = dol::slice(exe, 0x80211fb8, 9 * 4)?;
    let mut characters = BTreeMap::new();
    for id in 1..=10usize {
        let owner = mappings.target(id as u32)?;
        let (name, full_name, initial, growth) = if id == 10 {
            (
                text::string(exe, 0x8035bb80)?,
                serde_json::Value::Null,
                None,
                None,
            )
        } else {
            let index = id - 1;
            let row = &rows[index * STRIDE..(index + 1) * STRIDE];
            let name = source_name(row)?;
            let format =
                text::string(exe, dol::u32(full_name_formats, index * 4)?)?.replace("%s", "{name}");
            let member = PartyCharacter::try_from(id as u8)?;
            let initial = initial(
                row,
                &learning[index * 41..(index + 1) * 41],
                member,
                &items,
                &techniques,
                &recipes,
                &ex,
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
    let experience = dol::slice(exe, EXPERIENCE, 252 * 4)?
        .chunks_exact(4)
        .enumerate()
        .skip(1)
        .map(|(level, row)| Ok((level as u8, dol::u32(row, 0)?)))
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
    let learned = u64::from_be_bytes(row[0x70..0x78].try_into()?);
    ensure!(
        allowed.len() <= 64,
        "character learning mask exceeds 64 bits"
    );
    let learned_techniques = allowed
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(slot, id)| {
            (learned & (1 << slot) != 0).then(|| Ok(techniques.target(u32::from(id))?.to_owned()))
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
            shortcuts.insert(direction, techniques.target(u32::from(id))?.to_owned());
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
        let bits = dol::u32(row, at)?;
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
        experience: dol::u32(row, 0x18)?,
        affinity: dol::u32(row, 0x58)? as i32,
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
