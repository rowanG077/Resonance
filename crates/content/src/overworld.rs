//! World terrain and travel tables shared by cooking and gameplay.
//! Coordinates use the original world plane: horizontal X/Z followed by height.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
pub mod collision_mesh;
pub mod encounters;
mod movement;
mod surface;
pub use movement::{
    CameraDistances, CollisionSettings, FlightZone, FlightZones, MountDistances, MovementParameters,
};
pub use surface::TerrainSurface;
mod position;
pub use position::{Position, TileCoordinate, World};
mod travel;
pub use travel::{MapDisplay, Mount, TravelState};
mod landmarks;
pub use landmarks::{
    EntryOverride, EntryPlacement, FlightFloor, Guidepost, Interaction, Landmark, Landmarks,
    Marker, PartyCharacter, ShipCameraClearance, ShopVariant,
};
mod cinematic;
pub mod document;
mod package;
pub use cinematic::*;
pub use package::{Cinematic, CinematicDialogue, Package, TerrainTile, TileResources, Visuals};

pub const TILE_COLUMNS: usize = 12;
pub const TILE_ROWS: usize = 9;
pub const TILE_SIZE: f32 = 6400.;
pub const WORLD_WIDTH: f32 = TILE_SIZE * TILE_COLUMNS as f32;
pub const WORLD_DEPTH: f32 = TILE_SIZE * TILE_ROWS as f32;
pub const AREA_COLUMNS: usize = 24;
pub const AREA_ROWS: usize = 18;
pub const SURFACES: usize = TerrainSurface::ALL.len();
pub const TERRAINS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldKind {
    Sylvarant,
    TetheAlla,
    /// Encounter table only; shares Tethe'alla's terrain and region grid.
    TetheAllaLate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
pub struct EncounterTables {
    pub worlds: Vec<EncounterWorld>,
    /// Null is the authored sentinel that suppresses encounters.
    pub surface_terrain: Vec<[Option<Terrain>; SURFACES]>,
    pub area_grids: Vec<Vec<[u8; AREA_COLUMNS]>>,
    /// Paired embark/disembark endpoints in their authored order.
    pub travel_points: Vec<[TravelEndpoint; 2]>,
    pub later_story: i32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TravelEndpoint {
    pub map_x: i32,
    pub map_z: i32,
    pub height: f32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncounterWorld {
    pub kind: WorldKind,
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
                        && !world.areas.is_empty()
                        && world.areas.len() <= 256
                        && world.area_names.len() == world.areas.len()),
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
            self.travel_points
                .iter()
                .flatten()
                .all(|p| (0..WORLD_WIDTH as i32).contains(&p.map_x)
                    && (0..WORLD_DEPTH as i32).contains(&p.map_z)
                    && p.height.is_finite()),
            "invalid world travel endpoint"
        );
        Ok(())
    }
}
