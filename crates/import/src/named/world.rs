use super::*;
use resonance_content::overworld::{self, document as world};

impl Publisher<'_> {
    pub fn world(&mut self, package: overworld::Package) -> Result<()> {
        package.validate_content(|path| self.source.join(path).is_file())?;
        let definition = world::Definition {
            version: world::VERSION,
            script: "world/scripts/scenario.ssb".into(),
            messages: "world/messages.json".into(),
            audio: "world/audio.json".into(),
            movement: "world/movement.json".into(),
            collision: "world/collision.json".into(),
            encounters: "world/encounters.json".into(),
            landmarks: "world/landmarks.json".into(),
            guideposts: "world/guideposts.json".into(),
            continents: world::MAPPINGS.into(),
            actors: world::ACTORS.into(),
            cinematics: world::CINEMATICS.into(),
            dust: std::array::from_fn(|i| format!("world/effects/dust-{i}")),
            skies: [
                "world/skies/normal/model.json".into(),
                "world/skies/late/model.json".into(),
            ],
        };
        self.copy(&package.script.path, &definition.script)?;
        self.copy(&package.messages, &definition.messages)?;
        self.world_audio(&definition.audio)?;
        self.json(&definition.movement, &package.movement)?;
        self.json(&definition.collision, &package.collision)?;
        self.json(&definition.encounters, &package.encounters)?;
        self.json(&definition.landmarks, &package.landmarks)?;
        self.json(&definition.guideposts, &package.guideposts)?;
        let mut continent_mappings = BTreeMap::new();
        for (index, (name, tiles)) in ["sylvarant", "tethe-alla"]
            .into_iter()
            .zip(package.worlds)
            .enumerate()
        {
            let owner = format!("world/{name}");
            let mut terrain = Vec::new();
            for tile in tiles {
                let root = format!(
                    "{owner}/terrain/column-{:02}-row-{:02}",
                    tile.column, tile.row
                );
                terrain.push(world::Tile {
                    column: tile.column,
                    row: tile.row,
                    base: self.world_terrain(&format!("{root}/base"), tile.base)?,
                    alternate: tile
                        .alternate
                        .map(|tile| self.world_terrain(&format!("{root}/alternate"), tile))
                        .transpose()?,
                });
            }
            let mut markers = BTreeMap::new();
            for (&id, parts) in &package.visuals.markers[index] {
                let name = format!("marker-{id:03}");
                self.world_model(&format!("{owner}/markers/{name}"), parts.clone())?;
                markers.insert(u32::from(id), name);
            }
            let markers_path = format!("{owner}/markers/mappings.json");
            self.json(&markers_path, &Mappings::new(markers)?)?;
            self.json(
                &format!("{owner}/world.json"),
                &world::Continent {
                    version: world::VERSION,
                    terrain,
                    markers: markers_path,
                },
            )?;
            continent_mappings.insert(index as u32, name.into());
        }
        self.json(world::MAPPINGS, &Mappings::new(continent_mappings)?)?;
        let mut actor_mappings = BTreeMap::new();
        for (id, parts) in package.visuals.actors {
            let name = actor_name(id);
            let owner = format!("world/actors/{name}");
            // The native world catalogue selects the same default party bodies
            // as fields, but adds world travel/mount clips. Share that body by
            // declared character identity, not by a coincidentally equal hash.
            let body = party_owner(u32::from(id)).unwrap_or_else(|| owner.clone());
            self.world_model_with_body(&owner, &body, parts)?;
            actor_mappings.insert(u32::from(id), name);
        }
        self.json(world::ACTORS, &Mappings::new(actor_mappings)?)?;
        for (name, parts) in ["normal", "late"].into_iter().zip(package.visuals.skies) {
            self.world_model(&format!("world/skies/{name}"), parts)?;
        }
        let mut cinematic_mappings = BTreeMap::new();
        for (id, scene) in package.visuals.cinematics {
            let name = format!("cinematic-{id:04}");
            let owner = format!("world/cinematics/{name}");
            let mut actors = BTreeMap::new();
            for (id, parts) in scene.actors {
                actors.insert(
                    id,
                    self.world_model(&format!("{owner}/actors/actor-{id}"), parts)?,
                );
            }
            let camera = format!("{owner}/camera.json");
            let dialogue = format!("{owner}/dialogue.json");
            self.json(&camera, &scene.camera)?;
            self.json(&dialogue, &scene.dialogue)?;
            self.json(
                &format!("{owner}/cinematic.json"),
                &world::Presentation {
                    version: world::VERSION,
                    world: scene.world,
                    presentation: scene.presentation,
                    camera,
                    dialogue,
                    actors,
                },
            )?;
            cinematic_mappings.insert(u32::from(id), name);
        }
        self.json(world::CINEMATICS, &Mappings::new(cinematic_mappings)?)?;
        let mut definition = definition;
        for (path, source) in definition.dust.iter_mut().zip(&package.visuals.dust) {
            *path = self.image(source, path)?;
        }
        definition.validate()?;
        self.json(world::PATH, &definition)
    }

    fn world_model(&mut self, owner: &str, parts: Vec<ScenePart>) -> Result<String> {
        self.world_model_with_body(owner, owner, parts)
    }

    fn world_model_with_body(
        &mut self,
        owner: &str,
        body: &str,
        mut parts: Vec<ScenePart>,
    ) -> Result<String> {
        for (index, part) in parts.iter_mut().enumerate() {
            self.body(body, index, part)?;
            self.clips(&format!("{owner}/animations"), index, &mut part.clips)?;
        }
        let path = format!("{owner}/model.json");
        self.json(
            &path,
            &world::Model {
                version: world::VERSION,
                parts,
            },
        )?;
        Ok(path)
    }

    fn world_terrain(&mut self, owner: &str, tile: overworld::TileResources) -> Result<String> {
        let model = self.world_model(owner, tile.parts)?;
        let collision = format!("{owner}/collision.json");
        self.json(&collision, &tile.ground)?;
        let path = format!("{owner}/terrain.json");
        self.json(
            &path,
            &world::Terrain {
                version: world::VERSION,
                model,
                collision,
            },
        )?;
        Ok(path)
    }
}

/// These native roles are also used by world travel/effect presentation. Names
/// are maintained independently of edited text and the contents of the model.
fn actor_name(id: u16) -> String {
    let character = |id| {
        party_owner(id)
            .unwrap()
            .strip_prefix("meshes/characters/")
            .unwrap()
            .to_owned()
    };
    match id {
        1..=9 => character(u32::from(id)),
        99 => "field-point".into(),
        100 => "enemy-symbol-0".into(),
        101 => "enemy-symbol-1".into(),
        200 => "rheaird".into(),
        201 => "ship".into(),
        202 => "noishe".into(),
        203 => "rheaird-exhaust".into(),
        204..=212 => format!("rheaird-rider-{}", character(u32::from(id - 203))),
        213 => "ship-wake".into(),
        214 => "ship-travel-effect".into(),
        215 => "flight-barrier".into(),
        216 => "world-portal".into(),
        _ => format!("actor-{id:04}"),
    }
}

#[cfg(test)]
mod tests;
