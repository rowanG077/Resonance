//! Native world encounter tables directly into named regions and dock rules.
use crate::{dol, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::{
    Terrain, TerrainSurface,
    encounters::{
        Continent, Definition, Dock, Endpoint, Groups, Region, RegionGroups, SurfaceTerrain,
        TerrainGroups,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const DATA: usize = 6;
const RODATA: usize = 5;
const TERRAIN_COUNT: usize = 10;
const AREA_COUNT: usize = 12;
const AREA_SLOTS: usize = 21;
const AREA_BYTES: usize = TERRAIN_COUNT * 4;
const ROWS: usize = 18;
const COLUMNS: usize = 24;
const SURFACES: usize = 16;
const AREA_ROWS: usize = 0x2fa20;
const WORLD_LISTS: usize = 0x300bc;
const ARENAS: usize = 0x3018c;
const SURFACE_TERRAIN: usize = 0x3c8;
type NativeArea = [[u16; 2]; TERRAIN_COUNT];

const REGION_NAMES: [[&str; AREA_COUNT]; 2] = [
    [
        "iselia",
        "triet",
        "izoold",
        "palmacosta",
        "palmacosta-ranch",
        "thoda-island",
        "luin",
        "balacruf-mausoleum",
        "hima",
        "katz-village",
        "tower-of-salvation",
        "unassigned-region-11",
    ],
    [
        "fooji-mountains",
        "temple-of-earth",
        "sybak",
        "temple-of-lightning",
        "ozette",
        "tower-of-salvation",
        "flanoir",
        "altamira",
        "heimdall",
        "house-of-guidance-3",
        "toize-valley-mine",
        "house-of-guidance-6",
    ],
];

fn surface(id: usize) -> TerrainSurface {
    use TerrainSurface::*;
    match id {
        0 | 8 | 9 | 12 | 14 => Barrier,
        1 => Grassland,
        2 => Road,
        3 => Wasteland,
        4 => Snowfield,
        5 => Desert,
        6 => Forest,
        7 => DeepForest,
        10 => Beach,
        11 => Water,
        13 => Bridge,
        15 => Pier,
        _ => unreachable!(),
    }
}

fn native_areas(rel: &Rel) -> Result<Vec<Vec<Option<NativeArea>>>> {
    let end = AREA_ROWS + 3 * AREA_COUNT * AREA_BYTES;
    let expected: BTreeSet<_> = (AREA_ROWS..end).step_by(AREA_BYTES).collect();
    ensure!(
        rel.pointers
            .range((DATA, AREA_ROWS)..(DATA, end))
            .next()
            .is_none(),
        "unexpected relocation inside encounter group IDs"
    );
    let mut used = BTreeSet::new();
    let worlds = (0..3)
        .map(|world| {
            let (section, list) = rel.pointer(DATA, WORLD_LISTS + world * 4)?;
            ensure!(section == DATA, "encounter area list outside data section");
            rel.slice(DATA, list, AREA_SLOTS * 4)?;
            (0..AREA_SLOTS)
                .map(|index| {
                    let slot = list + index * 4;
                    let Some(&(section, at)) = rel.pointers.get(&(DATA, slot)) else {
                        ensure!(
                            dol::u32(rel.slice(DATA, slot, 4)?, 0)? == 0,
                            "unrelocated encounter area pointer"
                        );
                        return Ok(None);
                    };
                    ensure!(
                        section == DATA && expected.contains(&at),
                        "invalid encounter area pointer"
                    );
                    used.insert(at);
                    let mut groups = [[0; 2]; TERRAIN_COUNT];
                    for (pair, row) in groups
                        .iter_mut()
                        .zip(rel.slice(DATA, at, AREA_BYTES)?.chunks_exact(4))
                    {
                        *pair = [dol::u16(row, 0)?, dol::u16(row, 2)?];
                    }
                    Ok(Some(groups))
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        used == expected,
        "unreferenced authored encounter area rows"
    );
    Ok(worlds)
}

fn native_surfaces(rel: &Rel) -> Result<Vec<[Option<Terrain>; 12]>> {
    rel.slice(RODATA, SURFACE_TERRAIN, 2 * SURFACES)?
        .chunks_exact(SURFACES)
        .map(|row| {
            let mut values = [None; resonance_content::overworld::SURFACES];
            for (native, &id) in row.iter().enumerate() {
                let kind = surface(native);
                if kind != TerrainSurface::Barrier {
                    values[kind.index()] = if id == u8::MAX {
                        None
                    } else {
                        Some(
                            *Terrain::ALL
                                .get(id as usize)
                                .context("invalid surface terrain")?,
                        )
                    };
                }
            }
            Ok(values)
        })
        .collect()
}

fn convert(bytes: Vec<u8>) -> Result<Definition> {
    let rel = Rel::read(bytes)?;
    let areas = native_areas(&rel)?;
    let surfaces = native_surfaces(&rel)?;
    let grids = rel
        .slice(RODATA, SURFACE_TERRAIN + 2 * SURFACES, 2 * ROWS * COLUMNS)?
        .chunks_exact(ROWS * COLUMNS)
        .enumerate()
        .map(|(world, grid)| {
            ensure!(
                grid.iter()
                    .all(|&id| areas[world].get(id as usize).is_some_and(Option::is_some)),
                "world grid selects an absent encounter area"
            );
            if world == 1 {
                ensure!(
                    grid.iter()
                        .all(|&id| areas[2].get(id as usize).is_some_and(Option::is_some)),
                    "world grid selects an absent late encounter area"
                );
            }
            grid.chunks_exact(COLUMNS)
                .map(|row| Ok(row.try_into()?))
                .collect::<Result<Vec<[u8; COLUMNS]>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let mut counts = BTreeMap::<[u16; 2], usize>::new();
    for pair in areas
        .iter()
        .flatten()
        .flatten()
        .flat_map(|area| area.iter())
    {
        *counts.entry(*pair).or_default() += 1;
    }
    let default = counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(pair, _)| pair)
        .unwrap_or([0; 2]);
    let compact = |area: NativeArea| -> TerrainGroups {
        Terrain::ALL
            .into_iter()
            .zip(area)
            .filter_map(|(terrain, pair)| (pair != default).then(|| (terrain, Groups::new(pair))))
            .collect()
    };
    let continent = |world: usize| -> Result<Continent> {
        let mut regions = BTreeMap::new();
        let mut owners = BTreeMap::<NativeArea, String>::new();
        let mut later_owners = BTreeMap::<NativeArea, String>::new();
        for (id, area) in areas[world].iter().enumerate() {
            let Some(&area) = area.as_ref() else {
                continue;
            };
            let name = REGION_NAMES[world]
                .get(id)
                .context("encounter region has no name")?
                .to_string();
            let groups = if let Some(owner) = owners.get(&area) {
                RegionGroups::Shared(owner.clone())
            } else {
                owners.insert(area, name.clone());
                RegionGroups::Inline(compact(area))
            };
            let later = if world == 1 {
                let late = *areas[2][id]
                    .as_ref()
                    .context("missing later encounter region")?;
                if late == area {
                    if matches!(groups, RegionGroups::Inline(_)) {
                        later_owners.entry(late).or_insert_with(|| name.clone());
                    }
                    None
                } else if let Some(owner) = later_owners.get(&late) {
                    Some(RegionGroups::Shared(owner.clone()))
                } else {
                    later_owners.insert(late, name.clone());
                    Some(RegionGroups::Inline(compact(late)))
                }
            } else {
                None
            };
            ensure!(
                regions
                    .insert(
                        name,
                        Region {
                            id: id as u8,
                            groups,
                            later
                        }
                    )
                    .is_none(),
                "duplicate encounter region name"
            );
        }
        let arenas = Terrain::ALL
            .into_iter()
            .zip(
                rel.slice(DATA, ARENAS + world * TERRAIN_COUNT, TERRAIN_COUNT)?
                    .iter()
                    .copied(),
            )
            .collect();
        let surface_terrain: SurfaceTerrain = surfaces[world]
            .iter()
            .enumerate()
            .filter_map(|(id, &terrain)| {
                (terrain != surfaces[0][id]).then_some((TerrainSurface::ALL[id], terrain))
            })
            .collect();
        Ok(Continent {
            arenas,
            grid: grids[world].clone(),
            regions,
            surface_terrain,
        })
    };
    let endpoint = |row: &[u8]| -> Result<Endpoint> {
        Ok(Endpoint {
            position: [dol::u32(row, 0)? as i32, dol::u32(row, 4)? as i32],
            height: f32::from_bits(dol::u32(row, 8)?),
        })
    };
    let docks = rel
        .slice(RODATA, 0x60, 8 * 32)?
        .chunks_exact(32)
        .map(|pair| {
            Ok(Dock {
                land: endpoint(&pair[..16])?,
                sea: endpoint(&pair[16..])?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(Definition {
        default_group: Groups::new(default),
        surface_terrain: surfaces[0]
            .iter()
            .enumerate()
            .filter_map(|(id, &terrain)| terrain.map(|_| (TerrainSurface::ALL[id], terrain)))
            .collect(),
        docks,
        later_story: 10_405_000,
        sylvarant: continent(0)?,
        tethe_alla: continent(1)?,
    })
}

pub fn cook_world_encounters(root: &Path, library: &Path) -> Result<()> {
    let output = library.join("world/encounters.json");
    ensure!(!output.exists(), "world encounters output already exists");
    let document = convert(fs::read(root.join("disc1/files/US_r_Top2field.rel"))?)?;
    crate::output::json(&output, &document)?;
    Ok(())
}
