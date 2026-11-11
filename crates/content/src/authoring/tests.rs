use super::*;
use crate::library::{Descriptor, GAME, Library, VERSION};
use serde_json::json;
use std::{fs, path::Path};

fn write(root: &Path, path: &str, value: &Value) -> Result<()> {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, serde_json::to_vec(value)?)?;
    Ok(())
}
fn base(root: &Path) -> Result<()> {
    write(
        root,
        "content.json",
        &serde_json::to_value(Descriptor {
            version: VERSION,
            game: GAME.into(),
        })?,
    )
}

#[test]
fn empty_collections_open_with_captured_and_validated_markers() -> Result<()> {
    for (path, value, marker) in [
        ("textures/banks.json", json!({}), json!({})),
        ("fields/empty/messages.json", json!([]), Value::Null),
        ("ui/boot/sequence.json", json!([]), Value::Null),
    ] {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        base(root)?;
        publish(root, path, &value)?;
        let library = Library::open_paths(root, &[])?;
        assert_eq!(library.snapshot().json::<Value>(path, 4096)?, value);
        let physical = root
            .join(path.strip_suffix(".json").unwrap())
            .join("collection.json");
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(&physical)?)?,
            marker
        );

        // Markers are parsed at startup, and their captured bytes stay frozen.
        fs::write(&physical, b"invalid JSON")?;
        assert!(Library::open_paths(root, &[]).is_err());
        assert_eq!(library.snapshot().json::<Value>(path, 4096)?, value);
        fs::write(&physical, b"{\"unexpected\":true}")?;
        assert!(Library::open_paths(root, &[]).is_err());
    }
    Ok(())
}

#[test]
fn independent_item_rules_text_and_additions_resolve_once_at_startup() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("base");
    let balance = temp.path().join("balance");
    let translation = temp.path().join("translation");
    let addition = temp.path().join("addition");
    base(&root)?;
    publish(
        &root,
        crate::items::PATH,
        &json!({
            "mappings":{"0":"apple-gel"},
            "items":{"apple-gel":{"category":"gel","price":100,"text":{"name":"Apple Gel"}}},
            "equipment_descriptions":{"mappings":"Special description"}
        }),
    )?;
    write(
        &balance,
        "game/items/items/apple-gel/definition.json",
        &json!({"category":"gel","price":250}),
    )?;
    publish(
        &translation,
        crate::items::PATH,
        &json!({"items":{"apple-gel":{"text":{"name":"Appelgel"}}}}),
    )?;
    write(&addition, "game/items/mappings/1.json", &json!("mod-berry"))?;
    write(
        &addition,
        "game/items/items/mod-berry/definition.json",
        &json!({"category":"fruit","price":5}),
    )?;
    let mods = vec![balance, translation.clone(), addition];
    let library = Library::open_paths(&root, &mods)?;
    let read = |library: &Library| -> Result<crate::items::Prepared> {
        library
            .snapshot()
            .json::<crate::items::Document>(crate::items::PATH, 100_000)?
            .prepare()
    };
    let prepared = read(&library)?;
    assert_eq!(prepared.menu.len(), 2);
    assert_eq!(prepared.menu[0].price, 250);
    assert_eq!(prepared.text[0]["name"], "Appelgel");
    // No source reopening, even across fresh preparations of the same library.
    fs::remove_dir_all(&translation)?;
    fs::remove_dir_all(&root)?;
    assert_eq!(read(&library)?.text[0]["name"], "Appelgel");
    Ok(())
}

