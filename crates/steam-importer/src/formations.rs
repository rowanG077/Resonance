//! Steam encounter records directly into the current named formation catalogue.
use crate::{monsters, package::Package, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_formation::{Actor, Formation, Formations, Resource, Settings, WeightedFormation},
    battle_rules,
    library::Mappings,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) fn read(usual: &[u8]) -> Result<Formations> {
    let package = Package::parse(usual)?;
    let rows = package.member(1)?.context("missing Steam formations")?;
    ensure!(
        !rows.is_empty() && rows.len().is_multiple_of(128),
        "invalid formation table"
    );
    let records = rows
        .chunks_exact(128)
        .map(|row| {
            ensure!(row.starts_with(b"gp3\0"), "invalid formation record");
            ensure!(
                row[88..].iter().all(|&b| b == 0),
                "unknown Steam formation suffix"
            );
            let actor_count = usize::from(row[4]);
            let resource_count = usize::from(row[5]);
            ensure!(
                actor_count <= 8 && resource_count <= 4,
                "invalid formation counts"
            );
            let signed = |at| -> Result<i16> { Ok(pe::half(row, at)? as i16) };
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
    let pool_rows = package
        .member(0)?
        .context("missing Steam encounter pools")?;
    ensure!(
        pool_rows.len().is_multiple_of(64),
        "invalid encounter pool table"
    );
    let pools = pool_rows
        .chunks_exact(64)
        .map(|row| {
            ensure!(
                row[32..].iter().all(|&b| b == 0),
                "unknown Steam encounter pool suffix"
            );
            let count = pe::word(row, 0)? as usize;
            ensure!((1..=6).contains(&count), "invalid encounter pool");
            (0..count)
                .map(|slot| {
                    Ok(WeightedFormation {
                        formation: pe::half(row, 4 + slot * 2)?,
                        weight: pe::half(row, 16 + slot * 2)?,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let table = Formations { records, pools };
    table.validate()?;
    Ok(table)
}

fn formation_name(row: &Formation) -> Result<String> {
    let mut groups = Vec::<(String, usize)>::new();
    for actor in &row.actors {
        let resource = &row.resources[usize::from(actor.resource)];
        let full = monsters::symbol(resource.enemy);
        let short = if full.starts_with("model-") {
            full.as_str()
        } else {
            full.rsplit_once('-')
                .map_or(full.as_str(), |(stem, _)| stem)
        };
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

/// Convert the complete battle formation and encounter-pool owner from the installation.
pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let table = read(&source.require("btlusual.pac")?)?;
    let image = pe::Image::parse(&source.executable)?;
    let enemies = Mappings::new(
        (0..monsters::count(&image)?)
            .map(|id| (id as u32, monsters::symbol(id as u16)))
            .collect(),
    )?;
    let document = battle_rules::Formations::from_table(&table, &enemies, &names(&table)?)?;
    crate::output::json(
        &library.join(resonance_content::battle_enemy::document::MAPPINGS),
        &enemies,
    )?;
    let output = library.join("battle/formations.json");
    crate::output::json(&output, &document)?;
    Ok(())
}
