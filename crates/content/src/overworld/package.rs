//! Assembled world resources. Original archives and relocations stay in the importer.
use super::*;
use crate::{ScenePart, field::CollisionGroup};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct Package {
    pub script: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
    pub movement: MovementParameters,
    pub encounters: EncounterTables,
    pub landmarks: Landmarks,
    /// Row-major terrain, both variants prepared even before their story trigger.
    pub worlds: [Vec<TerrainTile>; 2],
    pub visuals: Visuals,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visuals {
    /// Four dust sprites used by footsteps on sand and snow.
    pub dust: [String; 4],
    /// Native world actor IDs: party 1..9, encounters 100/101, vehicles 200..216.
    pub actors: BTreeMap<u16, Vec<ScenePart>>,
    pub markers: [BTreeMap<u8, Vec<ScenePart>>; 2],
    /// Original normal and late-story sky models.
    pub skies: [Vec<ScenePart>; 2],
    /// Numbered original presentations, selected by native command 0x4c.
    #[serde(default)]
    pub cinematics: BTreeMap<u16, Cinematic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cinematic {
    pub world: World,
    pub presentation: CinematicPresentation,
    /// Resource frames at 30 Hz. The native camera advances twice by 0.25
    /// on each 60 Hz world update; actor clips use the same authored clock.
    pub camera: crate::camera_motion::CameraMotion,
    /// Original archive indices, including gaps and non-actor members.
    pub actors: BTreeMap<u8, Vec<ScenePart>>,
    #[serde(default)]
    pub dialogue: Vec<CinematicDialogue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CinematicDialogue {
    pub tick: u32,
    pub duration: u16,
    pub speaker: u8,
    pub voice: u32,
    pub text: String,
}
impl Cinematic {
    pub fn validate(&self, contains: impl Fn(&str) -> bool + Copy) -> Result<()> {
        self.presentation.validate(&self.actors)?;
        self.camera.validate()?;
        ensure!(
            self.camera.duration_frames() > 0. && self.camera.duration_frames() <= 18000.,
            "invalid world cinematic duration"
        );
        ensure!(!self.actors.is_empty(), "empty world cinematic actors");
        for (&index, parts) in &self.actors {
            ensure!(
                (1..=9).contains(&index) && !parts.is_empty(),
                "invalid cinematic actor slot"
            );
            for part in parts {
                part.validate(contains)?;
            }
        }
        ensure!(
            self.dialogue.windows(2).all(|p| p[0].tick < p[1].tick),
            "unordered cinematic dialogue"
        );
        for line in &self.dialogue {
            ensure!(
                line.tick > 0
                    && line.tick as f32 <= self.camera.duration_frames() * 2.
                    && (1..=255).contains(&line.duration)
                    && (1..=9).contains(&line.speaker)
                    && !line.text.is_empty()
                    && line.text.len() <= 512,
                "invalid cinematic dialogue"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TerrainTile {
    pub column: u8,
    pub row: u8,
    pub base: TileResources,
    pub alternate: Option<TileResources>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TileResources {
    pub ground: Vec<CollisionGroup>,
    /// Local world plane; the renderer supplies wrapped tile placement.
    pub parts: Vec<ScenePart>,
}

impl Package {
    /// Validate gameplay and presentation semantics against the current prepared
    /// resources. Named documents have no cooker inventory or provenance gate.
    pub fn validate_content(&self, contains: impl Fn(&str) -> bool + Copy) -> Result<()> {
        crate::library::validate_path(&self.script)?;
        if let Some(path) = &self.messages {
            crate::library::validate_path(path)?;
        }
        ensure!(
            contains(&self.script) && self.messages.as_deref().is_none_or(contains),
            "world event dependencies are incomplete"
        );
        self.movement.validate()?;
        self.encounters.validate()?;
        self.landmarks.validate()?;
        for floor in self
            .landmarks
            .worlds
            .iter()
            .flatten()
            .filter_map(|l| l.flight_floor)
        {
            ensure!(
                floor.height + self.movement.flight_clearance <= self.movement.maximum_altitude,
                "landmark flight floor exceeds the maximum altitude"
            );
        }
        for world in &self.worlds {
            ensure!(
                world.len() == TILE_COLUMNS * TILE_ROWS,
                "incomplete world terrain"
            );
            for (index, tile) in world.iter().enumerate() {
                ensure!(
                    TileCoordinate::new(tile.column, tile.row)?.index() == index,
                    "unordered world terrain"
                );
                for variant in std::iter::once(&tile.base).chain(&tile.alternate) {
                    ensure!(
                        !variant.ground.is_empty() && !variant.parts.is_empty(),
                        "empty world terrain"
                    );
                    for group in &variant.ground {
                        group.validate()?;
                        TerrainSurface::from_index(group.surface)?;
                    }
                    for part in &variant.parts {
                        part.validate(contains)?;
                    }
                }
            }
        }
        for id in (1..=9).chain([99, 100, 101]).chain(200..=214) {
            ensure!(
                self.visuals
                    .actors
                    .get(&id)
                    .is_some_and(|parts| !parts.is_empty()),
                "missing world actor {id}"
            );
        }
        ensure!(
            self.visuals.skies.iter().all(|sky| !sky.is_empty()),
            "world sky models are missing"
        );
        for path in &self.visuals.dust {
            crate::validate_asset_path(path)?;
            ensure!(contains(path), "world dust texture is missing");
        }
        for part in self
            .visuals
            .actors
            .values()
            .chain(self.visuals.markers.iter().flat_map(|world| world.values()))
            .flatten()
            .chain(self.visuals.skies.iter().flatten())
        {
            part.validate(contains)?;
        }
        ensure!(
            self.visuals.cinematics.len() == 14
                && (513..=526).all(|id| self.visuals.cinematics.contains_key(&id)),
            "incomplete world cinematics"
        );
        for (&id, scene) in &self.visuals.cinematics {
            ensure!((513..=526).contains(&id), "invalid world cinematic ID");
            scene.validate(contains)?;
        }
        Ok(())
    }
}
