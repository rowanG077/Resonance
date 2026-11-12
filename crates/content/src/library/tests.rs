use super::*;
mod music;
mod skits;
mod textures;
mod victories;

#[test]
fn scenes_share_startup_inputs_but_cannot_retain_other_scenes() -> Result<()> {
    use crate::{diagnostics::Diagnostics, prepared::Files};
    let fixture = Fixture::new();
    fixture.write("base/common.bin", b"common");
    fixture.write("base/field-a.bin", b"field a");
    let library = fixture.open(&[])?;
    let mut snapshot = library.snapshot();
    let common_bytes = snapshot.read("common.bin", 64)?;
    let common = Files::from_snapshot(snapshot, || false, Diagnostics::default())?;

    let mut snapshot = library.snapshot_with(&common)?;
    assert!(Arc::ptr_eq(
        &common_bytes,
        &snapshot.read("common.bin", 64)?
    ));
    let field_bytes = snapshot.read("field-a.bin", 64)?;
    let field_weak = Arc::downgrade(&field_bytes);
    drop(field_bytes);
    let first = Files::from_snapshot(snapshot, || false, Diagnostics::default())?;
    assert!(
        library.snapshot_with(&first).is_err(),
        "scene inheritance would defeat eviction"
    );
    let second = Files::from_snapshot(
        library.snapshot_with(&common)?,
        || false,
        Diagnostics::default(),
    )?;
    assert!(!second.contains_key("field-a.bin"));
    drop(first);
    assert!(
        field_weak.upgrade().is_none(),
        "another scene pinned evicted bytes"
    );
    assert!(Arc::ptr_eq(&common_bytes, &second.read("common.bin")?));

    // Inherited bytes keep their limits and namespace; a fresh snapshot sees edits.
    fixture.write("base/common.bin", b"edited");
    let mut scene = library.snapshot_with(&common)?;
    assert_eq!(&*scene.read("common.bin", 64)?, b"common");
    assert!(scene.read("common.bin", 2).is_err());
    assert_eq!(&*library.snapshot().read("common.bin", 64)?, b"edited");
    fixture.write("mod/common.bin", b"mod");
    assert!(fixture.open(&["mod"])?.snapshot_with(&common).is_err());
    Ok(())
}

#[test]
fn inherited_files_keep_ordered_unique_keys_and_local_editing() -> Result<()> {
    use crate::{diagnostics::Diagnostics, prepared::Files};
    let fixture = Fixture::new();
    fixture.write("base/a.bin", b"a");
    fixture.write("base/z.bin", b"z");
    fixture.write("base/m.bin", b"m");
    let library = fixture.open(&[])?;
    let mut snapshot = library.snapshot();
    snapshot.read("a.bin", 64)?;
    snapshot.read("z.bin", 64)?;
    let common = Files::from_snapshot(snapshot, || false, Diagnostics::default())?;
    let mut snapshot = library.snapshot_with(&common)?;
    snapshot.read("m.bin", 64)?;
    let original = Files::from_snapshot(snapshot, || false, Diagnostics::default())?;
    let mut edited = original.clone();
    edited.insert("a.bin".into(), Arc::from(b"edited".as_slice()));
    assert_eq!(
        edited.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a.bin", "m.bin", "z.bin"]
    );
    assert_eq!(edited.len(), 3);
    edited.remove("z.bin");
    assert!(!edited.contains_key("z.bin"));
    assert!(original.contains_key("z.bin") && common.contains_key("z.bin"));
    assert_eq!(&*edited.read("a.bin")?, b"edited");
    assert_eq!(&*original.read("a.bin")?, b"a");
    edited.clear();
    assert!(edited.is_empty() && edited.keys().next().is_none());
    assert!(!original.is_empty());
    Ok(())
}

