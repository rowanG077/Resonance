//! Extract fresh-game definitions from the executable into ordinary JSON.
use crate::{digest, dol, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::session::{CharacterDefinition, ItemDefinition, SessionData, StatGrowth};
use std::{collections::BTreeMap, fs, path::Path};

/// Localized display text stays separate from save-compatible item statistics.
pub(crate) fn cook_text(extracted: &Path, output: &Path) -> Result<String> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let characters = (1..=10)
        .map(|id| {
            let address = if id == 10 {
                0x8035bb80 // The companion's name is initialized separately from party records.
            } else {
                0x801f9fc8 + (id - 1) as u32 * 0x118
            };
            let name = dol::text(&executable, address)?;
            ensure!(
                !name.is_empty() && !name.chars().any(char::is_control),
                "invalid character name {id}"
            );
            Ok((id, name))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut names = BTreeMap::new();
    for (id, row) in dol::slice(&executable, 0x801fad98, 528 * 60)?
        .chunks_exact(60)
        .enumerate()
    {
        let pointer = u32::from_be_bytes(row[..4].try_into()?);
        let bytes = dol::slice(&executable, pointer, 128)?;
        let end = bytes
            .iter()
            .position(|&b| b == 0)
            .context("unterminated item name")?;
        let (name, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes[..end]);
        ensure!(
            !invalid && !name.chars().any(char::is_control),
            "invalid item name {id}"
        );
        names.insert(id as u16, name.into_owned());
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

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<String> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let half = |bytes: &[u8], at: usize| -> Result<u16> {
        Ok(u16::from_be_bytes(
            bytes
                .get(at..at + 2)
                .context("truncated session halfword")?
                .try_into()?,
        ))
    };
    let word = |bytes: &[u8], at: usize| -> Result<u32> {
        Ok(u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .context("truncated session word")?
                .try_into()?,
        ))
    };
    // The equipment table has 528 entries of 60 bytes: categories,
    // permitted owners, and one-item stack limits.
    let zelos_only = [0x800eba48, 0x800eba54, 0x800eba60, 0x800eba6c]
        .into_iter()
        .map(|address| {
            let instruction = word(dol::slice(&executable, address, 4)?, 0)?;
            ensure!(
                instruction >> 16 == 0x2c05,
                "unsupported equipment owner check"
            );
            Ok(usize::from(instruction as u16))
        })
        .collect::<Result<Vec<_>>>()?;
    let items = dol::slice(&executable, 0x801FAD98, 528 * 60)?
        .chunks_exact(60)
        .enumerate()
        .map(|(id, row)| {
            let mask = u16::from(row[0x15]);
            // Expand the shared swordsman bit, then apply Zelos's exclusive gear.
            let mut mask = mask | ((mask & 0x20) << 3);
            if zelos_only.contains(&id) {
                mask &= !0x20;
            }
            ItemDefinition {
                equipment_kind: match row[0x1a] {
                    13..=22 => Some(0),
                    23..=26 => Some(1),
                    27..=30 => Some(2),
                    31..=34 => Some(3),
                    35..=42 => Some(4),
                    _ => None,
                },
                allowed_characters: mask,
                stack_limit: if row[0x1a] == 45 { 1 } else { 20 },
            }
        })
        .collect();
    let experience = dol::slice(&executable, 0x80202958, 252 * 4)?
        .chunks_exact(4)
        .map(|v| u32::from_be_bytes(v.try_into().unwrap()))
        .collect();
    let mut characters = Vec::new();
    for index in 0..9u32 {
        let row = dol::slice(&executable, 0x801F9FC8 + index * 0x118, 0x118)?;
        let list = dol::slice(&executable, 0x80202DC8 + index * 0x29, 0x29)?;
        ensure!(list[0] <= 40, "invalid character technique list");
        let learned = u64::from_be_bytes(row[0x70..0x78].try_into()?);
        let mut techniques = Vec::new();
        let mut level_techniques = BTreeMap::<u8, Vec<u16>>::new();
        for (slot, &id) in list[1..=usize::from(list[0])].iter().enumerate() {
            // Owner masks number bits from the least significant bit.
            if learned & (1u64 << slot) != 0 {
                techniques.push(u16::from(id));
            }
            let tech = dol::slice(&executable, 0x80202F90 + u32::from(id) * 0x58, 0x58)?;
            let required = half(tech, 0x3e)?;
            if required != 0
                && required <= 250
                && tech[0x17] == 0
                && half(tech, 0x18)? == 0
                && half(tech, 0x26)? == 0
            {
                level_techniques
                    .entry(required as u8)
                    .or_default()
                    .push(u16::from(id));
            }
        }
        let gains = dol::slice(&executable, 0x80202D48 + index * 14, 14)?;
        let title_start = half(dol::slice(&executable, 0x80210920 + index * 2, 2)?, 0)?;
        // Members start with title 1 before setup scripts run.
        let title = dol::slice(&executable, 0x80210934 + u32::from(title_start) * 16, 16)?;
        let stats = [0x26, 0x28, 0x2a, 0x2c, 0x34, 0x32, 0x30].map(|at| half(row, at).unwrap());
        characters.push(CharacterDefinition {
            cooking: row[0xf4..0x10c].try_into()?,
            ex_skills: row[0xee..0xf2].try_into()?,
            ex_gems: row[0xea..0xee].try_into()?,
            compound_ex_skills: (0..24u8)
                .filter(|i| word(row, 0x110).unwrap() & (1 << i) != 0)
                .collect(),
            recent_compound_ex_skills: (0..24u8)
                .filter(|i| word(row, 0x114).unwrap() & (1 << i) != 0)
                .collect(),
            technique_balance: row[0x10c] as i8,
            affinity: word(row, 0x58)? as i32,
            level: row[0x10],
            experience: word(row, 0x18)?,
            base_stats: stats,
            luck: (half(row, 0x2e)? / 10).min(255) as u8,
            overlimit: row[0x56],
            equipment: [0x4a, 0x4c, 0x4e, 0x52, 0x54, 0x50].map(|at| half(row, at).unwrap()),
            techniques,
            allowed_techniques: list[1..=usize::from(list[0])]
                .iter()
                .map(|id| u16::from(*id))
                .collect(),
            shortcuts: [0xd8, 0xda, 0xdc, 0xde].map(|at| half(row, at).unwrap()),
            growth: std::array::from_fn(|i| StatGrowth {
                base: gains[i * 2],
                random: gains[i * 2 + 1],
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
        experience,
    };
    data.validate()?;
    let path = "game/session-data.json";
    write_atomic(&output.join(path), &serde_json::to_vec_pretty(&data)?)?;
    Ok(path.into())
}
