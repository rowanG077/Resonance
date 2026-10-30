use super::*;
use serde_json::{Value, json};

fn document() -> Value {
    let mut arenas = serde_json::Map::new();
    for terrain in Terrain::ALL {
        arenas.insert(
            serde_json::to_value(terrain)
                .unwrap()
                .as_str()
                .unwrap()
                .into(),
            json!(0),
        );
    }
    let continent = json!({"arenas":arenas,"grid":vec![[0;24];18],"regions":{
        "home":{"id":0,"groups":{"grassland":[11,12]}},
        "coast":{"id":1,"groups":"home"}
    }});
    json!({"default_group":500,"surface_terrain":{"grassland":"grassland"},
        "docks":[{"land":{"position":[100,100],"height":20},"sea":{"position":[200,100]}}],
        "later_story":100,"sylvarant":continent,"tethe_alla":continent})
}

#[test]
fn named_regions_share_directly_and_later_changes_do_not_retarget_early_aliases() -> Result<()> {
    let mut doc = document();
    doc["tethe_alla"]["regions"]["home"]["later"] = json!({"grassland":[21,22]});
    let tables: EncounterTables = serde_json::from_value(doc.clone())?;
    assert_eq!(
        tables.worlds[1].areas[1].as_ref().unwrap().encounter_groups[0],
        [11, 12]
    );
    assert_eq!(
        tables.worlds[2].areas[0].as_ref().unwrap().encounter_groups[0],
        [21, 22]
    );
    assert_eq!(
        tables.worlds[2].areas[1].as_ref().unwrap().encounter_groups[0],
        [11, 12]
    );
    assert_eq!(
        tables.worlds[2].areas[1].as_ref().unwrap().encounter_groups[1],
        [500; 2]
    );
    doc["tethe_alla"]["regions"]["coast"]["later"] = json!("home");
    doc["tethe_alla"]["surface_terrain"] = json!({"grassland":null,"road":"beach"});
    let edited: EncounterTables = serde_json::from_value(doc)?;
    assert_eq!(
        edited.worlds[2].areas[1].as_ref().unwrap().encounter_groups[0],
        [21, 22]
    );
    assert_eq!(
        edited.surface_terrain[0][TerrainSurface::Grassland.index()],
        Some(Terrain::Grassland)
    );
    assert_eq!(
        edited.surface_terrain[1][TerrainSurface::Grassland.index()],
        None
    );
    assert_eq!(
        edited.surface_terrain[1][TerrainSurface::Road.index()],
        Some(Terrain::Beach)
    );
    let reread: EncounterTables = serde_json::from_slice(&serde_json::to_vec(&edited)?)?;
    assert_eq!(
        reread.worlds[2].areas[1].as_ref().unwrap().encounter_groups[0],
        [21, 22]
    );
    Ok(())
}

#[test]
fn invalid_region_references_ids_terrain_and_unused_native_fields_are_rejected() {
    for (pointer, value) in [
        ("/tethe_alla/regions/home/groups", json!("coast")),
        ("/tethe_alla/regions/coast/groups", json!("missing")),
        ("/tethe_alla/regions/coast/id", json!(0)),
        ("/sylvarant/grid/0/0", json!(3)),
        ("/sylvarant/regions/home/later", json!({})),
        ("/surface_terrain/16", json!("grassland")),
        ("/sylvarant/arenas/volcano", json!(4)),
        ("/tile_loading_order", json!([])),
    ] {
        let mut doc = document();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        let parent = doc.pointer_mut(parent).unwrap();
        if let Some(index) = parent
            .as_array_mut()
            .map(|a| &mut a[key.parse::<usize>().unwrap()])
        {
            *index = value;
        } else {
            parent[key] = value;
        }
        assert!(
            serde_json::from_value::<EncounterTables>(doc).is_err(),
            "{pointer}"
        );
    }
}
