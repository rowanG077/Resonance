//! Named world authoring roots. Terrain, models and rules have explicit owners;
//! preparation follows current references instead of a persisted file inventory.
use super::*;
use crate::{
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::Context;
use std::collections::BTreeMap;
mod identity;
pub use identity::gameplay;

pub const PATH: &str = "world/world.json";
pub const VERSION: u32 = 1;
pub const MAPPINGS: &str = "world/mappings.json";
pub const ACTORS: &str = "world/actors/mappings.json";
pub const CINEMATICS: &str = "world/cinematics/mappings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub script: String,
    pub messages: String,
    pub audio: String,
    pub movement: String,
    pub collision: String,
    pub encounters: String,
    pub landmarks: String,
    pub guideposts: String,
    pub continents: String,
    pub actors: String,
    pub cinematics: String,
    pub dust: [String; 4],
    pub skies: [String; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Continent {
    pub version: u32,
    /// Row-major positions retain their native travel/collision identity.
    pub terrain: Vec<Tile>,
    pub markers: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tile {
    pub column: u8,
    pub row: u8,
    pub base: String,
    pub alternate: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terrain {
    pub version: u32,
    pub collision: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub version: u32,
    pub parts: Vec<crate::ScenePart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presentation {
    pub version: u32,
    pub world: World,
    pub presentation: CinematicPresentation,
    pub camera: String,
    pub actors: BTreeMap<u8, String>,
    pub dialogue: String,
}

impl Continent {
    pub fn validate(&self, path: &str) -> Result<()> {
        version(self.version, path)?;
        crate::library::validate_path(&self.markers)?;
        ensure!(
            self.terrain.len() == TILE_COLUMNS * TILE_ROWS,
            "incomplete world terrain in {path}"
        );
        for (index, tile) in self.terrain.iter().enumerate() {
            ensure!(
                TileCoordinate::new(tile.column, tile.row)?.index() == index,
                "unordered world terrain in {path}"
            );
            for reference in std::iter::once(&tile.base).chain(&tile.alternate) {
                crate::library::validate_path(reference)?;
            }
        }
        Ok(())
    }
}

impl Definition {
    pub fn validate(&self) -> Result<()> {
        version(self.version, PATH)?;
        for path in [
            &self.script,
            &self.messages,
            &self.audio,
            &self.movement,
            &self.collision,
            &self.encounters,
            &self.landmarks,
            &self.guideposts,
            &self.continents,
            &self.actors,
            &self.cinematics,
        ]
        .into_iter()
        .chain(&self.dust)
        .chain(&self.skies)
        {
            crate::library::validate_path(path)?;
        }
        Ok(())
    }
}

fn version(value: u32, path: &str) -> Result<()> {
    ensure!(
        value == VERSION,
        "unsupported world document version in {path}"
    );
    Ok(())
}

pub fn present(snapshot: &mut Snapshot) -> Result<bool> {
    Ok(snapshot.read_optional(PATH, JSON_LIMIT)?.is_some())
}

pub fn capture(
    snapshot: &mut Snapshot,
    cancelled: &impl Fn() -> bool,
) -> Result<(Definition, Package)> {
    ensure!(!cancelled(), "world preparation cancelled");
    let definition: Definition = snapshot.json(PATH, JSON_LIMIT)?;
    definition.validate()?;
    let mut worlds: [Vec<TerrainTile>; 2] = Default::default();
    let mut markers: [BTreeMap<u8, Vec<crate::ScenePart>>; 2] = Default::default();
    let continents = entries(snapshot, &definition.continents)?;
    ensure!(
        continents == [0, 1],
        "world mappings must contain continents 0 and 1"
    );
    for id in continents {
        ensure!(!cancelled(), "world preparation cancelled");
        let path = snapshot.resolve(&definition.continents, id, Some("world.json"))?;
        let continent: Continent = snapshot.json(&path, JSON_LIMIT)?;
        continent.validate(&path)?;
        for tile in continent.terrain {
            worlds[id as usize].push(TerrainTile {
                column: tile.column,
                row: tile.row,
                base: terrain(snapshot, &tile.base, cancelled)?,
                alternate: tile
                    .alternate
                    .as_ref()
                    .map(|path| terrain(snapshot, path, cancelled))
                    .transpose()?,
            });
        }
        for marker in entries(snapshot, &continent.markers)? {
            let path = snapshot.resolve(&continent.markers, marker, Some("model.json"))?;
            markers[id as usize].insert(
                u8::try_from(marker).context("world marker ID exceeds its native range")?,
                model(snapshot, &path, cancelled)?,
            );
        }
    }
    let mut actors = BTreeMap::new();
    for id in entries(snapshot, &definition.actors)? {
        let path = snapshot.resolve(&definition.actors, id, Some("model.json"))?;
        actors.insert(
            u16::try_from(id).context("world actor ID exceeds its native range")?,
            model(snapshot, &path, cancelled)?,
        );
    }
    let mut cinematics = BTreeMap::new();
    for id in entries(snapshot, &definition.cinematics)? {
        let path = snapshot.resolve(&definition.cinematics, id, Some("cinematic.json"))?;
        let presentation: Presentation = snapshot.json(&path, JSON_LIMIT)?;
        version(presentation.version, &path)?;
        let actors = presentation
            .actors
            .iter()
            .map(|(&id, path)| Ok((id, model(snapshot, path, cancelled)?)))
            .collect::<Result<_>>()?;
        cinematics.insert(
            u16::try_from(id).context("world cinematic ID exceeds its native range")?,
            Cinematic {
                world: presentation.world,
                presentation: presentation.presentation,
                actors,
                camera: snapshot.json(&presentation.camera, JSON_LIMIT)?,
                dialogue: snapshot.json(&presentation.dialogue, JSON_LIMIT)?,
            },
        );
    }
    let skies = [
        model(snapshot, &definition.skies[0], cancelled)?,
        model(snapshot, &definition.skies[1], cancelled)?,
    ];
    for path in definition
        .dust
        .iter()
        .chain([&definition.script, &definition.messages])
    {
        ensure!(!cancelled(), "world preparation cancelled");
        snapshot
            .read(path, ASSET_LIMIT)
            .with_context(|| format!("{PATH} refers to {path}"))?;
    }
    let package = Package {
        version: PACKAGE_VERSION,
        script: crate::ScriptAsset {
            path: definition.script.clone(),
            sha256: String::new(),
        },
        messages: definition.messages.clone(),
        movement: snapshot.json(&definition.movement, JSON_LIMIT)?,
        collision: snapshot.json(&definition.collision, JSON_LIMIT)?,
        encounters: snapshot.json(&definition.encounters, JSON_LIMIT)?,
        landmarks: snapshot.json(&definition.landmarks, JSON_LIMIT)?,
        guideposts: snapshot.json(&definition.guideposts, JSON_LIMIT)?,
        worlds,
        visuals: Visuals {
            dust: definition.dust.clone(),
            actors,
            markers,
            skies,
            cinematics,
        },
    };
    // Every semantic reference was read above; there is no inventory to consult.
    package.validate_content(|_| true)?;
    ensure!(!cancelled(), "world preparation cancelled");
    Ok((definition, package))
}

pub(super) fn entries(snapshot: &mut Snapshot, mapping: &str) -> Result<Vec<u32>> {
    Ok(snapshot
        .mappings(mapping)?
        .iter()
        .map(|(id, _)| id)
        .collect())
}

fn terrain(
    snapshot: &mut Snapshot,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<TileResources> {
    ensure!(!cancelled(), "world terrain preparation cancelled");
    let terrain: Terrain = snapshot.json(path, JSON_LIMIT)?;
    version(terrain.version, path)?;
    Ok(TileResources {
        ground: snapshot
            .json(&terrain.collision, JSON_LIMIT)
            .with_context(|| format!("{path} collision {}", terrain.collision))?,
        parts: model(snapshot, &terrain.model, cancelled)
            .with_context(|| format!("{path} model {}", terrain.model))?,
    })
}

fn model(
    snapshot: &mut Snapshot,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<crate::ScenePart>> {
    ensure!(!cancelled(), "world model preparation cancelled");
    let mut model: Model = snapshot.json(path, JSON_LIMIT)?;
    version(model.version, path)?;
    for part in &mut model.parts {
        part.validate(|_| true).with_context(|| path.to_owned())?;
        for resource in std::iter::once(&part.mesh).chain(&part.textures) {
            ensure!(!cancelled(), "world model preparation cancelled");
            snapshot
                .read(resource, ASSET_LIMIT)
                .with_context(|| format!("{path} refers to {resource}"))?;
        }
        for clip in &mut part.clips {
            ensure!(!cancelled(), "world motion preparation cancelled");
            let motion =
                crate::animation::Motion::decode(&snapshot.read(&clip.motion, ASSET_LIMIT)?)
                    .with_context(|| format!("{path} motion {}", clip.motion))?;
            motion.validate_bones(part.bone_names.len())?;
            clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
        }
    }
    Ok(model.parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continent_coordinates_are_complete_ordered_and_path_safe() -> Result<()> {
        let original = Continent {
            version: VERSION,
            terrain: (0..TILE_COLUMNS * TILE_ROWS)
                .map(|index| Tile {
                    column: (index % TILE_COLUMNS) as u8,
                    row: (index / TILE_COLUMNS) as u8,
                    base: format!("world/test/terrain/{index}/terrain.json"),
                    alternate: None,
                })
                .collect(),
            markers: "world/test/markers/mappings.json".into(),
        };
        original.validate("test continent")?;
        let mut invalid = original.clone();
        invalid.terrain.pop();
        assert!(invalid.validate("test continent").is_err());
        let mut invalid = original.clone();
        invalid.terrain.swap(0, 1);
        assert!(invalid.validate("test continent").is_err());
        let mut invalid = original.clone();
        invalid.terrain[1].column = 0;
        assert!(invalid.validate("test continent").is_err());
        let mut invalid = original;
        invalid.terrain[0].alternate = Some("../outside.json".into());
        assert!(invalid.validate("test continent").is_err());
        Ok(())
    }
}
