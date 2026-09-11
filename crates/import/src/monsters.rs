//! Decode enemy packages into named data, meshes, textures and animation clips.
use crate::{
    compression, dol,
    read::{u16 as half, u32 as word},
    write_atomic,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    menu_data::Element,
    monster::{MONSTER_COUNT, MONSTER_VERSION, Monster, MonsterStats},
};
use std::{fs, path::Path};
mod preview;

pub(crate) fn book(
    executable: &[u8],
    output: &Path,
) -> Result<resonance_content::monster::MonsterBook> {
    let mut labels = std::collections::BTreeMap::new();
    for (key, address) in [
        ("number", 0x8035d370),
        ("hp", 0x8035d374),
        ("tp", 0x8035d378),
        ("unknown_item", 0x801aa9ec),
    ] {
        labels.insert(key.into(), dol::text(executable, address)?);
    }
    for (key, offset) in [
        ("title", 0x26c),
        ("normal", 0x2a4),
        ("hard", 0x2a8),
        ("mania", 0x2ac),
        ("attack", 0x2b0),
        ("experience", 0x2b4),
        ("gald", 0x2b8),
        ("defense", 0x2bc),
        ("drops", 0x2c0),
        ("steal", 0x2c4),
        ("location", 0x2c8),
        ("attack_element", 0x2cc),
        ("weak", 0x2d0),
        ("strong", 0x2d4),
        ("battle_rank", 0x2e0),
    ] {
        let address = word(dol::slice(executable, 0x8019d490 + offset, 4)?, 0)?;
        labels.insert(key.into(), dol::text(executable, address)?);
    }
    let unknown = word(dol::slice(executable, 0x8035d30c, 4)?, 0)?;
    labels.insert("unknown_stat".into(), dol::text(executable, unknown)?);
    Ok(resonance_content::monster::MonsterBook {
        labels,
        records: (0..MONSTER_COUNT)
            .map(|id| {
                serde_json::from_slice(&fs::read(output.join(format!("monsters/{id:03}.json")))?)
                    .context("monster assets need recooking; run cook-monsters")
            })
            .collect::<Result<_>>()?,
    })
}

pub fn cook(extracted: &Path, output: &Path, ktx: &Path, selected: &[u8]) -> Result<()> {
    ensure!(
        selected.iter().all(|&id| usize::from(id) < MONSTER_COUNT),
        "unknown monster requested"
    );
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let directory = fs::read(extracted.join("files/BTL/BTLusual.dat"))?;
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat"))?;
    let table = word(&directory, 0x2c)? as usize;
    for id in 0..MONSTER_COUNT as u8 {
        if !selected.is_empty() && !selected.contains(&id) {
            continue;
        }
        let start = word(&directory, table + usize::from(id) * 4)? as usize;
        let end = word(&directory, table + (usize::from(id) + 1) * 4)? as usize;
        let bytes = compression::decode(
            archive
                .get(start..end)
                .context("enemy package exceeds archive")?,
        )
        .with_context(|| format!("decode monster {id}"))?;
        cook_monster(&executable, &bytes, id, output, ktx)
            .with_context(|| format!("cook monster {id}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn cook_monster(executable: &[u8], bytes: &[u8], id: u8, output: &Path, ktx: &Path) -> Result<()> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
    let metadata = bytes
        .get(usize::from(half(bytes, 4)?)..)
        .context("invalid enemy metadata")?;
    ensure!(metadata.len() >= 0x1e8, "truncated enemy metadata");
    let stats = bytes
        .get(usize::from(half(bytes, 6)?)..)
        .context("invalid enemy statistics")?;
    let row = dol::slice(executable, 0x802113f4 + u32::from(id) * 12, 12)?;
    let text = |address| dol::text(executable, address);
    let pointer = |address| word(dol::slice(executable, address, 4)?, 0);
    let element = |value: u8| -> Result<Option<Element>> {
        ensure!(value <= 8, "invalid monster element");
        Ok(value.checked_sub(1).map(|i| Element::ALL[usize::from(i)]))
    };
    let elements = |test: fn(i8) -> bool| {
        Element::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(i, element)| test(metadata[2 + i] as i8).then_some(element))
            .collect()
    };
    let item = |offset| -> Result<Option<u16>> {
        let id = half(metadata, offset)?;
        Ok((id != 0).then_some(id))
    };
    let name = text(word(row, 0)?)?;
    let mut statistics = vec![MonsterStats {
        hp: word(stats, 0x20)?,
        tp: half(stats, 0x28)?,
        attack: half(stats, 0x2c)?,
        defense: half(stats, 0x30)?,
        experience: word(metadata, 0x40)?,
        gald: word(metadata, 0x44)?,
    }];
    let variants = usize::from(metadata[0x1e7]);
    ensure!(variants < 16, "invalid enemy variant count");
    if variants > 0 {
        let start = word(bytes, 0x1e0)? as usize;
        ensure!(start >= 0x1e8, "missing enemy variant table");
        for row in bytes
            .get(start..start + variants * 36)
            .context("truncated enemy variant table")?
            .chunks_exact(36)
        {
            statistics.push(MonsterStats {
                hp: word(row, 0)?,
                tp: half(row, 8)?,
                attack: half(row, 0x14)?,
                defense: half(row, 0x18)?,
                experience: word(row, 0xc)?,
                gald: word(row, 0x10)?,
            });
        }
    }
    let monster = Monster {
        version: MONSTER_VERSION,
        id,
        name,
        location: text(pointer(0x80211328 + u32::from(row[9]) * 4)?)?,
        category: text(pointer(
            0x8019d650 + 0xb0 + u32::from(half(metadata, 0x2a)?) * 4,
        )?)?,
        statistics,
        drops: [item(0x48)?, item(0x4a)?],
        steal: item(0x4c)?,
        attack_element: element(metadata[0])?,
        weaknesses: elements(|v| v == 1),
        resistances: elements(|v| v > 1),
        preview: preview::cook(bytes, metadata, id, output, ktx)?,
    };
    monster.validate(528)?;
    write_atomic(
        &output.join(format!("monsters/{id:03}.json")),
        &serde_json::to_vec_pretty(&monster)?,
    )?;
    println!("Cooked monster {id}: {}", monster.name);
    Ok(())
}
