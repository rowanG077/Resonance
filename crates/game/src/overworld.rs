//! Overworld simulation. Rendering and original executable layouts stay outside gameplay.
pub mod camera;
pub mod cinematic;
pub mod collision;
pub mod enemies;
mod entry;
pub mod landmarks;
mod package;
mod portal;
pub mod scripts;
mod session;
pub use package::Prepared;
pub use portal::Portal;
pub use session::{Assets, Checkpoint, Input, Prompt, Session, SpecialEncounter};
pub mod travel;
use anyhow::Result;
use resonance_content::overworld::{EncounterTables, Terrain, TerrainSurface};
pub use resonance_content::overworld::{Position, TileCoordinate, World};

/// The battle owner resolves this group through its weighted formation table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encounter {
    pub group: u16,
    pub arena: u8,
    pub terrain: Terrain,
    pub area: u8,
}

pub struct Rules(EncounterTables);
impl Rules {
    pub fn new(tables: EncounterTables) -> Result<Self> {
        tables.validate()?;
        Ok(Self(tables))
    }

    pub fn area(&self, world: World, position: Position) -> u8 {
        let [column, row] = position.area_cell();
        self.0.area_grids[world.index()][row][column]
    }

    pub fn terrain(&self, world: World, surface: Option<TerrainSurface>) -> Option<Terrain> {
        *self.0.surface_terrain[world.index()].get(surface?.index())?
    }

    /// Guidepost flags unlock Noishe independently for each travel region.
    pub fn noishe_available(
        &self,
        world: World,
        position: Position,
        event_flags: &std::collections::BTreeSet<u16>,
    ) -> bool {
        let region = u16::from(self.area(world, position)) + world.index() as u16 * 11;
        event_flags.contains(&(900 + region))
    }

    /// Rheaird landing requires a ground surface with a mapped terrain. The
    /// caller must also resolve any landmark contact before starting descent.
    pub fn can_land(&self, world: World, surface: Option<TerrainSurface>) -> bool {
        self.terrain(world, surface).is_some()
    }

    /// Embarking requires a pier. The native search chooses the
    /// last nearby endpoint, retaining pair zero if no other pier pair matches.
    pub fn embark_destination(
        &self,
        position: Position,
        surface: Option<TerrainSurface>,
    ) -> Option<Position> {
        if surface != Some(TerrainSurface::Pier) {
            return None;
        }
        let index = self.last_dock(position, 0, 2000).unwrap_or(0);
        self.endpoint(index, 1)
    }

    /// Sailing cannot disembark along an arbitrary coast: only an authored sea
    /// endpoint within the strict 200-unit box offers the paired land position.
    pub fn disembark_destination(&self, position: Position) -> Option<Position> {
        self.endpoint(self.last_dock(position, 1, 200)?, 0)
    }

    fn last_dock(&self, position: Position, side: usize, radius: i32) -> Option<usize> {
        let [x, z, _] = position.map();
        self.0.travel_points.iter().rposition(|pair| {
            (pair[side].map_x - x as i32).abs() < radius
                && (pair[side].map_z - z as i32).abs() < radius
        })
    }

    fn endpoint(&self, pair: usize, side: usize) -> Option<Position> {
        let point = &self.0.travel_points.get(pair)?[side];
        Position::from_map([point.map_x as f32, point.map_z as f32, point.height]).ok()
    }

