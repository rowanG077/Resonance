//! Named world authoring roots. Terrain, models and rules have explicit owners;
//! preparation follows current references instead of a persisted file inventory.
use super::*;
use crate::{
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::Context;
use std::collections::BTreeMap;

pub const PATH: &str = "world/world.json";
pub const MAPPINGS: &str = "world/mappings.json";
pub const ACTORS: &str = "world/actors/mappings.json";
pub const MARKERS: &str = "world/markers/mappings.json";
pub const CINEMATICS: &str = "world/cinematics/mappings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub script: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "crate::field_audio::document::Definition::is_empty"
    )]
    pub audio: crate::field_audio::document::Definition,
    pub movement: String,
    pub encounters: String,
    pub landmarks: String,
    pub continents: String,
    pub actors: String,
    pub markers: String,
    pub cinematics: String,
    pub dust: [String; 4],
    pub skies: [String; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Continent {
    /// Row-major positions retain their native travel/collision identity.
    pub terrain: Vec<Tile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tile {
    pub column: u8,
    pub row: u8,
    pub base: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terrain {
    pub collision: String,
    pub parts: Vec<crate::ScenePart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub parts: Vec<crate::ScenePart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presentation {
    pub world: World,
    pub presentation: CinematicPresentation,
    pub camera: String,
    pub actors: BTreeMap<u8, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dialogue: Vec<CinematicDialogue>,
}

impl Continent {
    pub fn validate(&self, path: &str) -> Result<()> {
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
        for path in [
            &self.script,
            &self.movement,
            &self.encounters,
            &self.landmarks,
            &self.continents,
            &self.actors,
            &self.markers,
            &self.cinematics,
        ]
        .into_iter()
        .chain(&self.messages)
        .chain(&self.dust)
        .chain(&self.skies)
        {
            crate::library::validate_path(path)?;
        }
        Ok(())
    }
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
                camera: crate::camera_motion::CameraMotion::decode(
                    &snapshot.read(&presentation.camera, ASSET_LIMIT)?,
                )?,
                dialogue: presentation.dialogue,
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
        .chain([&definition.script])
        .chain(&definition.messages)
    {
        ensure!(!cancelled(), "world preparation cancelled");
        snapshot
            .read(path, ASSET_LIMIT)
            .with_context(|| format!("{PATH} refers to {path}"))?;
    }
    let package = Package {
        script: definition.script.clone(),
        messages: definition.messages.clone(),
        movement: snapshot.json(&definition.movement, JSON_LIMIT)?,
        encounters: snapshot.json(&definition.encounters, JSON_LIMIT)?,
        landmarks: snapshot.json(&definition.landmarks, JSON_LIMIT)?,
        worlds,
        visuals: Visuals {
            dust: definition.dust.clone(),
            actors,
            markers: capture_markers(snapshot, &definition.markers, cancelled)?,
            skies,
            cinematics,
        },
    };
    // Every semantic reference was read above; there is no inventory to consult.
    package.validate_content(|_| true)?;
    ensure!(!cancelled(), "world preparation cancelled");
    Ok((definition, package))
}

pub fn capture_markers(
    snapshot: &mut Snapshot,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<[BTreeMap<u8, Vec<crate::ScenePart>>; 2]> {
    let mut worlds: [BTreeMap<u8, Vec<crate::ScenePart>>; 2] = Default::default();
    for id in entries(snapshot, path)? {
        // As with location IDs, the upper byte identifies the world. This keeps
        // ordinary per-ID mod overrides and lets both worlds share one owner.
        let world = worlds
            .get_mut((id / 256) as usize)
            .context("unknown marker world")?;
        let marker = (id % 256) as u8;
        ensure!(marker != 0, "world marker zero is reserved");
        let owner = snapshot.resolve(path, id, Some("model.json"))?;
        world.insert(marker, model(snapshot, &owner, cancelled)?);
    }
    Ok(worlds)
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
    let ground = super::collision_mesh::decode(&snapshot.read(&terrain.collision, ASSET_LIMIT)?)
        .with_context(|| format!("{path} collision {}", terrain.collision))?;
    Ok(TileResources {
        ground,
        parts: prepare_parts(snapshot, terrain.parts, path, cancelled)?,
    })
}

fn model(
    snapshot: &mut Snapshot,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<crate::ScenePart>> {
    ensure!(!cancelled(), "world model preparation cancelled");
    let model: Model = snapshot.json(path, JSON_LIMIT)?;
    prepare_parts(snapshot, model.parts, path, cancelled)
}

fn prepare_parts(
    snapshot: &mut Snapshot,
    mut parts: Vec<crate::ScenePart>,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<crate::ScenePart>> {
    for part in &mut parts {
        part.prepare_bones(&snapshot.read(&part.mesh, ASSET_LIMIT)?)
            .with_context(|| format!("{path} mesh {}", part.mesh))?;
        part.validate(|_| true).with_context(|| path.to_owned())?;
        for resource in &part.textures {
            ensure!(!cancelled(), "world model preparation cancelled");
            snapshot
                .read(resource, ASSET_LIMIT)
                .with_context(|| format!("{path} refers to {resource}"))?;
        }
        for clip in &mut part.clips {
            ensure!(!cancelled(), "world motion preparation cancelled");
            let motion = clip
                .decode_motion(&snapshot.read(&clip.motion, ASSET_LIMIT)?, &part.bone_names)
                .with_context(|| format!("{path} motion {}", clip.motion))?;
            clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
        }
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS; loads only the named world markers"]
    fn named_world_marker_mods_share_owners_and_override_one_machine_id() -> Result<()> {
        use crate::library::Library;
        use std::{fs, path::Path};
        let root =
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
        let root = Path::new(&root);
        let modifications = tempfile::tempdir()?;
        let library = Library::open_paths(root, &[modifications.path().to_owned()])?;
        let mut retained = library.snapshot();
        let before = capture_markers(&mut retained, MARKERS, &|| false)?;
        assert_eq!(before.each_ref().map(BTreeMap::len), [17, 10]);
        let path = "world/markers/stone-guidepost/model.json";
        let mut model: Model = serde_json::from_slice(&fs::read(root.join(path))?)?;
        model.parts[0].translation = [10., 20., 30.];
        fs::create_dir_all(modifications.path().join(path).parent().unwrap())?;
        fs::write(modifications.path().join(path), serde_json::to_vec(&model)?)?;
        let shared = capture_markers(&mut library.snapshot(), MARKERS, &|| false)?;
        assert_eq!(shared[0][&16][0].translation, [10., 20., 30.]);
        assert_eq!(shared[1][&16][0].translation, [10., 20., 30.]);
        model.parts[0].translation = [40., 50., 60.];
        let separate = modifications
            .path()
            .join("world/markers/my-guidepost/model.json");
        fs::create_dir_all(separate.parent().unwrap())?;
        fs::write(separate, serde_json::to_vec(&model)?)?;
        fs::write(
            modifications.path().join(MARKERS),
            br#"{"272":"my-guidepost"}"#,
        )?;
        let changed = capture_markers(&mut library.snapshot(), MARKERS, &|| false)?;
        assert_eq!(changed[0][&16][0].translation, [10., 20., 30.]);
        assert_eq!(changed[1][&16][0].translation, [40., 50., 60.]);
        for world in 0..2 {
            for (&id, original) in &before[world] {
                if id != 16 {
                    assert_eq!(
                        serde_json::to_value(original)?,
                        serde_json::to_value(&changed[world][&id])?
                    );
                }
            }
        }
        assert_eq!(
            serde_json::to_value(capture_markers(&mut retained, MARKERS, &|| false)?)?,
            serde_json::to_value(before)?
        );
        Ok(())
    }

    #[test]
    fn continent_coordinates_are_complete_ordered_and_path_safe() -> Result<()> {
        let original = Continent {
            terrain: (0..TILE_COLUMNS * TILE_ROWS)
                .map(|index| Tile {
                    column: (index % TILE_COLUMNS) as u8,
                    row: (index / TILE_COLUMNS) as u8,
                    base: format!("world/test/terrain/{index}/terrain.json"),
                    alternate: None,
                })
                .collect(),
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