#[test]
fn learning_registrations_compose_with_explicit_order_and_removal() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("base");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let removal = temp.path().join("removal");
    base(&root)?;
    let source = json!({"mappings":{},"characters":{},"techniques":{},"learning":{"lloyd":["demon-fang","sword-rain"]}});
    publish(&root, crate::arte::PATH, &source)?;
    for (directory, name, order) in [(&first, "mod-flame", 5000), (&second, "mod-frost", 6000)] {
        write(
            directory,
            &format!("game/techniques/learning/lloyd/{name}/entry.json"),
            &json!(name),
        )?;
        write(
            directory,
            &format!("game/techniques/learning/lloyd/{name}/order.json"),
            &json!(order),
        )?;
    }
    write(
        &removal,
        "game/techniques/learning/lloyd/demon-fang/entry.json",
        &json!({"$remove":true}),
    )?;
    let library = Library::open_paths(&root, &[first, second, removal])?;
    let result: Value = library.snapshot().json(crate::arte::PATH, 100_000)?;
    assert_eq!(
        result["learning"]["lloyd"],
        json!(["sword-rain", "mod-flame", "mod-frost"])
    );
    // Positions and fields are not entries: they cannot be retired.
    let fixed = temp.path().join("fixed");
    write(
        &fixed,
        "game/techniques/characters.json",
        &json!({"$remove":true}),
    )?;
    assert!(Library::open_paths(&root, &[fixed]).is_err());
    Ok(())
}

#[test]
fn ownership_roundtrip_preserves_empty_values_and_escaped_names() -> Result<()> {
    for name in [
        "",
        "collection",
        "mappings",
        "A/B.glb",
        "a%2Fb",
        "日本語",
        "..",
        "\0",
    ] {
        assert_eq!(uncomponent(&component(name))?, name);
    }
    let source = json!({
        "mappings":{"0":"collection"},"items":{
            "collection":{"category":"gel","text":null},
            "A/B.glb":{"category":"gel","text":{}},
            "日本語":{"category":"gel","text":[]}, "invalid-optional":false
        },"equipment_descriptions":{}
    });
    let files = split(crate::items::PATH, &source)?.unwrap();
    validate_split(crate::items::PATH, &source, &files)
}

#[test]
fn partial_ui_mod_keeps_inherited_settings_without_a_synthetic_owner() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("base");
    let modification = temp.path().join("portraits");
    base(&root)?;
    let path = "ui/battle/battle.json";
    let original = json!({"font":"fonts/battle.json","stun":{"image":"ui/stun.ktx2","size":[16,16]},"portraits":{"lloyd":{"image":"ui/old.ktx2","size":[64,256]}}});
    publish(&root, path, &original)?;
    let contribution = json!({"portraits":{"lloyd":{"image":"ui/new.ktx2","size":[64,256]}}});
    let files = publish(&modification, path, &contribution)?;
    assert!(!files.contains("ui/battle/battle/definition.json"));
    let library = Library::open_paths(&root, &[modification.clone()])?;
    let resolved: Value = library.snapshot().json(path, 100_000)?;
    assert_eq!(resolved["font"], original["font"]);
    assert_eq!(resolved["stun"], original["stun"]);
    assert_eq!(resolved["portraits"], contribution["portraits"]);
    // The same source files also declare a standalone document without a root
    // definition marker, including when every field has an independent owner.
    base(&modification)?;
    assert_eq!(
        Library::open_paths(&modification, &[])?
            .snapshot()
            .json::<Value>(path, 100_000)?,
        contribution
    );
    Ok(())
}