    /// The player's named collision surface selects battle terrain. The enemy symbol
    /// supplies its variant and location. Neither lookup consumes gameplay RNG.
    pub fn lookup(
        &self,
        world: World,
        position: Position,
        surface: Option<TerrainSurface>,
        symbol_variant: u8,
        story: i32,
    ) -> Option<Encounter> {
        if symbol_variant > 1 {
            return None;
        }
        let terrain = self.terrain(world, surface)?;
        let area = self.area(world, position);
        let table = if world == World::TetheAlla && story >= self.0.later_story {
            2
        } else {
            world.index()
        };
        Some(Encounter {
            group: self.0.worlds[table].areas[usize::from(area)]
                .as_ref()?
                .encounter_groups[terrain.index()][usize::from(symbol_variant)],
            // The later encounter set does not replace the world's arena table.
            arena: self.0.worlds[world.index()].arena_by_terrain[terrain.index()],
            terrain,
            area,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::overworld::{EncounterArea, EncounterWorld, TravelEndpoint, WorldKind};

    pub(super) fn tables() -> EncounterTables {
        let mut tables = EncounterTables {
            worlds: [
                WorldKind::Sylvarant,
                WorldKind::TetheAlla,
                WorldKind::TetheAllaLate,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, kind)| EncounterWorld {
                kind,
                area_names: (0..21)
                    .map(|id| (id < 12).then(|| format!("region-{id}")))
                    .collect(),
                areas: (0..21)
                    .map(|area| {
                        (area < 12).then_some(EncounterArea {
                            encounter_groups: [[
                                1000 + index as u16 * 100 + area * 2,
                                1001 + index as u16 * 100 + area * 2,
                            ]; 10],
                        })
                    })
                    .collect(),
                arena_by_terrain: [index as u8; 10],
            })
            .collect(),
            surface_terrain: vec![
                [Some(Terrain::Grassland); resonance_content::overworld::SURFACES];
                2
            ],
            area_grids: vec![vec![[0; 24]; 18]; 2],
            travel_points: vec![
                [
                    TravelEndpoint {
                        map_x: 0,
                        map_z: 0,
                        height: 0.,
                    },
                    TravelEndpoint {
                        map_x: 200,
                        map_z: 0,
                        height: 0.,
                    }
                ];
                8
            ],
            later_story: 10_405_000,
        };
        tables.surface_terrain[0][TerrainSurface::Water.index()] = None;
        tables.area_grids[0][1][1] = 3;
        tables
    }

    #[test]
    fn region_quadrants_symbols_and_story_switch_select_the_correct_encounter() -> Result<()> {
        let encounters = Rules::new(tables())?;
        let northwest = Position::from_map([3199., 3199., 0.])?;
        let southeast = Position::from_map([3200., 3200., 0.])?;
        assert_eq!(
            encounters
                .lookup(
                    World::Sylvarant,
                    northwest,
                    Some(TerrainSurface::Grassland),
                    0,
                    0
                )
                .unwrap()
                .group,
            1000
        );
        assert_eq!(
            encounters
                .lookup(
                    World::Sylvarant,
                    southeast,
                    Some(TerrainSurface::Grassland),
                    1,
                    0
                )
                .unwrap()
                .group,
            1007
        );
        assert!(
            encounters
                .lookup(
                    World::Sylvarant,
                    southeast,
                    Some(TerrainSurface::Water),
                    0,
                    0
                )
                .is_none()
        );
        assert!(
            encounters
                .lookup(World::Sylvarant, southeast, None, 0, 0)
                .is_none()
        );
        assert!(
            encounters
                .lookup(World::Sylvarant, southeast, None, 0, 0)
                .is_none()
        );
        assert!(
            encounters
                .lookup(
                    World::Sylvarant,
                    southeast,
                    Some(TerrainSurface::Grassland),
                    2,
                    0
                )
                .is_none()
        );
        assert_eq!(
            encounters
                .lookup(
                    World::TetheAlla,
                    southeast,
                    Some(TerrainSurface::Grassland),
                    0,
                    10_404_999
                )
                .unwrap()
                .group,
            1100
        );
        let late = encounters
            .lookup(
                World::TetheAlla,
                southeast,
                Some(TerrainSurface::Grassland),
                1,
                10_405_000,
            )
            .unwrap();
        assert_eq!((late.group, late.arena), (1201, 1));
        assert_eq!(
            encounters
                .lookup(
                    World::Sylvarant,
                    southeast,
                    Some(TerrainSurface::Grassland),
                    0,
                    i32::MAX
                )
                .unwrap()
                .group,
            1006
        );
        Ok(())
    }

    #[test]
    fn mount_permissions_and_docks_follow_region_and_surface_rules() -> Result<()> {
        let mut data = tables();
        for (index, pair) in data.travel_points.iter_mut().enumerate() {
            pair[0].map_x = 1000 + index as i32 * 5000;
            pair[0].height = 75.;
            pair[1].map_x = pair[0].map_x + 300;
        }
        let rules = Rules::new(data)?;
        let pier = Position::from_map([1000., 0., 75.])?;
        assert!(!rules.noishe_available(World::Sylvarant, pier, &Default::default()));
        assert!(rules.noishe_available(World::Sylvarant, pier, &[900].into()));
        assert!(!rules.noishe_available(World::TetheAlla, pier, &[900].into()));
        assert!(rules.noishe_available(World::TetheAlla, pier, &[911].into()));
        assert!(rules.can_land(World::Sylvarant, Some(TerrainSurface::Grassland)));
        assert!(!rules.can_land(World::Sylvarant, Some(TerrainSurface::Water)));
        assert!(!rules.can_land(World::Sylvarant, None));
        assert!(
            rules
                .embark_destination(pier, Some(TerrainSurface::Grassland))
                .is_none()
        );
        let sea = rules
            .embark_destination(pier, Some(TerrainSurface::Pier))
            .unwrap();
        assert_eq!(sea.map(), [1300., 0., 0.]);
        assert_eq!(rules.disembark_destination(sea), Some(pier));
        assert!(
            rules
                .disembark_destination(sea.translated([200., 0., 0.])?)
                .is_none()
        );
        assert_eq!(
            rules.disembark_destination(sea.translated([199., 0., 0.])?),
            Some(pier)
        );
        Ok(())
    }

    #[test]
    fn overlapping_docks_choose_the_last_pair_and_piers_keep_the_native_fallback() -> Result<()> {
        let mut data = tables();
        data.travel_points[7][1].map_x = 250;
        let rules = Rules::new(data)?;
        assert_eq!(
            rules
                .embark_destination(Position::from_map([0.; 3])?, Some(TerrainSurface::Pier))
                .unwrap()
                .map()[0],
            250.
        );
        assert_eq!(
            rules
                .embark_destination(
                    Position::from_map([50000., 10000., 0.])?,
                    Some(TerrainSurface::Pier)
                )
                .unwrap()
                .map()[0],
            200.
        );
        assert!(
            rules
                .disembark_destination(Position::from_map([50000., 10000., 0.])?)
                .is_none()
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires current named world metadata and its saved prior table; no media loading"]
    fn named_world_rules_preserve_every_cell_surface_symbol_and_story_selection() -> Result<()> {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let before: serde_json::Value = serde_json::from_slice(&std::fs::read(
            repo.join("local/structure-review-20261027/world-rules/encounters-before.json"),
        )?)?;
        let bytes = std::fs::read(repo.join("local/layout-migration/final-integration-20261025-supervised/library/world/encounters.json"))?;
        let current = Rules::new(serde_json::from_slice(&bytes)?)?;
        for world in [World::Sylvarant, World::TetheAlla] {
            for row in 0..18 {
                for column in 0..24 {
                    let position = Position::from_map([
                        column as f32 * 3200. + 1.,
                        row as f32 * 3200. + 1.,
                        0.,
                    ])?;
                    let area = before["area_grids"][world.index()][row][column]
                        .as_u64()
                        .unwrap() as usize;
                    assert_eq!(current.area(world, position), area as u8);
                    for (surface, native) in std::iter::once((None, None)).chain(
                        [
                            (TerrainSurface::Grassland, 1),
                            (TerrainSurface::Road, 2),
                            (TerrainSurface::Wasteland, 3),
                            (TerrainSurface::Snowfield, 4),
                            (TerrainSurface::Desert, 5),
                            (TerrainSurface::Forest, 6),
                            (TerrainSurface::DeepForest, 7),
                            (TerrainSurface::Beach, 10),
                            (TerrainSurface::Water, 11),
                            (TerrainSurface::Bridge, 13),
                            (TerrainSurface::Pier, 15),
                        ]
                        .into_iter()
                        .map(|(surface, native)| (Some(surface), Some(native))),
                    ) {
                        let terrain: Option<Terrain> = native
                            .map(|native| {
                                serde_json::from_value(
                                    before["surface_terrain"][world.index()][native].clone(),
                                )
                            })
                            .transpose()?
                            .flatten();
                        for symbol in 0..=2 {
                            for story in [10_404_999, 10_405_000] {
                                let actual =
                                    current.lookup(world, position, surface, symbol, story);
                                if symbol > 1 || terrain.is_none() {
                                    assert!(actual.is_none());
                                    continue;
                                }
                                let terrain = terrain.unwrap();
                                let table = if world == World::TetheAlla && story >= 10_405_000 {
                                    2
                                } else {
                                    world.index()
                                };
                                assert_eq!(actual, Some(Encounter {
                                    group: before["worlds"][table]["areas"][area]["encounter_groups"][terrain.index()][symbol as usize].as_u64().unwrap() as u16,
                                    arena: before["worlds"][world.index()]["arena_by_terrain"][terrain.index()].as_u64().unwrap() as u8,
                                    terrain, area: area as u8,
                                }));
                            }
                        }
                    }
                }
            }
        }
        let mut edited: serde_json::Value = serde_json::from_slice(&bytes)?;
        edited["tethe_alla"]["regions"]["fooji-mountains"]["groups"]["grassland"] =
            serde_json::json!([321, 322]);
        edited["later_story"] = 50.into();
        let changed = Rules::new(serde_json::from_value(edited)?)?;
        for (row, line) in current.0.area_grids[1].iter().enumerate() {
            for (column, &area) in line.iter().enumerate().filter(|(_, area)| **area != 6) {
                let position =
                    Position::from_map([column as f32 * 3200. + 1., row as f32 * 3200. + 1., 0.])?;
                assert_eq!(
                    changed
                        .lookup(
                            World::TetheAlla,
                            position,
                            Some(TerrainSurface::Grassland),
                            1,
                            49
                        )
                        .unwrap()
                        .group,
                    322,
                    "region {area}"
                );
                assert_eq!(
                    current
                        .lookup(
                            World::TetheAlla,
                            position,
                            Some(TerrainSurface::Grassland),
                            1,
                            49
                        )
                        .unwrap()
                        .group,
                    632
                );
                assert_eq!(
                    changed.lookup(
                        World::TetheAlla,
                        position,
                        Some(TerrainSurface::Grassland),
                        1,
                        50
                    ),
                    current.lookup(
                        World::TetheAlla,
                        position,
                        Some(TerrainSurface::Grassland),
                        1,
                        10_405_000
                    )
                );
            }
        }
        let mut no_docks = current.0.clone();
        no_docks.travel_points.clear();
        let no_docks = Rules::new(no_docks)?;
        let position = Position::from_map([0.; 3])?;
        assert!(
            no_docks
                .embark_destination(position, Some(TerrainSurface::Pier))
                .is_none()
        );
        assert!(no_docks.disembark_destination(position).is_none());
        Ok(())
    }

    #[test]
    fn preparation_rejects_incomplete_and_dangling_encounter_data() {
        let mut invalid = tables();
        invalid.area_grids[0][0][0] = 20;
        assert!(Rules::new(invalid).is_err());
        let mut invalid = tables();
        invalid.area_grids[1].pop();
        assert!(Rules::new(invalid).is_err());
        let mut invalid = tables();
        invalid.worlds.swap(1, 2);
        assert!(Rules::new(invalid).is_err());
    }
}
