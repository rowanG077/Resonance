//! Overworld travel points and maps selecting encounter groups and battle arenas.
//! These IDs feed the weighted encounter table; they are not formation IDs.
use crate::{embedded, read::u32 as word, rel::Rel};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};
#[path = "overworld_tiles.rs"]
pub(super) mod tiles;

const DATA: usize = 6;
const RODATA: usize = 5;
const TERRAINS: usize = 10;
const AREA_COUNT: usize = 12;
const AREA_SLOTS: usize = 21;
const AREA_BYTES: usize = TERRAINS * 2 * 2;
const WORLD_ROWS: usize = 18;
const WORLD_COLUMNS: usize = 24;
const SURFACES: usize = 16;
const TRAVEL_POINTS_TABLE: usize = 0x60;
const TRAVEL_POINTS: usize = 8;
const TRAVEL_PAIR_BYTES: usize = 32;

/// Indexed [heading sector][position quadrant][priority][column, row].
type TileLoadingOrder = [[[[i8; 2]; 9]; 4]; 8];

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Terrain {
    Grassland,
    Road,
    Wasteland,
    Snowfield,
    Desert,
    Forest,
    DeepForest,
    Beach,
    Bridge,
    Mountains,
}

const TERRAIN: [Terrain; TERRAINS] = [
    Terrain::Grassland,
    Terrain::Road,
    Terrain::Wasteland,
    Terrain::Snowfield,
    Terrain::Desert,
    Terrain::Forest,
    Terrain::DeepForest,
    Terrain::Beach,
    Terrain::Bridge,
    Terrain::Mountains,
];

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum WorldKind {
    Sylvarant,
    TetheAlla,
    /// The encounter lookup switches to this table at story phase 0x9ec488.
    TetheAllaLate,
}

#[derive(Serialize)]
struct Tables {
    terrains: Vec<TerrainLabel>,
    worlds: Vec<World>,
    /// Sylvarant and Tethe'alla each map 16 collision surfaces to a terrain.
    /// Null is the authored 0xff sentinel, which suppresses encounters.
    surface_terrain: Vec<[Option<Terrain>; SURFACES]>,
    /// Two complete world maps, indexed [world][row][column]. Values select areas.
    area_grids: Vec<Vec<[u8; WORLD_COLUMNS]>>,
    /// Paired embark/disembark endpoints in their authored order.
    travel_points: Vec<[TravelEndpoint; 2]>,
    tile_loading_order: TileLoadingOrder,
}

#[derive(Deserialize, Serialize)]
struct TravelEndpoint {
    map_x: i32,
    map_z: i32,
    height: f32,
    /// Retail and debug travel selectors/transfers consume XYZ only.
    /// Keep the authored bits; their original editor meaning is unknown.
    unused_word: u32,
}

#[derive(Serialize)]
struct TerrainLabel {
    terrain: Terrain,
    name: String,
}

#[derive(Serialize)]
struct World {
    kind: WorldKind,
    name: String,
    /// Names and encounter slots have different physical lengths.
    area_names: Vec<Option<String>>,
    areas: Vec<Option<Area>>,
    arena_by_terrain: [u8; TERRAINS],
}

#[derive(Serialize)]
struct Area {
    /// Indexed [terrain][enemy symbol kind]. Both authored alternatives remain.
    encounter_groups: [[u16; 2]; TERRAINS],
}

#[derive(Clone, Copy)]
struct Layout {
    /// All fields except surface_terrain are offsets in DATA.
    surface_terrain: usize,
    tile_loading_order: usize,
    area_rows: usize,
    worlds: usize,
    world_names: usize,
    area_names: usize,
    terrain_names: usize,
    arenas: usize,
}

