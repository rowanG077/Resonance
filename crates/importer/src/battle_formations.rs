//! GameCube battle encounter tables directly into one editable formation catalogue.
use crate::dol;
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_formation::{Actor, Formation, Formations, Resource, Settings, WeightedFormation},
    battle_rules,
    library::Mappings,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::MonsterId;

fn section(bytes: &[u8], index: usize) -> Result<&[u8]> {
    let start = dol::u32(bytes, 4 + index * 4)? as usize;
    let end = dol::u32(bytes, 4 + (index + 1) * 4)? as usize;
    ensure!(start < end, "missing battle table {index}");
    bytes.get(start..end).context("battle table exceeds file")
}

fn read(usual: &[u8]) -> Result<Formations> {
    let rows = section(usual, 1)?;
    ensure!(
        !rows.is_empty() && rows.len().is_multiple_of(96),
        "invalid formation table"
    );
    let records = rows
        .chunks_exact(96)
        .map(|row| {
            ensure!(row.starts_with(b"gp3\0"), "invalid formation record");
            let actor_count = usize::from(row[4]);
            let resource_count = usize::from(row[5]);
            ensure!(
                actor_count <= 8 && resource_count <= 4,
                "invalid formation counts"
            );
            let signed = |at| -> Result<i16> { Ok(dol::u16(row, at)? as i16) };
            let resources = (0..resource_count)
                .map(|slot| {
                    Ok(Resource {
                        enemy: u16::try_from(signed(8 + slot * 2)?)?,
                        hidden_name: row[7] & (1 << slot) != 0,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let actors = (0..actor_count)
                .map(|slot| {
                    let position = if row[6] & 2 == 0 {
                        Some([signed(56 + slot * 4)?, signed(58 + slot * 4)?])
                    } else {
                        None
                    };
                    Ok(Actor {
                        resource: row[16 + slot],
                        variant: row[32 + slot],
                        texture_variant: row[24 + slot],
                        weapon_variant: row[40 + slot],
                        position,
                        unsupported_reason: (row[48 + slot] != 0)
                            .then(|| "encounter appearance override is not prepared".into()),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Formation {
                settings: Settings {
                    play_music: row[6] & 0x10 == 0,
                    celebrate: row[6] & 0x20 == 0,
                    entry_voice: row[6] & 0x40 == 0,
                    escape_restricted: row[6] & 1 != 0,
                },
                resources,
                actors,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let pool_rows = section(usual, 0)?;
    ensure!(
        pool_rows.len().is_multiple_of(32),
        "invalid encounter pool table"
    );
    let pools = pool_rows
        .chunks_exact(32)
        .map(|row| {
            let count = usize::from(dol::u16(row, 2)?);
            ensure!(
                dol::u16(row, 0)? == 0 && (1..=6).contains(&count),
                "invalid encounter pool"
            );
            (0..count)
                .map(|slot| {
                    Ok(WeightedFormation {
                        formation: dol::u16(row, 4 + slot * 2)?,
                        weight: dol::u16(row, 16 + slot * 2)?,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let table = Formations { records, pools };
    table.validate()?;
    Ok(table)
}

fn enemy_name(id: u16) -> Result<&'static str> {
    Ok(MonsterId::try_from(u8::try_from(id)?)?
        .resource_name()
        .context("formation enemy has no name")?)
}

fn formation_name(row: &Formation) -> Result<String> {
    let mut groups = Vec::<(String, usize)>::new();
    for actor in &row.actors {
        let resource = &row.resources[usize::from(actor.resource)];
        let full = enemy_name(resource.enemy)?;
        let short = full.rsplit_once('-').map_or(full, |(stem, _)| stem);
        if let Some((_, count)) = groups.iter_mut().find(|(name, _)| name == short) {
            *count += 1;
        } else {
            groups.push((short.into(), 1));
        }
    }
    if groups.is_empty() {
        return Ok("empty-formation".into());
    }
    Ok(groups
        .into_iter()
        .map(|(name, count)| {
            if count == 1 {
                name
            } else {
                format!("{name}-x{count}")
            }
        })
        .collect::<Vec<_>>()
        .join("-and-"))
}

fn names(table: &Formations) -> Result<Mappings> {
    let mut names = BTreeMap::<u32, String>::new();
    let mut used = BTreeSet::new();
    for (id, row) in table.records.iter().enumerate() {
        if let Some((previous, _)) = table.records[..id]
            .iter()
            .enumerate()
            .find(|(_, old)| *old == row)
        {
            names.insert(
                id as u32,
                names
                    .get(&(previous as u32))
                    .context("missing repeated formation name")?
                    .clone(),
            );
            continue;
        }
        let base = formation_name(row)?;
        let mut name = base.clone();
        if used.contains(&name) {
            name = format!("{base}-{id:04}");
        }
        ensure!(used.insert(name.clone()), "duplicate formation name {name}");
        names.insert(id as u32, name);
    }
    Mappings::new(names)
}

/// Convert the complete battle formation and encounter-pool owner from disc 1.
pub fn cook_battle_formations(extracted: &Path, library: &Path) -> Result<()> {
    let source = fs::read(extracted.join("disc1/files/BTL/BTLusual.dat"))?;
    let table = read(&source)?;
    let enemies = Mappings::new(
        MonsterId::ALL
            .into_iter()
            .filter_map(|id| {
                id.resource_name()
                    .map(|name| (u32::from(id.raw()), name.into()))
            })
            .collect(),
    )?;
    let document = battle_rules::Formations::from_table(&table, &enemies, &names(&table)?)?;
    let output = library.join("battle/formations.json");
    crate::output::json(&output, &document)?;
    Ok(())
}
