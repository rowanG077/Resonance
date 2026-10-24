use super::*;
use serde_json::json;

/// Save compatibility needs world rules, collision and native code, without
/// requiring artwork/audio or making paths and display labels part of identity.
pub fn gameplay(snapshot: &mut Snapshot, cancelled: &impl Fn() -> bool) -> Result<Option<Vec<u8>>> {
    if !present(snapshot)? {
        return Ok(None);
    }
    ensure!(!cancelled(), "world identity preparation cancelled");
    let definition: Definition = snapshot.json(PATH, JSON_LIMIT)?;
    definition.validate()?;
    let movement: MovementParameters = snapshot.json(&definition.movement, JSON_LIMIT)?;
    let collision: CollisionTables = snapshot.json(&definition.collision, JSON_LIMIT)?;
    let mut encounters: EncounterTables = snapshot.json(&definition.encounters, JSON_LIMIT)?;
    let mut landmarks: Landmarks = snapshot.json(&definition.landmarks, JSON_LIMIT)?;
    let mut guideposts: Vec<Guidepost> = snapshot.json(&definition.guideposts, JSON_LIMIT)?;
    movement.validate()?;
    collision.validate()?;
    encounters.validate()?;
    landmarks.validate()?;
    for post in &guideposts {
        post.validate(&landmarks)?;
    }
    for label in &mut encounters.terrains {
        label.name.clear();
    }
    for world in &mut encounters.worlds {
        world.name.clear();
        world.area_names.fill(None);
    }
    for landmark in landmarks.worlds.iter_mut().flatten() {
        landmark.name.clear();
    }
    for post in &mut guideposts {
        post.name.clear();
    }
    let mut continents = BTreeMap::new();
    let ids = entries(snapshot, &definition.continents)?;
    ensure!(
        ids == [0, 1],
        "world mappings must contain continents 0 and 1"
    );
    for id in ids {
        let path = snapshot.resolve(&definition.continents, id, Some("world.json"))?;
        let continent: Continent = snapshot.json(&path, JSON_LIMIT)?;
        continent.validate(&path)?;
        let mut terrain = Vec::new();
        for tile in continent.terrain {
            ensure!(!cancelled(), "world identity preparation cancelled");
            let base = ground(snapshot, &tile.base)?;
            let alternate = tile
                .alternate
                .as_ref()
                .map(|path| ground(snapshot, path))
                .transpose()?;
            terrain.push((tile.column, tile.row, base, alternate));
        }
        continents.insert(id, terrain);
    }
    let mut cinematics = BTreeMap::new();
    for id in entries(snapshot, &definition.cinematics)? {
        ensure!(!cancelled(), "world identity preparation cancelled");
        let path = snapshot.resolve(&definition.cinematics, id, Some("cinematic.json"))?;
        let cinematic: Presentation = snapshot.json(&path, JSON_LIMIT)?;
        version(cinematic.version, &path)?;
        cinematics.insert(id, cinematic.world);
    }
    let script = snapshot.read(&definition.script, JSON_LIMIT)?;
    ensure!(!cancelled(), "world identity preparation cancelled");
    Ok(Some(serde_json::to_vec(&json!({
        "script": &*script, "movement": movement, "collision": collision,
        "encounters": encounters, "landmarks": landmarks, "guideposts": guideposts,
        "continents": continents, "cinematics": cinematics,
    }))?))
}

fn ground(snapshot: &mut Snapshot, path: &str) -> Result<Vec<crate::field::CollisionGroup>> {
    let tile: Terrain = snapshot.json(path, JSON_LIMIT)?;
    version(tile.version, path)?;
    let ground: Vec<crate::field::CollisionGroup> = snapshot.json(&tile.collision, JSON_LIMIT)?;
    ensure!(
        !ground.is_empty(),
        "empty world terrain collision in {path}"
    );
    for group in &ground {
        group.validate()?;
        ensure!(
            group.surface < 16,
            "invalid world terrain surface in {path}"
        );
    }
    Ok(ground)
}