fn layout(file: &Path) -> Option<Layout> {
    let (
        surface_terrain,
        tile_loading_order,
        area_rows,
        worlds,
        world_names,
        area_names,
        terrain_names,
        arenas,
    ) = match file.file_name()?.to_str()? {
        "US_r_Top2field.rel" | "US_m_Top2field.rel" | "US_Top2field.rel" => (
            0x3c8, 0x2a588, 0x2fa20, 0x300bc, 0x300c8, 0x300d4, 0x30164, 0x3018c,
        ),
        "r_Top2field.rel" | "m_Top2field.rel" | "Top2field.rel" => (
            0x2c8, 0x2a51c, 0x2f9c0, 0x3005c, 0x30068, 0x30074, 0x30104, 0x3012c,
        ),
        "Top2fieldD.rel" => (
            0x3a0, 0x2accb, 0x30588, 0x30c24, 0x30c54, 0x30d98, 0x30e58, 0x30f55,
        ),
        _ => return None,
    };
    Some(Layout {
        surface_terrain,
        tile_loading_order,
        area_rows,
        worlds,
        world_names,
        area_names,
        terrain_names,
        arenas,
    })
}

fn slice(rel: &Rel, section: usize, at: usize, size: usize) -> Result<&[u8]> {
    rel.at((section, at))?
        .get(..size)
        .context("truncated overworld encounter table")
}

fn optional_pointer(rel: &Rel, at: usize) -> Result<Option<(usize, usize)>> {
    if let Some(&pointer) = rel.pointers.get(&(DATA, at)) {
        Ok(Some(pointer))
    } else {
        ensure!(
            word(rel.at((DATA, at))?, 0)? == 0,
            "unrelocated encounter pointer"
        );
        Ok(None)
    }
}

fn name(rel: &Rel, at: usize) -> Result<Option<String>> {
    optional_pointer(rel, at)?
        .map(|pointer| {
            let text = rel.text(pointer)?;
            ensure!(!text.is_empty(), "empty overworld encounter label");
            Ok(text)
        })
        .transpose()
}

