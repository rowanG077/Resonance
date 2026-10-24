//! Decode effect timelines, particle inputs, and combat colors.
pub(crate) mod art;
mod declaration;
mod palettes;
pub(crate) use palettes::SharedPalettes;
mod source;
mod timeline;
use anyhow::{Context, Result, ensure};
pub use source::{prepare, read};
pub(crate) use source::{read_enemy_with_palettes, read_with_palettes};
use timeline::Record;

pub fn read_tints(file: &std::path::Path) -> Result<resonance_content::battle_effect::Tints> {
    use resonance_content::battle_effect::{ActorColors, AdmissionColors, Tints};
    let module = crate::rel::Rel::read(file)?;
    let actors: [[u8; 4]; 12] = crate::read::Field::read(module.at((4, 0x1564))?, 0)?;
    let admission: [[u8; 4]; 4] = crate::read::Field::read(module.at((4, 0x11b0))?, 0)?;
    let tints = Tints {
        palettes: module
            .at((4, 0x2174))?
            .get(..10)
            .context("truncated effect palette table")?
            .try_into()?,
        colors: crate::read::Field::read(module.at((4, 0x2180))?, 0)?,
        actors: ActorColors {
            recovery: actors[0],
            buff: actors[2],
            scan: actors[7],
            debuff: actors[8],
        },
        contact_effects: crate::read::Field::read(module.at((5, 0x13f8))?, 0)?,
        contact_colors: crate::read::Field::read(module.at((5, 0x13d0))?, 0)?,
        admission_colors: AdmissionColors {
            basic: admission[0],
            advanced: admission[1],
            arcane: admission[2],
        },
    };
    Ok(tints)
}

pub fn modifier(bytes: &[u8], at: usize) -> Result<Vec<u16>> {
    let mut bytes = bytes.get(at..).context("effect modifier outside bank")?;
    let mut words = Vec::new();
    loop {
        let opcode = u16::from_be_bytes(
            bytes
                .get(..2)
                .context("truncated effect modifier")?
                .try_into()
                .unwrap(),
        );
        let count = match opcode {
            0xffff => 1,
            0 | 2..=6 | 10..=12 | 16 | 17 | 19 | 21 | 23..=25 => 4,
            1 | 20 | 22 => 8,
            7 | 8 | 13..=15 | 18 | 26..=30 => 6,
            9 => 12,
            _ => anyhow::bail!("unknown effect modifier opcode {opcode}"),
        };
        let row = bytes
            .get(..count * 2)
            .context("truncated effect modifier operands")?;
        words.extend(
            row.chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]])),
        );
        if opcode == 0xffff {
            return Ok(words);
        }
        bytes = &bytes[count * 2..];
    }
}

fn timelines(bytes: &[u8], roots: &[u8]) -> Result<Vec<Result<Vec<Record>>>> {
    let starts = roots
        .chunks_exact(2)
        .map(|root| {
            let start = usize::try_from(i16::from_be_bytes([root[0], root[1]]))
                .context("effect program outside command section")?;
            ensure!(
                start < bytes.len(),
                "effect program outside command section"
            );
            Ok(start)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(starts
        .into_iter()
        .map(|start| {
            let mut records = Vec::new();
            let mut payload = false;
            for bytes in bytes[start..].chunks_exact(6) {
                let record = Record::from_bytes(bytes.try_into().unwrap());
                records.push(record);
                if payload {
                    ensure!(
                        record.command < 254,
                        "effect repeat requires an emission, sound or modification"
                    );
                    payload = false;
                } else if record.command == 254 {
                    return Ok(records);
                } else {
                    payload = record.command == 255;
                }
            }
            anyhow::bail!("unterminated effect program")
        })
        .collect())
}