#[test]
fn enemy_document_edits_keep_gameplay_required_and_artwork_optional() -> Result<()> {
    use crate::{battle_enemy::document as enemies, diagnostics::Diagnostics};
    use serde_json::json;
    let fixture = Fixture::new();
    fixture.write("base/battle/enemies/mappings.json", br#"{"0":"test"}"#);
    let mesh = resonance_asset_writer::gltf::pack_glb(
        &json!({
            "asset":{"version":"2.0"},"nodes":[{"name":"root","extras":{"resonance_bone":0}}]
        }),
        &[],
    )?;
    fixture.write("base/body.glb", &mesh);
    fixture.write(
        "base/preview.json",
        &serde_json::to_vec(&json!({
            "scale":1,"elevation":0,"hidden_geometry":[],"behavior":null,
            "parts":[{"resource":0,"mesh":"body.glb","textures":[],"materials":[]}]
        }))?,
    );
    let entry = enemies::Entry {
        rules: crate::battle_enemy::rules::tests::fixture()?,
        artwork: json!({"model":"preview.json","attachment_nodes":{"0":"root"}}),
        effects: None,
    };
    let original = serde_json::to_value(&entry)?;
    assert!(original.get("version").is_none() && original["rules"].get("version").is_none());
    assert!(original.get("effects").is_none());
    let path = "battle/enemies/test/enemy.json";
    fixture.write(&format!("base/{path}"), &serde_json::to_vec(&entry)?);
    let mut retained = fixture.open(&[])?.snapshot();
    let read = |snapshot: &mut Snapshot, diagnostics: &Diagnostics| {
        enemies::capture(snapshot, 0, diagnostics, &|| false)
    };
    let strict = Diagnostics::new(true);
    let base = read(&mut retained, &strict)?;
    assert_eq!(base.definition.guard_recovery_bonus, 0);
    assert_eq!(base.artwork.unwrap().source.body.attachments[&0], 0);

    let mut edited = original.clone();
    edited["rules"]["guard_recovery_bonus"] = 7.into();
    edited["artwork"]["attachment_nodes"]["0"] = "missing".into();
    let write = |value: &serde_json::Value| {
        fixture.write(&format!("mod/{path}"), &serde_json::to_vec(value).unwrap())
    };
    write(&edited);
    let modified = fixture.open(&["mod"])?;
    let tolerant = Diagnostics::default();
    let current = read(&mut modified.snapshot(), &tolerant)?;
    assert_eq!(current.definition.guard_recovery_bonus, 7);
    assert!(current.artwork.is_none() && tolerant.has_errors());
    assert!(read(&mut modified.snapshot(), &strict).is_err());
    let old = read(&mut retained, &strict)?;
    assert_eq!(old.definition.guard_recovery_bonus, 0);
    assert!(old.artwork.is_some());

    // Malformed optional settings follow the same policy as a missing visual
    // dependency. Rules and the containing document must still be valid.
    edited["artwork"] = json!({"model":42});
    write(&edited);
    let tolerant = Diagnostics::default();
    assert!(read(&mut modified.snapshot(), &tolerant)?.artwork.is_none());
    assert!(tolerant.has_errors() && read(&mut modified.snapshot(), &strict).is_err());
    edited.as_object_mut().unwrap().remove("artwork");
    write(&edited);
    let quiet = Diagnostics::default();
    assert!(read(&mut modified.snapshot(), &quiet)?.artwork.is_none());
    assert!(!quiet.has_errors());
    edited["rules"] = json!({});
    write(&edited);
    assert!(read(&mut modified.snapshot(), &quiet).is_err());
    edited = original;
    edited["rules"]["attacks"]["spit"]["events"][0][1] = json!({"contact":"missing"});
    write(&edited);
    assert!(
        format!("{:#}", read(&mut modified.snapshot(), &quiet).unwrap_err())
            .contains("unknown enemy contact missing")
    );
    fixture.write(&format!("mod/{path}"), b"invalid JSON");
    assert!(read(&mut modified.snapshot(), &quiet).is_err());
    Ok(())
}

struct Fixture(tempfile::TempDir);

impl Fixture {
    fn new() -> Self {
        let fixture = Self(tempfile::tempdir().unwrap());
        fixture.write(
            "base/content.json",
            &serde_json::to_vec(&Descriptor {
                version: VERSION,
                game: GAME.into(),
            })
            .unwrap(),
        );
        fixture
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// Publish a document as granular owners, so that a mod can override one.
    fn publish(&self, layer: &str, path: &str, document: serde_json::Value) {
        crate::authoring::publish(&self.0.path().join(layer), path, &document).unwrap();
    }

    fn open(&self, names: &[&str]) -> Result<Library> {
        Library::open(
            &self.0.path().join("base"),
            &names
                .iter()
                .map(|name| Mod {
                    name: (*name).into(),
                    root: self.0.path().join(name),
                })
                .collect::<Vec<_>>(),
        )
    }
}

#[test]
fn game_identity_is_semantic_and_source_metadata_is_rejected() -> Result<()> {
    let fixture = Fixture::new();
    let library = fixture.open(&[])?;
    assert_eq!(library.descriptor.game, GAME);
    for invalid in [
        serde_json::json!({"version": VERSION - 1, "game": GAME}),
        serde_json::json!({"version": VERSION, "game": "GQSEAF"}),
        serde_json::json!({"version": VERSION, "game": GAME, "revision": 0}),
        serde_json::json!({"version": VERSION, "game": GAME, "platform": "steam"}),
    ] {
        fixture.write("base/content.json", &serde_json::to_vec(&invalid)?);
        assert!(fixture.open(&[]).is_err(), "accepted {invalid}");
    }
    Ok(())
}

#[test]
fn directory_selection_preserves_order_and_allows_equal_folder_names() -> Result<()> {
    let fixture = Fixture::new();
    for name in ["first", "second", "translated", "corrected"] {
        fixture.write(&format!("base/fields/{name}/field.json"), b"{}");
    }
    fixture.publish(
        "base",
        FIELDS,
        serde_json::json!({"1":"first","2":"second"}),
    );
    fixture.write("one/translation/fields/mappings/1.json", br#""translated""#);
    fixture.write("two/translation/fields/mappings/1.json", br#""corrected""#);
    let mods = [
        fixture.0.path().join("one/translation"),
        fixture.0.path().join("two/translation"),
    ];
    let library = Library::open_paths(&fixture.0.path().join("base"), &mods)?;
    let mut snapshot = library.snapshot();
    assert_eq!(snapshot.field(1)?, "fields/corrected/field.json");
    assert_eq!(snapshot.field(2)?, "fields/second/field.json");
    assert_eq!(
        Library::open_paths(
            &fixture.0.path().join("base"),
            &[mods[1].clone(), mods[0].clone()]
        )?
        .snapshot()
        .field(1)?,
        "fields/translated/field.json"
    );
    assert!(
        Library::open_paths(
            &fixture.0.path().join("base"),
            &[mods[0].clone(), mods[0].clone()]
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn battle_pool_selection_and_worker_keep_one_effective_formation_table() -> Result<()> {
    use crate::{battle_formation as f, battle_rules};
    let fixture = Fixture::new();
    let source = f::Formations {
        records: [17, 23]
            .map(|enemy| f::Formation {
                settings: f::Settings {
                    play_music: true,
                    celebrate: true,
                    entry_voice: true,
                    escape_restricted: false,
                },
                resources: vec![f::Resource {
                    enemy,
                    hidden_name: false,
                }],
                actors: vec![f::Actor::default()],
            })
            .into(),
        pools: vec![vec![f::WeightedFormation {
            formation: 0,
            weight: 1,
        }]],
    };
    let enemies = crate::library::Mappings::new(
        [
            (17, "wolf".into()),
            (23, "boar".into()),
            (42, "bear".into()),
        ]
        .into(),
    )?;
    let names = crate::library::Mappings::new([(0, "wolves".into()), (1, "boars".into())].into())?;
    let mut table = battle_rules::Formations::from_table(&source, &enemies, &names)?;
    fixture.write(
        "base/battle/enemies/mappings.json",
        &serde_json::to_vec(&enemies)?,
    );
    fixture.write("base/battle/formations.json", &serde_json::to_vec(&table)?);
    table.pools[0].choices[0].formation = 1;
    fixture.write("mod/battle/formations.json", &serde_json::to_vec(&table)?);
    let library = fixture.open(&["mod"])?;
    let mut snapshot = library.snapshot();
    let selected = battle_rules::capture_formations(&mut snapshot)?.select(0, 0)?;
    assert_eq!(selected, 1);
    table.formations.get_mut("boars").unwrap().actors[0].enemy = "bear".into();
    table.pools[0].choices[0].formation = 0;
    fixture.write("mod/battle/formations.json", &serde_json::to_vec(&table)?);
    let captured = battle_rules::capture_formations(&mut snapshot)?;
    assert_eq!(captured.select(0, 0)?, selected);
    assert_eq!(
        captured.records[usize::from(selected)].resources[0].enemy,
        23
    );
    let current = battle_rules::capture_formations(&mut library.snapshot())?;
    assert_eq!(current.select(0, 0)?, 0);
    assert_eq!(current.records[1].resources[0].enemy, 42);
    Ok(())
}

#[test]
fn optional_roots_and_layer_selection_remain_pinned_until_restart() -> Result<()> {
    let fixture = Fixture::new();
    let library = fixture.open(&[])?;
    let mut absent = library.snapshot();
    assert!(!crate::overworld::document::present(&mut absent)?);
    fixture.write("base/world/world.json", b"base root");
    assert!(!crate::overworld::document::present(&mut absent)?);
    assert!(absent.read("world/world.json", 1024).is_err());
    assert!(!crate::overworld::document::present(
        &mut library.snapshot()
    )?);
    assert!(crate::overworld::document::present(
        &mut fixture.open(&[])?.snapshot()
    )?);
    fixture.write("mod/world/world.json", b"mod root");
    let modified = fixture.open(&["mod"])?;
    let mut snapshot = modified.snapshot();
    assert_eq!(
        &*snapshot.read_optional("world/world.json", 1024)?.unwrap(),
        b"mod root"
    );
    fs::remove_file(fixture.0.path().join("mod/world/world.json"))?;
    assert_eq!(&*snapshot.read("world/world.json", 1024)?, b"mod root");
    // A removed selected override fails until restart; it never falls through.
    assert!(
        modified
            .snapshot()
            .read_optional("world/world.json", 1024)
            .is_err()
    );
    assert_eq!(
        &*fixture
            .open(&["mod"])?
            .snapshot()
            .read("world/world.json", 1024)?,
        b"base root"
    );
    assert!(snapshot.read_optional("world/world.json", 1).is_err());
    assert_eq!(snapshot.overrides().len(), 1);
    assert!(
        snapshot.read_optional("world", 1024).is_err(),
        "a directory is not an absent document"
    );
    Ok(())
}

#[test]
fn namespace_discovery_includes_mod_modules_and_pins_each_preparation() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write("base/scripts/field/memory.sym", b"original");
    fixture.write("mod/scripts/field/memory.sym", b"edited");
    fixture.write("mod/scripts/field/custom.sym", b"new module");
    let library = fixture.open(&["mod"])?;
    let mut snapshot = library.snapshot();
    let paths = snapshot.files_under("scripts")?.clone();
    assert_eq!(
        paths,
        [
            "scripts/field/custom.sym".into(),
            "scripts/field/memory.sym".into()
        ]
        .into()
    );
    let sources = crate::preparation::scripts(&mut snapshot, &|| false)?;
    assert_eq!(sources["field::memory"], "edited");
    assert_eq!(sources["field::custom"], "new module");
    fixture.write("mod/scripts/later.sym", b"later");
    assert_eq!(*snapshot.files_under("scripts")?, paths);
    assert_eq!(library.snapshot().files_under("scripts")?.len(), 2);
    assert_eq!(
        fixture
            .open(&["mod"])?
            .snapshot()
            .files_under("scripts")?
            .len(),
        3
    );
    assert!(crate::preparation::scripts(&mut library.snapshot(), &|| true).is_err());
    Ok(())
}

#[test]
fn namespace_discovery_rejects_case_ambiguous_names_and_non_directories() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write("base/scripts/field/start.sym", b"base");
    fixture.write("mod/scripts/Field/start.sym", b"mod");
    let error = format!("{:#}", fixture.open(&["mod"]).unwrap_err());
    assert!(error.contains("case-conflicting content paths"), "{error}");
    fixture.write("invalid/scripts", b"not a directory");
    assert!(fixture.open(&["invalid"]).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn links_are_not_content() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write("base/scripts/start.sym", b"base");
    std::os::unix::fs::symlink(".", fixture.0.path().join("base/scripts/loop"))?;
    let error = format!("{:#}", fixture.open(&[]).unwrap_err());
    assert!(
        error.contains("symbolic links are not supported"),
        "{error}"
    );
    // Hidden entries, such as a mod's version control, are not scanned at all.
    fs::rename(
        fixture.0.path().join("base/scripts/loop"),
        fixture.0.path().join("base/scripts/.loop"),
    )?;
    fixture.write("base/.git/odd#name", b"state");
    let library = fixture.open(&[])?;
    assert_eq!(library.snapshot().files_under("scripts")?.len(), 1);
    Ok(())
}

#[test]
fn captured_resources_reach_runtime_and_new_reads_use_current_bytes() -> Result<()> {
    use crate::{diagnostics::Diagnostics, prepared::Files};
    let fixture = Fixture::new();
    fixture.write("base/fields/iselia/messages.json", br#"["original"]"#);
    fixture.write("base/fields/iselia/wall.png", b"unchanged image");
    let library = fixture.open(&[])?;
    let mut snapshot = library.snapshot();
    snapshot.read("fields/iselia/messages.json", 4096)?;
    snapshot.read("fields/iselia/wall.png", 4096)?;
    fixture.write("base/fields/iselia/messages.json", br#"["edited"]"#);

    let first = Files::from_snapshot(snapshot, || false, Diagnostics::new(true))?;
    assert_eq!(
        &*first.read("fields/iselia/messages.json")?,
        br#"["original"]"#
    );
    let mut snapshot = library.snapshot();
    snapshot.read("fields/iselia/messages.json", 4096)?;
    snapshot.read("fields/iselia/wall.png", 4096)?;
    let second = Files::from_snapshot(snapshot, || false, Diagnostics::new(true))?;
    assert_eq!(
        &*second.read("fields/iselia/messages.json")?,
        br#"["edited"]"#
    );
    assert!(Files::from_snapshot(library.snapshot(), || true, Diagnostics::new(true)).is_err());
    Ok(())
}

#[test]
fn mappings_reject_duplicate_or_ambiguous_ids_and_paths() {
    for invalid in [
        r#"{"1":"iselia","1":"triet"}"#,
        r#"{"01":"iselia"}"#,
        r#"{"+1":"iselia"}"#,
        r#"{"-1":"iselia"}"#,
        r#"{"4294967296":"iselia"}"#,
        r#"{"1":null}"#,
        r#"{"1":"../iselia"}"#,
        r#"{"1":"/iselia"}"#,
        r#"{"1":"iselia//room"}"#,
        r#"{"1":"iselia/./room"}"#,
        r#"{"1":"iselia/"}"#,
        r#"{"1":"iselia\nroom"}"#,
    ] {
        assert!(
            serde_json::from_str::<Mappings>(invalid).is_err(),
            "{invalid}"
        );
    }
    let mappings: Mappings =
        serde_json::from_str(r#"{"12":"bell.wav","0":"silence.wav"}"#).unwrap();
    assert_eq!(
        mappings
            .resolve("audio/effects/church/mappings.json", 12, None)
            .unwrap(),
        "audio/effects/church/bell.wav"
    );
    assert!(
        mappings
            .resolve("audio/effects/church/catalogue.json", 12, None)
            .is_err()
    );
    assert_eq!(
        serde_json::to_string(&mappings).unwrap(),
        r#"{"0":"silence.wav","12":"bell.wav"}"#
    );
}

#[test]
fn mod_mapping_owners_rebind_single_ids_and_references_inherit_base_files() -> Result<()> {
    let fixture = Fixture::new();
    fixture.publish(
        "base",
        FIELDS,
        serde_json::json!({"1":"iselia-1","2":"triet"}),
    );
    fixture.write(
        "base/fields/iselia-1/field.json",
        br#"{"messages":"fields/iselia-1/messages.json"}"#,
    );
    fixture.write("base/fields/iselia-1/messages.json", b"original dialogue");
    fixture.write("base/fields/triet/field.json", b"{}");
    fixture.write("base/fields/alternate/field.json", b"{}");
    fixture.write(
        "dialogue/fields/iselia-1/messages.json",
        b"replacement dialogue",
    );
    fixture.write("world/fields/mappings/2.json", br#""alternate""#);
    fixture.write("world/fields/mappings/3.json", br#""new-room""#);
    fixture.write("world/fields/new-room/field.json", b"{}");
    let mut snapshot = fixture.open(&["dialogue", "world"])?.snapshot();
    assert_eq!(snapshot.field(1)?, "fields/iselia-1/field.json");
    assert_eq!(snapshot.field(2)?, "fields/alternate/field.json");
    assert_eq!(snapshot.field(3)?, "fields/new-room/field.json");
    let field: serde_json::Value = snapshot.json("fields/iselia-1/field.json", 4096)?;
    let path = field["messages"].as_str().unwrap();
    assert_eq!(&*snapshot.read(path, 4096)?, b"replacement dialogue");
    let merged: Mappings = snapshot.json(FIELDS, 4096)?;
    assert_eq!(merged.iter().count(), 3);
    assert_eq!(
        snapshot.overrides(),
        &[
            Override {
                path: "fields/mappings/2.json".into(),
                previous: "base".into(),
                replacement: "world".into()
            },
            Override {
                path: path.into(),
                previous: "base".into(),
                replacement: "dialogue".into()
            },
        ]
    );
    let mut base = fixture.open(&[])?.snapshot();
    assert_eq!(&*base.read(path, 4096)?, b"original dialogue");
    assert_eq!(base.field(2)?, "fields/triet/field.json");
    assert!(base.field(3).is_err());
    Ok(())
}

#[test]
fn later_mod_wins_and_order_is_reported_for_each_conflict() -> Result<()> {
    let fixture = Fixture::new();
    fixture.publish("base", FIELDS, serde_json::json!({}));
    for (layer, target) in [("base", "one"), ("first", "two"), ("last", "three")] {
        fixture.write(
            &format!("{layer}/fields/mappings/1.json"),
            format!(r#""{target}""#).as_bytes(),
        );
        fixture.write(&format!("{layer}/data/rules.json"), target.as_bytes());
    }
    let library = fixture.open(&["first", "last"])?;
    let mut snapshot = library.snapshot();
    assert_eq!(snapshot.mappings(FIELDS)?.target(1)?, "three");
    assert_eq!(&*snapshot.read("data/rules.json", 4096)?, b"three");
    assert_eq!(snapshot.overrides().len(), 4);
    for conflict in [&snapshot.overrides()[1], &snapshot.overrides()[3]] {
        assert_eq!(
            (&*conflict.previous, &*conflict.replacement),
            ("first", "last")
        );
    }
    // Repeated reads do not duplicate override diagnostics.
    snapshot.read("data/rules.json", 4096)?;
    snapshot.mappings(FIELDS)?;
    assert_eq!(snapshot.overrides().len(), 4);
    let mut reversed = fixture.open(&["last", "first"])?.snapshot();
    assert_eq!(reversed.mappings(FIELDS)?.target(1)?, "two");
    assert_eq!(&*reversed.read("data/rules.json", 4096)?, b"two");
    Ok(())
}

#[test]
fn snapshot_keeps_captured_bytes_and_a_new_snapshot_sees_edits() -> Result<()> {
    let fixture = Fixture::new();
    fixture.publish("base", FIELDS, serde_json::json!({"1":"iselia"}));
    fixture.write("base/scripts/start.sym", b"before");
    let library = fixture.open(&[])?;
    let mut first = library.snapshot();
    let original = first.read("scripts/start.sym", 64)?;
    assert_eq!(first.mappings(FIELDS)?.target(1)?, "iselia");
    fixture.write("base/scripts/start.sym", b"after");
    fixture.write("base/fields/mappings/1.json", br#""triet""#);
    assert!(Arc::ptr_eq(
        &original,
        &first.read("scripts/start.sym", 64)?
    ));
    assert_eq!(first.mappings(FIELDS)?.target(1)?, "iselia");
    assert!(first.read("scripts/start.sym", 2).is_err());
    let mut next = library.snapshot();
    assert_eq!(&*next.read("scripts/start.sym", 64)?, b"after");
    assert_eq!(next.mappings(FIELDS)?.target(1)?, "iselia");
    assert_eq!(
        fixture.open(&[])?.snapshot().mappings(FIELDS)?.target(1)?,
        "triet"
    );
    Ok(())
}

#[test]
fn bank_scopes_are_independent_and_missing_targets_name_the_mapping() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write(
        "base/audio/effects/church/mappings.json",
        br#"{"12":"bell.wav"}"#,
    );
    fixture.write(
        "base/audio/effects/forest/mappings.json",
        br#"{"12":"bird.wav"}"#,
    );
    fixture.write("base/audio/effects/church/bell.wav", b"bell");
    let mut snapshot = fixture.open(&[])?.snapshot();
    assert_eq!(
        snapshot.resolve("audio/effects/church/mappings.json", 12, None)?,
        "audio/effects/church/bell.wav"
    );
    let error = snapshot
        .resolve("audio/effects/forest/mappings.json", 12, None)
        .unwrap_err();
    let error = format!("{error:#}");
    assert!(error.contains("audio/effects/forest/mappings.json ID 12"));
    assert!(error.contains("audio/effects/forest/bird.wav"));
    Ok(())
}

#[test]
fn malformed_override_does_not_fall_back_and_no_legacy_layout_is_guessed() -> Result<()> {
    let fixture = Fixture::new();
    fixture.publish("base", FIELDS, serde_json::json!({"1":"iselia"}));
    fixture.write("broken/fields/mappings/1.json", b"malformed JSON");
    let error = format!("{:#}", fixture.open(&["broken"]).unwrap_err());
    assert!(error.contains("fields/mappings/1.json"), "{error}");
    fixture.write("broken/fields/mappings/1.json", br#""../outside""#);
    let error = format!(
        "{:#}",
        fixture
            .open(&["broken"])?
            .snapshot()
            .mappings(FIELDS)
            .unwrap_err()
    );
    assert!(error.contains("unsafe asset path ../outside"), "{error}");
    // A whole document cannot be combined with owners of the same document.
    fixture.write("whole/fields/mappings.json", br#"{"1":"triet"}"#);
    let error = format!("{:#}", fixture.open(&["whole"]).unwrap_err());
    assert!(
        error.contains("has both a whole document and granular owners"),
        "{error}"
    );
    fixture.write(
        "base/content.json",
        &serde_json::to_vec(&Descriptor {
            version: VERSION + 1,
            game: GAME.into(),
        })?,
    );
    assert!(fixture.open(&[]).is_err());
    fs::remove_file(fixture.0.path().join("base/content.json"))?;
    assert!(fixture.open(&[]).is_err());
    Ok(())
}

#[test]
fn streams_use_mod_selection_and_metadata_cannot_be_overridden() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write("base/movies/opening/video.mkv", b"original");
    fixture.write("replacement/movies/opening/video.mkv", b"replacement");
    let mut snapshot = fixture.open(&["replacement"])?.snapshot();
    let mut bytes = Vec::new();
    snapshot
        .open_stream("movies/opening/video.mkv")?
        .read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"replacement");
    assert!(snapshot.open_stream(DESCRIPTOR).is_err());
    fixture.write("replacement/content.json", b"{}");
    assert!(fixture.open(&["replacement"]).is_err());
    Ok(())
}

/// Two mods add items without choosing numbers. Each gets its own ID after the
/// original items, and neither can displace the other or an original prop.
#[test]
fn independent_mods_add_named_items_without_sharing_a_number() -> Result<()> {
    use crate::battle_model::{Attachment, document};
    let fixture = Fixture::new();
    fixture.publish(
        "base",
        crate::items::PATH,
        serde_json::json!({
            "mappings": {"0":"no-item", "1":"apple-gel"},
            "items": {
                "no-item": {"category":"unclassified"},
                "apple-gel": {"category":"gel"}
            }
        }),
    );
    // The original game binds props to numbers just past its item table.
    fixture.publish(
        "base",
        document::EQUIPMENT,
        serde_json::json!({"2":"prop","3":"prop"}),
    );
    for name in ["prop", "moon-blade"] {
        fixture.write(
            &format!("base/meshes/equipment/{name}/equipment.json"),
            br#"{"kind":"weapon","parts":{}}"#,
        );
    }
    fixture.write(
        "blades/game/items/items/moon-blade/definition.json",
        br#"{"category":"dual_blades","characters":["lloyd"],"equipment":"moon-blade"}"#,
    );
    fixture.write(
        "snacks/game/items/items/berry/definition.json",
        br#"{"category":"gel","field_usable":true}"#,
    );
    let ids = |mods: &[&str]| -> Result<crate::items::Prepared> {
        fixture
            .open(mods)?
            .snapshot()
            .json::<crate::items::Document>(crate::items::PATH, 1 << 20)?
            .prepare()
    };
    let both = ids(&["blades", "snacks"])?;
    assert_eq!(both.mappings.id("berry")?, 2);
    assert_eq!(both.mappings.id("moon-blade")?, 3);
    assert_eq!(
        both.allocated,
        BTreeMap::from([(2, "berry".to_owned()), (3, "moon-blade".to_owned())])
    );
    // Load order does not decide identity, and each mod works alone.
    assert_eq!(ids(&["snacks", "blades"])?.mappings.id("moon-blade")?, 3);
    assert_eq!(ids(&["blades"])?.mappings.id("moon-blade")?, 2);
    assert!(ids(&[])?.allocated.is_empty());

    let diagnostics = crate::diagnostics::Diagnostics::new(true);
    let mut snapshot = fixture.open(&["blades", "snacks"])?.snapshot();
    // The gel at ID 2 must not inherit the prop registered under that number.
    assert!(matches!(
        document::capture_equipment(&mut snapshot, 2, &diagnostics, &|| false)?,
        Attachment::Nonvisual
    ));
    assert!(matches!(
        document::capture_equipment(&mut snapshot, 3, &diagnostics, &|| false)?,
        Attachment::Weapon(_)
    ));
    Ok(())
}

#[test]
fn failures_name_the_mods_behind_a_document() -> Result<()> {
    let fixture = Fixture::new();
    fixture.publish(
        "base",
        crate::items::PATH,
        serde_json::json!({
            "mappings": {"0":"no-item"},
            "items": {"no-item": {"category":"unclassified"}}
        }),
    );
    fixture.write(
        "typo/game/items/items/berry/definition.json",
        br#"{"category":"gel","feild_usable":true}"#,
    );
    let library = fixture.open(&["typo"])?;
    assert_eq!(
        library.attribution(crate::items::PATH),
        " (with content from typo)"
    );
    assert_eq!(fixture.open(&[])?.attribution(crate::items::PATH), "");
    let mut snapshot = library.snapshot();
    snapshot.read(crate::items::PATH, 1 << 20)?;
    let files = crate::prepared::Files::from_snapshot(
        snapshot,
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    let error = format!("{:#}", crate::items::load(&files).err().unwrap());
    assert!(
        error.contains("with content from typo") && error.contains("feild_usable"),
        "{error}"
    );

    fixture.write("broken/game/items/items/berry/definition.json", b"{");
    let error = format!("{:#}", fixture.open(&["broken"]).unwrap_err());
    assert!(
        error.contains("with content from broken")
            && error.contains("broken/game/items/items/berry/definition.json"),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_mod_manifest_gates_its_layout_and_is_never_content() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write("base/ui/logo.ktx2", b"base");
    for name in ["current", "other"] {
        fixture.write(
            &format!("{name}/mod.json"),
            format!(r#"{{"layout":{VERSION},"name":"Example"}}"#).as_bytes(),
        );
    }
    fixture.write("current/ui/logo.ktx2", b"mod");
    for name in ["current", "other"] {
        fixture.write(&format!("{name}/README.md"), name.as_bytes());
    }
    fixture.write("current/sounds/extra.wav", b"unused");
    let library = fixture.open(&["current", "other"])?;
    let mut snapshot = library.snapshot();
    assert!(snapshot.read_optional(MANIFEST, 4096)?.is_none());
    assert!(snapshot.read_optional("README.md", 4096)?.is_none());
    assert_eq!(library.conflicts().count(), 0);
    assert_eq!(
        library.unfamiliar_roots(),
        [("current".to_owned(), "sounds".to_owned())]
    );

    fixture.write("stale/mod.json", br#"{"layout":106}"#);
    let error = fixture.open(&["stale"]).unwrap_err().to_string();
    assert!(
        error.contains("stale") && error.contains("layout 106") && error.contains("update"),
        "{error}"
    );
    fixture.write("typo/mod.json", br#"{"layout":108,"nmae":"x"}"#);
    assert!(fixture.open(&["typo"]).is_err());
    Ok(())
}

#[test]
fn files_supplied_by_two_mods_are_conflicts_and_base_replacements_are_not() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write("base/ui/logo.ktx2", b"base");
    fixture.write("first/ui/logo.ktx2", b"first");
    fixture.write("first/ui/extra.ktx2", b"first");
    fixture.write("last/ui/extra.ktx2", b"last");
    let library = fixture.open(&["first", "last"])?;
    assert_eq!(library.overrides().count(), 2);
    let conflicts: Vec<_> = library.conflicts().collect();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        (
            &*conflicts[0].path,
            &*conflicts[0].previous,
            &*conflicts[0].replacement
        ),
        ("ui/extra.ktx2", "first", "last")
    );
    Ok(())
}

#[test]
fn equipment_mapping_preserves_item_keys_and_nonvisual_choices() -> Result<()> {
    use crate::battle_model::{Attachment, document};
    let fixture = Fixture::new();
    fixture.publish(
        "base",
        document::EQUIPMENT,
        serde_json::json!({"777":"sword","778":"shield","367":null}),
    );
    for (name, attachment) in [("sword", "weapon"), ("shield", "shield")] {
        fixture.write(
            &format!("base/meshes/equipment/{name}/equipment.json"),
            &serde_json::to_vec(&serde_json::json!({"kind":attachment,
                "parts":{},"trails":{},"uv_animation":{}
            }))?,
        );
    }
    let mut snapshot = fixture.open(&[])?.snapshot();
    let diagnostics = crate::diagnostics::Diagnostics::new(true);
    assert!(matches!(
        document::capture_equipment(&mut snapshot, 777, &diagnostics, &|| false)?,
        Attachment::Weapon(_)
    ));
    assert!(matches!(
        document::capture_equipment(&mut snapshot, 778, &diagnostics, &|| false)?,
        Attachment::Shield(_)
    ));
    assert!(matches!(
        document::capture_equipment(&mut snapshot, 367, &diagnostics, &|| false)?,
        Attachment::Nonvisual
    ));
    assert!(document::capture_equipment(&mut snapshot, 0, &diagnostics, &|| false).is_err());
    fixture.write("redirect/meshes/equipment/mappings/777.json", b"null");
    assert!(matches!(
        document::capture_equipment(
            &mut fixture.open(&["redirect"])?.snapshot(),
            777,
            &diagnostics,
            &|| false
        )?,
        Attachment::Nonvisual
    ));
    assert!(matches!(
        document::capture_equipment(&mut snapshot, 777, &diagnostics, &|| false)?,
        Attachment::Weapon(_)
    ));
    Ok(())
}

#[test]
fn automatic_mods_require_six_digits_and_sort_by_priority_then_name() -> Result<()> {
    let fixture = Fixture::new();
    let directory = fixture.0.path().join("mods");
    assert!(discover_mods(&directory)?.is_empty());
    fixture.write("base/data.txt", b"base");
    for (name, value) in [
        ("999999-last", b"last".as_slice()),
        ("000000-artwork", b"artwork"),
        ("000020-b", b"b"),
        ("000020-a", b"a"),
    ] {
        fixture.write(&format!("mods/{name}/data.txt"), value);
    }
    fixture.write("mods/README.txt", b"instructions");
    fixture.write("mods/.resonance-arte-building/data.txt", b"not published");
    let mods = discover_mods(&directory)?;
    assert_eq!(
        mods.iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect::<Vec<_>>(),
        ["000000-artwork", "000020-a", "000020-b", "999999-last"]
    );
    let library = Library::open_paths(&fixture.0.path().join("base"), &mods)?;
    assert_eq!(&*library.snapshot().read("data.txt", 64)?, b"last");
    for name in ["12345-short", "abcdef-invalid", "123456xname"] {
        let path = directory.join(name);
        fs::create_dir(&path)?;
        assert!(discover_mods(&directory).is_err(), "accepted {name}");
        fs::remove_dir(path)?;
    }
    Ok(())
}

#[test]
fn runtime_reuse_copies_requested_bytes_without_retaining_a_scene() -> Result<()> {
    use crate::{diagnostics::Diagnostics, prepared::Files};
    let fixture = Fixture::new();
    fixture.write("base/shared.bin", b"shared");
    fixture.write("base/unique.bin", b"unique");
    let library = fixture.open(&[])?;
    let mut first = library.snapshot();
    let shared = first.read("shared.bin", 64)?;
    let unique = Arc::downgrade(&first.read("unique.bin", 64)?);
    let first = Arc::new(Files::from_snapshot(
        first,
        || false,
        Diagnostics::default(),
    )?);
    let owner = Arc::downgrade(&first);
    let mut second = library.snapshot();
    second.reuse(first.clone())?;
    assert!(Arc::ptr_eq(&shared, &second.read("shared.bin", 64)?));
    assert!(second.read("shared.bin", 1).is_err());
    let second = Files::from_snapshot(second, || false, Diagnostics::default())?;
    drop(first);
    assert!(
        owner.upgrade().is_none(),
        "destination retained the source scene owner"
    );
    assert!(
        unique.upgrade().is_none(),
        "unrequested source data survived eviction"
    );
    assert!(!second.contains_key("unique.bin"));
    fixture.write("base/shared.bin", b"edited");
    assert_eq!(&*library.snapshot().read("shared.bin", 64)?, b"edited");
    assert_eq!(&*second.read("shared.bin")?, b"shared");
    Ok(())
}
