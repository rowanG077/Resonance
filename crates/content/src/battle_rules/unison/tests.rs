use super::*;
use serde_json::json;

fn fixture() -> Result<(Unison, Mappings, Mappings, Mappings)> {
    let characters = Mappings::new((1..=9).map(|id| (id, format!("character-{id}"))).collect())?;
    let mut value = json!({
        "host": {"placement":[[0,0,-1],[-1,0,0],[1,0,0],[0,0,1]],
            "minimum_distance":25,"short_weapon_penalty":40,"restore_height":0.1,
            "windup_rate":0.5,"windup_color":[16,16,16],
            "prelude":{"first_x":-75,"spacing":150,"camera_eye":[0,200,1500],
                "camera_target":[0,75,0],"color":[64,64,64]}},
        "characters":{},
        "openers":{"strike":{"duration":30,"recovery":10,"events":[]},
            "late-strike":{"duration":30,"recovery":10,"contact_ticks":45,"events":[]}},
        "mappings":{"10":"thrust","216":"lightning"},
        "placements":[{"techniques":["thrust",{"technique":"other-thrust","matches":"thrust"}],
            "distance":350,"duration":65},
            {"techniques":["lightning"],"altitude":200,"distance":400,"duration":0}],
        "combinations":[{"name":"Duet","scene":"duet","recipes":[["thrust","lightning"]],"duration":100},
            {"name":"Trio","scene":"duet","recipes":[["lightning","thrust","thrust"]],"duration":120}]
    });
    for (_, name) in characters.iter() {
        value["characters"][name] = json!({"opener":"strike"});
    }
    value["characters"]["character-3"] = json!({"opener":"late-strike","voice":{"stream":7}});
    Ok((
        serde_json::from_value(value)?,
        Mappings::new(
            [
                (1, "thrust".into()),
                (9, "lightning".into()),
                (210, "other-thrust".into()),
            ]
            .into(),
        )?,
        characters,
        Mappings::new([(301, "duet".into())].into())?,
    ))
}

#[test]
fn named_groups_preserve_shared_matching_and_order_without_native_table_sizes() -> Result<()> {
    let (document, techniques, characters, scenes) = fixture()?;
    let value = document.prepare(&techniques, &characters, &scenes)?;
    assert_eq!(value.placements.len(), 3);
    assert!(!value.placements.contains_key(&0));
    assert_eq!(value.placements[&1], value.placements[&210]);
    assert_eq!(value.placements[&9].duration, 0);
    assert_eq!(value.placements[&9].altitude, 200);
    assert_eq!(value.placements[&9].native_id, 216);
    assert_eq!(value.combinations[0].recipes, [vec![10, 216]]);
    assert_eq!(value.combinations[1].recipes, [vec![216, 10, 10]]);
    assert_eq!(value.combinations[1].participants, 3);
    assert_eq!(value.combinations[1].id, 2);
    assert_eq!(value.openers[0].contact_ticks, 30);
    assert_eq!(value.openers[2].contact_ticks, 45);
    assert_eq!(value.voices[2], Some(Sound::Stream(7)));
    assert!(value.voices[7].is_none());
    Ok(())
}

#[test]
fn shared_edits_and_renamed_bindings_keep_prepared_values_independent() -> Result<()> {
    let (mut document, techniques, characters, scenes) = fixture()?;
    let previous = document.prepare(&techniques, &characters, &scenes)?;
    document.openers.get_mut("strike").unwrap().duration = 32;
    document.placements[0].distance = 500;
    let current = document.prepare(&techniques, &characters, &scenes)?;
    assert_eq!(current.openers[0].contact_ticks, 32);
    assert_eq!(current.openers[2].contact_ticks, 45);
    assert_eq!(previous.openers[0].contact_ticks, 30);
    assert_eq!(current.placements[&210].distance, 500);
    assert_eq!(previous.placements[&210].distance, 350);
    let member = document.characters.remove("character-1").unwrap();
    document.characters.insert("renamed".into(), member);
    let characters = Mappings::new(
        characters
            .iter()
            .map(|(id, name)| (id, if id == 1 { "renamed" } else { name }.into()))
            .collect(),
    )?;
    assert_eq!(
        document.prepare(&techniques, &characters, &scenes)?.openers[0].duration,
        32
    );
    Ok(())
}

#[test]
fn invalid_named_dependencies_and_recipe_shapes_fail_preparation() -> Result<()> {
    let (document, techniques, characters, scenes) = fixture()?;
    for change in [
        |d: &mut Unison| d.characters.get_mut("character-1").unwrap().opener = "missing".into(),
        |d: &mut Unison| {
            d.placements[0]
                .techniques
                .push(Technique::Named("thrust".into()))
        },
        |d: &mut Unison| d.placements[0].techniques[0] = Technique::Named("missing".into()),
        |d: &mut Unison| d.combinations[0].recipes[0][0] = "missing".into(),
        |d: &mut Unison| d.combinations[0].participants = Some(3),
        |d: &mut Unison| d.combinations[0].scene = "missing".into(),
    ] {
        let mut edited = document.clone();
        change(&mut edited);
        assert!(edited.prepare(&techniques, &characters, &scenes).is_err());
    }
    Ok(())
}
