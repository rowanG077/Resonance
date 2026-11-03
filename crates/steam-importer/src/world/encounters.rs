use super::*;
use world::{EncounterArea, EncounterTables, EncounterWorld, Terrain, TravelEndpoint, WorldKind};
const REGION_NAMES: [[&str; 12]; 2] = [
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

pub(super) fn read(image: &pe::Image<'_>) -> Result<EncounterTables> {
    // 0x460380 selects one of three area directories, then indexes the ten
    // terrain/symbol pairs. The late Tethe'alla desert pair is 698/699 in Steam.
    let mut worlds = Vec::new();
    for (index, kind) in [
        WorldKind::Sylvarant,
        WorldKind::TetheAlla,
        WorldKind::TetheAllaLate,
    ]
    .into_iter()
    .enumerate()
    {
        let directory = pe::word(image.slice(0x8e5a3c + index as u32 * 4, 4)?, 0)?;
        let mut areas = Vec::new();
        for pointer in image.slice(directory, 21 * 4)?.chunks_exact(4) {
            let address = pe::word(pointer, 0)?;
            areas.push(if address == 0 {
                None
            } else {
                let mut groups = [[0; 2]; world::TERRAINS];
                for (group, pair) in groups
                    .iter_mut()
                    .zip(image.slice(address, world::TERRAINS * 4)?.chunks_exact(4))
                {
                    *group = [pe::half(pair, 0)?, pe::half(pair, 2)?];
                }
                Some(EncounterArea {
                    encounter_groups: groups,
                })
            });
        }
        worlds.push(EncounterWorld {
            kind,
            areas,
            area_names: (0..21)
                .map(|i| {
                    REGION_NAMES[index.min(1)]
                        .get(i)
                        .map(|name| (*name).to_owned())
                })
                .collect(),
            // 0x4b234c chooses arena by continent * 10 + terrain. Later story
            // changes encounter groups, while the terrain arenas stay shared.
            arena_by_terrain: image
                .slice(0x9190fc + index.min(1) as u32 * 10, world::TERRAINS)?
                .try_into()?,
        });
    }
    let surfaces = stack_array(image, -0x24, &[(0x4b18f1, 8, 7)])?;
    let surface_terrain = surfaces
        .chunks_exact(16)
        .map(|row| {
            let mut result = [None; world::SURFACES];
            for (native, &id) in row.iter().enumerate() {
                let surface = surface(native as u32)?;
                if surface == world::TerrainSurface::Barrier {
                    continue;
                }
                let target = &mut result[surface.index()];
                if id != u8::MAX {
                    *target = Some(
                        *Terrain::ALL
                            .get(usize::from(id))
                            .context("invalid Steam encounter terrain")?,
                    );
                }
            }
            Ok(result)
        })
        .collect::<Result<_>>()?;
    // 0x4b1950 initializes two row-major 24x18 grids. Interleaved FPU
    // instructions calculate the query position but do not change the arrays.
    let grids = stack_array(
        image,
        -0x364,
        &[
            (0x4b1968, 158, 10),
            (0x4b1f97, 1, 10),
            (0x4b1fa7, 1, 10),
            (0x4b1fb3, 1, 10),
            (0x4b1fbf, 2, 10),
            (0x4b1fd9, 22, 10),
            (0x4b20b5, 31, 7),
        ],
    )?;
    ensure!(
        grids.len() == 2 * world::AREA_ROWS * world::AREA_COLUMNS,
        "invalid Steam region grid extent"
    );
    let area_grids = grids
        .chunks_exact(world::AREA_ROWS * world::AREA_COLUMNS)
        .map(|grid| {
            grid.chunks_exact(world::AREA_COLUMNS)
                .map(|row| Ok(row.try_into()?))
                .collect::<Result<_>>()
        })
        .collect::<Result<_>>()?;
    let endpoint = |row: &[u8]| -> Result<TravelEndpoint> {
        Ok(TravelEndpoint {
            map_x: pe::word(row, 0)? as i32,
            map_z: pe::word(row, 4)? as i32,
            height: f32::from_bits(pe::word(row, 8)?),
        })
    };
    let travel_points = image
        .slice(0x7a35b8, 8 * 32)?
        .chunks_exact(32)
        .map(|row| Ok([endpoint(&row[..16])?, endpoint(&row[16..])?]))
        .collect::<Result<_>>()?;
    let tables = EncounterTables {
        worlds,
        surface_terrain,
        area_grids,
        travel_points,
        later_story: pe::word(image.slice(0x460397, 4)?, 0)? as i32,
    };
    tables.validate()?;
    Ok(tables)
}
