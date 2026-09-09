//! Extract fresh-game definitions from the executable into ordinary JSON.
use crate::{digest, dol, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::session::{CharacterDefinition, ItemDefinition, SessionData, StatGrowth};
use std::{collections::BTreeMap, fs, path::Path};

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
    let items = dol::slice(&executable, 0x801FAD98, 528 * 60)?
        .chunks_exact(60)
        .map(|row| {
            let mask = u16::from(row[0x15]);
            ItemDefinition {
                equipment_kind: match row[0x1a] {
                    13..=22 => Some(0),
                    23..=26 => Some(1),
                    27..=30 => Some(2),
                    31..=34 => Some(3),
                    35..=42 => Some(4),
                    _ => None,
                },
                // Kratos and Zelos share the source's owner bit 5.
                allowed_characters: mask | ((mask & 0x20) << 3),
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
