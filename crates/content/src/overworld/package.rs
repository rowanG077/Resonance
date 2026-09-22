//! Runtime world inventory. Original archives and relocations stay in the importer.
use super::*;
use crate::{ScenePart, ScriptAsset, field::CollisionGroup, field_preload::File};
use std::collections::BTreeMap;

pub const PACKAGE_PATH: &str = "worlds/world.json";
pub const PACKAGE_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub version: u32,
    pub script: ScriptAsset,
    pub messages: String,
    pub movement: MovementParameters,
    pub collision: CollisionTables,
    pub encounters: EncounterTables,
    pub landmarks: Landmarks,
    pub guideposts: Vec<Guidepost>,
    /// Row-major terrain, both variants prepared even before their story trigger.
    pub worlds: [Vec<TerrainTile>; 2],
    pub visuals: Visuals,
    pub files: BTreeMap<String, File>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visuals {
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
    /// Resource frames at 30 Hz. The native camera advances twice by 0.25
    /// on each 60 Hz world update; actor clips use the same authored clock.
    pub camera: Vec<crate::CameraKey>,
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
        ensure!(self.camera.len() >= 2, "empty world cinematic camera");
        ensure!(
            self.camera[0].time == 0.,
            "world cinematic camera starts late"
        );
        for key in &self.camera {
            ensure!(
                key.time.is_finite()
                    && key.time >= 0.
                    && key
                        .position
                        .iter()
                        .chain(&key.target)
                        .all(|v| v.is_finite()),
                "invalid world cinematic camera"
            );
        }
        ensure!(
            self.camera.windows(2).all(|k| k[0].time < k[1].time),
            "unordered world cinematic camera"
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
                    && line.tick as f32 <= self.camera.last().unwrap().time * 2.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainTile {
    pub column: u8,
    pub row: u8,
    pub base: TileResources,
    pub alternate: Option<TileResources>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TileResources {
    pub source_sha256: String,
    pub ground: Vec<CollisionGroup>,
    /// Local world plane; the renderer supplies wrapped tile placement.
    pub parts: Vec<ScenePart>,
}

impl Package {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == PACKAGE_VERSION, "unsupported world package");
        self.movement.validate()?;
        self.collision.validate()?;
        self.encounters.validate()?;
        self.landmarks.validate()?;
        let mut posts = BTreeSet::new();
        for post in &self.guideposts {
            post.validate(&self.landmarks)?;
            ensure!(posts.insert(post.location), "duplicate world guidepost");
        }
        for (path, file) in &self.files {
            crate::validate_asset_path(path)?;
            ensure!(
                path != PACKAGE_PATH
                    && file.sha256.len() == 64
                    && file.sha256.bytes().all(|c| c.is_ascii_hexdigit())
                    && !file.roles.is_empty(),
                "invalid world dependency"
            );
        }
        ensure!(
            self.files
                .get(&self.script.path)
                .is_some_and(|file| file.sha256 == self.script.sha256)
                && self.files.contains_key(&self.messages),
            "world event dependencies are incomplete"
        );
        for path in [
            "game/session-data.json",
            "game/text.json",
            "game/skits.json",
            "game/menu-data.json",
            "fonts/dialogue.json",
            "ui/dialogue.json",
            "ui/menu.json",
            "worlds/audio.json",
        ] {
            ensure!(
                self.files.contains_key(path),
                "world dependency {path} is missing"
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
                        variant.source_sha256.len() == 64
                            && variant.source_sha256.bytes().all(|c| c.is_ascii_hexdigit()),
                        "invalid terrain source digest"
                    );
                    ensure!(
                        !variant.ground.is_empty() && !variant.parts.is_empty(),
                        "empty world terrain"
                    );
                    for group in &variant.ground {
                        group.validate()?;
                        ensure!(group.surface < 16, "invalid terrain surface");
                    }
                    for part in &variant.parts {
                        part.validate(|path| self.files.contains_key(path))?;
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
        for part in self
            .visuals
            .actors
            .values()
            .chain(self.visuals.markers.iter().flat_map(|world| world.values()))
            .flatten()
            .chain(self.visuals.skies.iter().flatten())
        {
            part.validate(|path| self.files.contains_key(path))?;
        }
        ensure!(
            self.visuals.cinematics.len() == 14
                && (513..=526).all(|id| self.visuals.cinematics.contains_key(&id)),
            "incomplete world cinematics"
        );
        ensure!(
            self.visuals.cinematics[&517].dialogue.len() == 9,
            "missing world cinematic dialogue"
        );
        for (&id, scene) in &self.visuals.cinematics {
            ensure!((513..=526).contains(&id), "invalid world cinematic ID");
            scene.validate(|path| self.files.contains_key(path))?;
        }
        Ok(())
    }
}
