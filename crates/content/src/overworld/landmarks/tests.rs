use super::*;
use serde_json::{Value, json};

fn document() -> Value {
    json!({"sylvarant":[
        {"id":2,"name":"Second","position":[100.,200.],"height":0.,"radius":40.,"marker":"field_point",
         "reward_item":2,"required_character":"genis","guidepost":{"region":"Test region","discovery_flag":901,"unlock_flags":[902]}},
        {"id":1,"name":"First","position":[100.,200.],"radius":50.,"marker":{"model":11}}
    ],"tethe_alla":[{"id":257,"name":"Other world","position":[300.,400.],"radius":60.}]})
}

#[test]
fn locations_own_their_rewards_characters_and_guideposts_without_padding() -> Result<()> {
    let source = document();
    let data: Landmarks = serde_json::from_value(source.clone())?;
    data.validate()?;
    assert_eq!(
        data.worlds[0][0].required_character,
        Some(PartyCharacter::Genis)
    );
    assert_eq!(data.worlds[0][0].height, Some(0.));
    assert_eq!(data.worlds[0][1].height, None);
    assert_eq!(
        data.worlds[0][0]
            .guidepost
            .as_ref()
            .unwrap()
            .flags()
            .collect::<Vec<_>>(),
        [901, 902]
    );
    assert_eq!(data.worlds[0][1].marker, Marker::Model(11));
    assert_eq!(data.worlds[1][0].interaction, Interaction::Active);
    assert_eq!(data.worlds[1][0].marker, Marker::None);
    assert_eq!(
        serde_json::from_slice::<Value>(&serde_json::to_vec(&data)?)?,
        source
    );
    for (pointer, value) in [
        ("/sylvarant/0/required_character", json!(3)),
        ("/sylvarant/0/required_character", json!("unknown")),
        ("/sylvarant/0/guidepost/discovery_flag", json!(0)),
        ("/sylvarant/0/guidepost/name_id", json!(1)),
        ("/sylvarant/1/marker", json!({"kind":"model","id":11})),
        ("/worlds", json!([])),
        ("/item_rewards", json!({})),
    ] {
        let mut invalid = source.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        invalid.pointer_mut(parent).unwrap()[key] = value;
        assert!(
            serde_json::from_value::<Landmarks>(invalid).is_err(),
            "{pointer}"
        );
    }
    let mut duplicate = data.clone();
    duplicate.worlds[0][0].id = 1;
    assert!(duplicate.validate().is_err());
    let mut wrong_world = data;
    wrong_world.worlds[1][0].id = 2;
    assert!(wrong_world.validate().is_err());
    Ok(())
}

#[test]
fn location_mod_edits_remain_local_and_keep_retained_snapshots() -> Result<()> {
    use crate::library::{Descriptor, GAME, Library, VERSION};
    let root = tempfile::tempdir()?;
    let base = root.path().join("base");
    let modified = root.path().join("mod");
    for path in [&base, &modified] {
        std::fs::create_dir_all(path.join("world"))?;
    }
    std::fs::write(
        base.join("content.json"),
        serde_json::to_vec(&Descriptor {
            version: VERSION,
            game: GAME.into(),
        })?,
    )?;
    let original = document();
    let path = "world/landmarks.json";
    std::fs::write(base.join(path), serde_json::to_vec(&original)?)?;
    let library = Library::open_paths(&base, &[modified.clone()])?;
    let mut retained = library.snapshot();
    let before: Landmarks = retained.json(path, 1024 * 1024)?;
    let mut edit = original;
    edit["sylvarant"][0]["reward_item"] = 42.into();
    edit["sylvarant"][0]["required_character"] = "colette".into();
    edit["sylvarant"][0]["guidepost"]["unlock_flags"] = json!([903, 904]);
    edit["sylvarant"][0]["flight_floor"] = json!({"margin":50.,"height":1000.});
    std::fs::write(modified.join(path), serde_json::to_vec(&edit)?)?;
    let after: Landmarks = library.snapshot().json(path, 1024 * 1024)?;
    after.validate()?;
    assert_eq!(after.worlds[0][0].reward_item, Some(42));
    assert_eq!(
        after.worlds[0][0].flight_floor,
        Some(FlightFloor {
            margin: 50.,
            height: 1000.,
            active_only: true
        })
    );
    assert!(before.worlds[0][0].flight_floor.is_none());
    assert_eq!(
        after.worlds[0][0].required_character,
        Some(PartyCharacter::Colette)
    );
    assert_eq!(
        after.worlds[0][0]
            .guidepost
            .as_ref()
            .unwrap()
            .flags()
            .collect::<Vec<_>>(),
        [901, 903, 904]
    );
    assert_eq!(after.worlds[0][1], before.worlds[0][1]);
    assert_eq!(after.worlds[1], before.worlds[1]);
    assert_eq!(retained.json::<Landmarks>(path, 1024 * 1024)?, before);
    Ok(())
}