fn read(rel: &Rel, layout: Layout) -> Result<Tables> {
    let area_end = layout.area_rows + 3 * AREA_COUNT * AREA_BYTES;
    let expected_rows: BTreeSet<_> = (layout.area_rows..area_end).step_by(AREA_BYTES).collect();
    ensure!(
        rel.pointers
            .range((DATA, layout.area_rows)..(DATA, area_end))
            .next()
            .is_none(),
        "unexpected relocation inside encounter group IDs"
    );
    let mut decoded_rows = BTreeSet::new();
    let worlds = [
        WorldKind::Sylvarant,
        WorldKind::TetheAlla,
        WorldKind::TetheAllaLate,
    ]
    .into_iter()
    .enumerate()
    .map(|(world, kind)| -> Result<_> {
        let (section, list) = rel.pointer(DATA, layout.worlds + world * 4)?;
        ensure!(
            section == DATA,
            "encounter area list is outside data section"
        );
        slice(rel, DATA, list, AREA_SLOTS * 4)?;
        let areas = (0..AREA_SLOTS)
            .map(|index| {
                optional_pointer(rel, list + index * 4)?
                    .map(|(section, at)| {
                        ensure!(
                            section == DATA && expected_rows.contains(&at),
                            "invalid encounter area pointer"
                        );
                        decoded_rows.insert(at);
                        let bytes = slice(rel, DATA, at, AREA_BYTES)?;
                        let mut encounter_groups = [[0; 2]; TERRAINS];
                        for (pair, row) in encounter_groups.iter_mut().zip(bytes.chunks_exact(4)) {
                            *pair = [
                                u16::from_be_bytes([row[0], row[1]]),
                                u16::from_be_bytes([row[2], row[3]]),
                            ];
                        }
                        Ok(Area { encounter_groups })
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(World {
            kind,
            name: name(rel, layout.world_names + world * 4)?.context("missing world label")?,
            area_names: (0..AREA_COUNT)
                .map(|area| name(rel, layout.area_names + (world * AREA_COUNT + area) * 4))
                .collect::<Result<_>>()?,
            areas,
            arena_by_terrain: slice(rel, DATA, layout.arenas + world * TERRAINS, TERRAINS)?
                .try_into()?,
        })
    })
    .collect::<Result<Vec<_>>>()?;
    ensure!(
        decoded_rows == expected_rows,
        "unreferenced authored encounter area rows"
    );

    let surface_terrain = slice(rel, RODATA, layout.surface_terrain, 2 * SURFACES)?
        .chunks_exact(SURFACES)
        .map(|row| {
            let mut terrains = [None; SURFACES];
            for (terrain, &value) in terrains.iter_mut().zip(row) {
                *terrain = if value == u8::MAX {
                    None
                } else {
                    Some(
                        *TERRAIN
                            .get(usize::from(value))
                            .context("invalid surface terrain")?,
                    )
                };
            }
            Ok(terrains)
        })
        .collect::<Result<_>>()?;
    let area_grids = slice(
        rel,
        RODATA,
        layout.surface_terrain + 2 * SURFACES,
        2 * WORLD_ROWS * WORLD_COLUMNS,
    )?
    .chunks_exact(WORLD_ROWS * WORLD_COLUMNS)
    .enumerate()
    .map(|(world, grid)| {
        ensure!(
            grid.iter().all(|&area| worlds[world]
                .areas
                .get(usize::from(area))
                .is_some_and(Option::is_some)),
            "world grid selects an absent encounter area"
        );
        if world == 1 {
            ensure!(
                grid.iter().all(|&area| worlds[2]
                    .areas
                    .get(usize::from(area))
                    .is_some_and(Option::is_some)),
                "world grid selects an absent late encounter area"
            );
        }
        grid.chunks_exact(WORLD_COLUMNS)
            .map(|row| Ok(row.try_into()?))
            .collect::<Result<_>>()
    })
    .collect::<Result<_>>()?;
    let terrains = TERRAIN
        .into_iter()
        .enumerate()
        .map(|(index, terrain)| {
            Ok(TerrainLabel {
                terrain,
                name: name(rel, layout.terrain_names + index * 4)?
                    .context("missing terrain label")?,
            })
        })
        .collect::<Result<_>>()?;
    let endpoint = |bytes: &[u8]| -> Result<_> {
        Ok(TravelEndpoint {
            map_x: word(bytes, 0)? as i32,
            map_z: word(bytes, 4)? as i32,
            height: crate::read::f32(bytes, 8)?,
            unused_word: word(bytes, 12)?,
        })
    };
    let travel_points = slice(
        rel,
        RODATA,
        TRAVEL_POINTS_TABLE,
        TRAVEL_POINTS * TRAVEL_PAIR_BYTES,
    )?
    .chunks_exact(TRAVEL_PAIR_BYTES)
    .map(|pair| Ok([endpoint(&pair[..16])?, endpoint(&pair[16..])?]))
    .collect::<Result<_>>()?;
    let bytes = slice(
        rel,
        DATA,
        layout.tile_loading_order,
        size_of::<TileLoadingOrder>(),
    )?;
    let mut tile_loading_order = TileLoadingOrder::default();
    for (offset, &byte) in tile_loading_order
        .iter_mut()
        .flatten()
        .flatten()
        .flatten()
        .zip(bytes)
    {
        *offset = byte as i8;
    }
    Ok(Tables {
        terrains,
        worlds,
        surface_terrain,
        area_grids,
        travel_points,
        tile_loading_order,
    })
}

pub(super) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some(layout) = layout(file) else {
        return Ok(None);
    };
    let rel = Rel::read(file)?;
    let tables = read(&rel, layout)?;
    embedded::write(file, output, "overworld-encounters", &tables).map(Some)
}

#[test]
#[ignore = "requires both original extracted discs; no media conversion"]
fn original_overworld_encounters_cover_all_field_modules() -> Result<()> {
    use std::fs;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let output = crate::temporary_path(&std::env::temp_dir().join("overworld-encounters"));
    let mut publications = BTreeSet::new();
    let mut travel_points = BTreeSet::new();
    for disc in [1, 2] {
        for module in [
            "Top2field.rel",
            "Top2fieldD.rel",
            "US_Top2field.rel",
            "US_m_Top2field.rel",
            "US_r_Top2field.rel",
            "m_Top2field.rel",
            "r_Top2field.rel",
        ] {
            let file = root.join(format!("disc{disc}/files/{module}"));
            let mut rel = Rel::read(&file)?;
            let layout = layout(&file).unwrap();
            assert_eq!(
                crate::digest(slice(
                    &rel,
                    DATA,
                    layout.area_rows,
                    3 * AREA_COUNT * AREA_BYTES
                )?),
                "6abd7cc56d443d69abf97b5df29330da8b5d95fa10ec1afce6714cc93620d261"
            );
            assert_eq!(
                crate::digest(slice(
                    &rel,
                    RODATA,
                    layout.surface_terrain,
                    2 * SURFACES + 2 * WORLD_ROWS * WORLD_COLUMNS
                )?),
                "62fe4b798969277f88e5e0a4bd79d4c3985f77167c65f8e30a13991e74a0bdeb"
            );
            let tables = read(&rel, layout)?;
            let loading_order = slice(&rel, DATA, layout.tile_loading_order, 576)?;
            assert_eq!(
                crate::digest(loading_order),
                "7886a13a9b12940a32eb9b0d74b728e5a45bf810c8b540fde5c8cf19b945a9fe"
            );
            for (heading, quadrants) in tables.tile_loading_order.iter().enumerate() {
                for (quadrant, priorities) in quadrants.iter().enumerate() {
                    for (priority, pair) in priorities.iter().enumerate() {
                        // Native addressing: heading*0x48 + (quadrant+1)*0x12 - 0x12.
                        let at = heading * 0x48 + quadrant * 0x12 + priority * 2;
                        assert_eq!(
                            pair.map(i16::from),
                            std::array::from_fn(|axis| {
                                let byte = loading_order[at + axis];
                                i16::from(byte) - if byte >= 128 { 256 } else { 0 }
                            })
                        );
                    }
                }
            }
            let travel = slice(
                &rel,
                RODATA,
                TRAVEL_POINTS_TABLE,
                TRAVEL_POINTS * TRAVEL_PAIR_BYTES,
            )?;
            assert_eq!(
                crate::digest(travel),
                "84dd0cf6ad294be533ae74ceb4f8cace7f92513594f7f9d77eb0d9ca3db11b7b"
            );
            assert_eq!(tables.travel_points.len(), 8);
            let points: Vec<[TravelEndpoint; 2]> =
                serde_json::from_slice(&serde_json::to_vec(&tables.travel_points)?)?;
            for (point, bytes) in points.iter().flatten().zip(travel.chunks_exact(16)) {
                let source: [u32; 4] = std::array::from_fn(|i| {
                    u32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                assert_eq!(
                    [
                        point.map_x as u32,
                        point.map_z as u32,
                        point.height.to_bits(),
                        point.unused_word
                    ],
                    source
                );
            }
            travel_points.insert(serde_json::to_string(&tables.travel_points)?);
            assert_eq!(tables.worlds.len(), 3);
            assert_eq!(tables.worlds[0].name, "シルヴァラント");
            assert_eq!(
                tables.worlds[0].area_names[0].as_deref(),
                Some("イセリア周辺")
            );
            assert!(tables.worlds[0].area_names[11].is_none());
            assert_eq!(
                tables.worlds[0].areas[0].as_ref().unwrap().encounter_groups[0],
                [501, 502]
            );
            assert_eq!(
                tables.worlds[0].arena_by_terrain,
                [1, 0, 0, 0, 5, 2, 2, 3, 0, 4]
            );
            assert_eq!(
                tables.worlds[1].arena_by_terrain,
                [7, 6, 11, 12, 0, 8, 8, 9, 6, 10]
            );
            assert_eq!(
                tables.worlds[1].arena_by_terrain,
                tables.worlds[2].arena_by_terrain
            );
            for world in &tables.worlds {
                assert_eq!(world.area_names.len(), AREA_COUNT);
                assert_eq!(world.areas.len(), AREA_SLOTS);
                assert!(world.areas[..AREA_COUNT].iter().all(Option::is_some));
                assert!(world.areas[AREA_COUNT..].iter().all(Option::is_none));
            }
            assert_eq!(
                tables
                    .terrains
                    .iter()
                    .map(|label| label.name.as_str())
                    .collect::<Vec<_>>(),
                [
                    "草原",
                    "街道",
                    "荒地",
                    "雪原",
                    "砂漠",
                    "森",
                    "深い森",
                    "砂浜",
                    "橋",
                    "山岳"
                ]
            );
            assert_eq!(tables.surface_terrain.len(), 2);
            assert_eq!(tables.surface_terrain[0][8], None);
            assert_eq!(tables.surface_terrain[0][10], Some(Terrain::Beach));
            assert_eq!(tables.area_grids.len(), 2);
            assert!(
                tables
                    .area_grids
                    .iter()
                    .all(|grid| grid.len() == WORLD_ROWS)
            );
            let paths = cook(&file, &output)?.unwrap();
            assert_eq!(paths.len(), 2);
            assert_eq!(
                fs::read(output.join(&paths[0]))?,
                serde_json::to_vec(&tables)?
            );
            let provenance: serde_json::Value =
                serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
            assert_eq!(provenance["module"], module);

            publications.insert(paths[0].clone());
            let travel_at = rel.sections[RODATA].0 + TRAVEL_POINTS_TABLE;
            for offset in [8, 24] {
                let original = rel.bytes[travel_at + offset..travel_at + offset + 4].to_vec();
                rel.bytes[travel_at + offset..travel_at + offset + 4]
                    .copy_from_slice(&f32::NAN.to_be_bytes());
                assert!(read(&rel, layout).is_err());
                rel.bytes[travel_at + offset..travel_at + offset + 4].copy_from_slice(&original);
            }
            let storage = rel.bytes[travel_at + 12..travel_at + 16].to_vec();
            rel.bytes[travel_at + 12..travel_at + 16].fill(0xff);
            assert_eq!(
                read(&rel, layout)?.travel_points[0][0].unused_word,
                u32::MAX
            );
            rel.bytes[travel_at + 12..travel_at + 16].copy_from_slice(&storage);
            // Missing/out-of-range relocations and invalid grid/surface selectors
            // must be reported; they must not turn into missing rows at runtime.
            let root_pointer = rel.pointers.remove(&(DATA, layout.worlds)).unwrap();
            assert!(read(&rel, layout).is_err());
            rel.pointers.insert((DATA, layout.worlds), root_pointer);
            let row_pointer = rel
                .pointers
                .insert(root_pointer, (DATA, layout.area_rows + 1))
                .unwrap();
            assert!(read(&rel, layout).is_err());
            rel.pointers.insert(root_pointer, row_pointer);
            let surface = rel.sections[RODATA].0 + layout.surface_terrain;
            let original_surface = rel.bytes[surface];
            rel.bytes[surface] = TERRAINS as u8;
            assert!(read(&rel, layout).is_err());
            rel.bytes[surface] = original_surface;
            rel.bytes[surface + 2 * SURFACES] = AREA_SLOTS as u8;
            assert!(read(&rel, layout).is_err());
        }
    }
    assert_eq!(
        publications.len(),
        1,
        "all shipped tables have the same authored values"
    );
    assert_eq!(
        travel_points.len(),
        1,
        "all modules retain the same endpoint pairs"
    );
    fs::remove_dir_all(output)?;
    Ok(())
}