#[test]
fn ambiguous_owners_and_mismatched_registration_ids_fail_at_startup() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    base(root)?;
    publish(
        root,
        crate::items::PATH,
        &json!({"mappings":{},"items":{},"equipment_descriptions":{}}),
    )?;
    write(root, crate::items::PATH, &json!({}))?;
    assert!(Library::open_paths(root, &[]).is_err());
    fs::remove_file(root.join(crate::items::PATH))?;
    write(
        root,
        "game/items/items/gel/definition.json",
        &json!({"category":"gel","text":"wrong owner"}),
    )?;
    assert!(Library::open_paths(root, &[]).is_err());
    fs::remove_dir_all(root.join("game/items/items/gel"))?;
    // A misspelled owner is reported instead of being silently ignored.
    write(root, "game/items/mapings/0.json", &json!("gel"))?;
    let error = format!("{:#}", Library::open_paths(root, &[]).unwrap_err());
    assert!(error.contains("is not a source of"), "{error}");
    fs::remove_dir_all(root.join("game/items/mapings"))?;
    publish(
        root,
        crate::arte::PATH,
        &json!({"learning":{"lloyd":["demon-fang"]}}),
    )?;
    write(
        root,
        "game/techniques/learning/lloyd/demon-fang/entry.json",
        &json!("other"),
    )?;
    assert!(Library::open_paths(root, &[]).is_err());
    Ok(())
}

#[test]
fn an_edit_published_over_a_base_retires_what_it_dropped() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let (root, layer) = (temp.path().join("base"), temp.path().join("mod"));
    base(&root)?;
    let titles = json!({
        "mappings": {"lloyd": {"0": "swordsman", "1": "drifter"}},
        "titles": {
            "swordsman": {"text": {"name": "Swordsman"}, "growth": {"strength": 1}},
            "drifter": {"text": {"name": "Drifter"}, "growth": {}}
        },
        "awards": {
            "levels": [
                {"character": "lloyd", "title": "swordsman", "level": 5},
                {"character": "lloyd", "title": "drifter", "level": 9}
            ],
            "combos": [],
            "equipment": []
        }
    });
    let ids = json!({"1": "iselia", "2": "triet"});
    publish(&root, crate::titles::PATH, &titles)?;
    publish(&root, crate::library::FIELDS, &ids)?;

    // A named entry with its own fields, a nested named collection, a
    // registration and a mapped ID each leave through their own marker.
    let mut edited = titles.clone();
    edited["titles"].as_object_mut().unwrap().remove("drifter");
    edited["mappings"].as_object_mut().unwrap().remove("lloyd");
    edited["awards"]["levels"].as_array_mut().unwrap().pop();
    let files = publish_over(&layer, crate::titles::PATH, &titles, &edited)?;
    for marker in [
        "game/titles/titles/drifter/definition.json",
        "game/titles/mappings/lloyd/collection.json",
        "game/titles/awards/levels/lloyd--drifter/entry.json",
    ] {
        assert!(files.contains(marker), "{marker} is missing from {files:?}");
    }
    publish_over(
        &layer,
        crate::library::FIELDS,
        &ids,
        &json!({"1": "iselia"}),
    )?;

    // Omitting a default-valued setting must clear its inherited definition,
    // even when the object still has independently owned fields.
    let skits = json!({"skits":[],"preview_order":[],"media":{"ids":[1],"speed":2.0}});
    let mut edited_skits = skits.clone();
    edited_skits["media"]
        .as_object_mut()
        .unwrap()
        .remove("speed");
    publish(&root, crate::skit::document::PATH, &skits)?;
    publish_over(&layer, crate::skit::document::PATH, &skits, &edited_skits)?;

    let mut snapshot = Library::open_paths(&root, &[layer])?.snapshot();
    assert_eq!(
        snapshot.json::<Value>(crate::titles::PATH, 1 << 20)?,
        edited
    );
    assert_eq!(
        snapshot.json::<Value>(crate::library::FIELDS, 1 << 20)?,
        json!({"1": "iselia"})
    );
    assert_eq!(
        snapshot.json::<Value>(crate::skit::document::PATH, 1 << 20)?,
        edited_skits
    );
    let skits: crate::skit::document::Definition =
        snapshot.json(crate::skit::document::PATH, 1 << 20)?;
    assert_eq!(skits.media.speed, 1.0);
    // A field of a document cannot be retired.
    assert!(
        publish_over(
            &temp.path().join("other"),
            crate::titles::PATH,
            &titles,
            &json!({})
        )
        .is_err()
    );
    Ok(())
}
