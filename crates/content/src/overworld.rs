//! World terrain and travel tables shared by cooking and gameplay.
//! Coordinates use the original world plane: horizontal X/Z followed by height.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
mod movement;
pub use movement::MovementParameters;
mod position;
pub use position::{Position, TileCoordinate, World};
mod travel;
pub use travel::{MapDisplay, Mount, TravelState};
mod landmarks;
pub use landmarks::{Guidepost, Interaction, Landmark, Landmarks, Marker};
mod package;
pub use package::{
    Cinematic, CinematicDialogue, PACKAGE_PATH, PACKAGE_VERSION, Package, TerrainTile,
    TileResources, Visuals,
};

pub const TILE_COLUMNS: usize = 12;
pub const TILE_ROWS: usize = 9;
pub const TILE_SIZE: f32 = 6400.;
pub const WORLD_WIDTH: f32 = TILE_SIZE * TILE_COLUMNS as f32;
pub const WORLD_DEPTH: f32 = TILE_SIZE * TILE_ROWS as f32;
pub const AREA_COLUMNS: usize = 24;
pub const AREA_ROWS: usize = 18;
pub const SURFACES: usize = 16;
pub const TERRAINS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldKind {
    Sylvarant,
    TetheAlla,
    /// Encounter table only; shares Tethe'alla's terrain and region grid.
    TetheAllaLate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terrain {
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
impl Terrain {
    pub const ALL: [Self; TERRAINS] = [
        Self::Grassland,
        Self::Road,
        Self::Wasteland,
        Self::Snowfield,
        Self::Desert,
        Self::Forest,
        Self::DeepForest,
        Self::Beach,
        Self::Bridge,
        Self::Mountains,
    ];

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollisionTables {
    /// Indexed [surface][query mode]. -1 rejects, other values are response classes.
    pub surface_responses: [[i8; 4]; SURFACES],
    pub mode2_probe_half_extent: f32,
    pub other_probe_half_extent: f32,
}
impl CollisionTables {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.surface_responses
                .iter()
                .flatten()
                .all(|v| (-1..16).contains(v)),
            "invalid world collision response class"
        );
        ensure!(
            [self.mode2_probe_half_extent, self.other_probe_half_extent]
                .iter()
                .all(|v| v.is_finite() && *v > 0. && *v < TILE_SIZE / 2.),
            "invalid world collision probe extent"
        );
        Ok(())
    }
}

/// Indexed [heading sector][position quadrant][priority][column, row].
pub type TileLoadingOrder = [[[[i8; 2]; 9]; 4]; 8];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncounterTables {
    pub terrains: Vec<TerrainLabel>,
    pub worlds: Vec<EncounterWorld>,
    /// Null is the authored sentinel that suppresses encounters.
    pub surface_terrain: Vec<[Option<Terrain>; SURFACES]>,
    pub area_grids: Vec<Vec<[u8; AREA_COLUMNS]>>,
    /// Paired embark/disembark endpoints in their authored order.
    pub travel_points: Vec<[TravelEndpoint; 2]>,
    pub tile_loading_order: TileLoadingOrder,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TravelEndpoint {
    pub map_x: i32,
    pub map_z: i32,
    pub height: f32,
    /// Retained for round trips; no surviving travel consumer reads this word.
    pub unused_word: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainLabel {
    pub terrain: Terrain,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncounterWorld {
    pub kind: WorldKind,
    pub name: String,
    pub area_names: Vec<Option<String>>,
    pub areas: Vec<Option<EncounterArea>>,
    pub arena_by_terrain: [u8; TERRAINS],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncounterArea {
    /// Group IDs, not formations: [terrain][enemy symbol kind].
    pub encounter_groups: [[u16; 2]; TERRAINS],
}
impl EncounterTables {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.terrains.len() == TERRAINS
                && self
                    .terrains
                    .iter()
                    .zip(Terrain::ALL)
                    .all(|(label, terrain)| label.terrain == terrain && !label.name.is_empty()),
            "invalid world terrain labels"
        );
        ensure!(
            self.worlds.len() == 3
                && self
                    .worlds
                    .iter()
                    .zip([
                        WorldKind::Sylvarant,
                        WorldKind::TetheAlla,
                        WorldKind::TetheAllaLate
                    ])
                    .all(|(world, kind)| world.kind == kind
                        && !world.name.is_empty()
                        && world.area_names.len() == 12
                        && world.areas.len() == 21),
            "invalid world encounter tables"
        );
        ensure!(
            self.surface_terrain.len() == 2
                && self.area_grids.len() == 2
                && self.area_grids.iter().all(|grid| grid.len() == AREA_ROWS),
            "invalid world region grids"
        );
        for (index, world) in self.worlds.iter().enumerate() {
            ensure!(
                self.area_grids[index.min(1)]
                    .iter()
                    .flatten()
                    .all(|area| world
                        .areas
                        .get(usize::from(*area))
                        .is_some_and(Option::is_some)),
                "world region references an absent encounter area"
            );
        }
        ensure!(
            self.travel_points.len() == 8
                && self
                    .travel_points
                    .iter()
                    .flatten()
                    .all(|p| (0..WORLD_WIDTH as i32).contains(&p.map_x)
                        && (0..WORLD_DEPTH as i32).contains(&p.map_z)
                        && p.height.is_finite()),
            "invalid world travel endpoint"
        );
        for order in self.tile_loading_order.iter().flatten() {
            ensure!(
                order[0] == [0, 0]
                    && order.iter().all(|p| p.iter().all(|v| (-2..=2).contains(v)))
                    && order.iter().collect::<BTreeSet<_>>().len() == 9,
                "invalid world tile loading priorities"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileCatalogue {
    pub axis_labels: String,
    pub worlds: [TileWorld; 2],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileWorld {
    pub kind: WorldKind,
    /// Source provenance; gameplay uses the explicit tile bindings.
    pub templates: [String; 2],
    pub tiles: Vec<Tile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tile {
    pub column: u8,
    pub row: u8,
    pub base: TileAsset,
    /// Presence does not select it; story state selects the variant.
    pub alternate: Option<TileAsset>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileAsset {
    pub source: String,
    pub package: String,
}
impl TileCatalogue {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.axis_labels.len() >= TILE_COLUMNS
                && self.axis_labels.bytes().all(|b| b.is_ascii_alphanumeric())
                && self.axis_labels.bytes().collect::<BTreeSet<_>>().len()
                    == self.axis_labels.len(),
            "invalid world tile axis labels"
        );
        for (world, kind) in self
            .worlds
            .iter()
            .zip([WorldKind::Sylvarant, WorldKind::TetheAlla])
        {
            ensure!(
                world.kind == kind && world.tiles.len() == TILE_COLUMNS * TILE_ROWS,
                "invalid world terrain tile count or world binding"
            );
            for (index, tile) in world.tiles.iter().enumerate() {
                ensure!(
                    usize::from(tile.row) == index / TILE_COLUMNS
                        && usize::from(tile.column) == index % TILE_COLUMNS,
                    "world tiles must occur once in row-major order"
                );
                for asset in std::iter::once(&tile.base).chain(&tile.alternate) {
                    crate::validate_asset_path(&asset.source)?;
                    crate::validate_asset_path(&asset.package)?;
                    ensure!(
                        asset
                            .package
                            .strip_prefix("assets/")
                            .is_some_and(|hash| hash.len() == 64
                                && hash.bytes().all(|b| b.is_ascii_hexdigit())),
                        "invalid world tile package binding"
                    );
                }
            }
        }
        Ok(())
    }
}
