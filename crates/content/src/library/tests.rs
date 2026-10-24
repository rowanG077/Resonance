use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
mod music;
mod skits;
mod textures;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "resonance-content-library-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let fixture = Self(path);
        fixture.write(
            "base/content.json",
            br#"{"version":1,"game":"GQSEAF","revision":0}"#,
        );
        fixture
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn open(&self, names: &[&str]) -> Result<Library> {
        Library::open(
            &self.0.join("base"),
            &names
                .iter()
                .map(|name| Mod {
                    name: (*name).into(),
                    root: self.0.join(name),
                })
                .collect::<Vec<_>>(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn directory_selection_preserves_order_and_allows_equal_folder_names() -> Result<()> {
    let fixture = Fixture::new();
    for name in ["first", "second", "translated", "corrected"] {
        fixture.write(&format!("base/fields/{name}/field.json"), b"{}");
    }
    fixture.write(
        "base/fields/mappings.json",
        br#"{"1":"first","2":"second"}"#,
    );
    fixture.write(
        "one/translation/fields/mappings.json",
        br#"{"1":"translated"}"#,
    );
    fixture.write(
        "two/translation/fields/mappings.json",
        br#"{"1":"corrected"}"#,
    );
    let mods = [
        fixture.0.join("one/translation"),
        fixture.0.join("two/translation"),
    ];
    let library = Library::open_paths(&fixture.0.join("base"), &mods)?;
    let mut snapshot = library.snapshot();
    assert_eq!(snapshot.field(1)?, "fields/corrected/field.json");
    assert_eq!(snapshot.field(2)?, "fields/second/field.json");
    assert_eq!(
        Library::open_paths(&fixture.0.join("base"), &[mods[1].clone(), mods[0].clone()])?
            .snapshot()
            .field(1)?,
        "fields/translated/field.json"
    );
    assert!(
        Library::open_paths(&fixture.0.join("base"), &[mods[0].clone(), mods[0].clone()]).is_err()
    );
    Ok(())
}

#[test]
fn battle_pool_selection_and_worker_keep_one_effective_formation_table() -> Result<()> {
    use crate::{battle_formation as f, battle_rules};
    let fixture = Fixture::new();
    let mut table = battle_rules::Formations {
        version: battle_rules::VERSION,
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
    fixture.write("base/battle/formations.json", &serde_json::to_vec(&table)?);
    table.pools[0][0].formation = 1;
    fixture.write("mod/battle/formations.json", &serde_json::to_vec(&table)?);
    let library = fixture.open(&["mod"])?;
    let mut snapshot = library.snapshot();
    let selected = battle_rules::capture_formations(&mut snapshot)?.select(0, 0)?;
    assert_eq!(selected, 1);
    table.records[1].resources[0].enemy = 42;
    table.pools[0][0].formation = 0;
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
fn optional_roots_pin_absence_and_mod_bytes_without_scanning_assets() -> Result<()> {
    let fixture = Fixture::new();
    let library = fixture.open(&[])?;
    let mut absent = library.snapshot();
    assert!(!crate::overworld::document::present(&mut absent)?);
    fixture.write("base/world/world.json", b"base root");
    assert!(!crate::overworld::document::present(&mut absent)?);
    assert!(absent.read("world/world.json", 1024).is_err());
    assert!(crate::overworld::document::present(
        &mut library.snapshot()
    )?);
    fixture.write("mod/world/world.json", b"mod root");
    let modified = fixture.open(&["mod"])?;
    let mut snapshot = modified.snapshot();
    assert_eq!(
        &*snapshot.read_optional("world/world.json", 1024)?.unwrap(),
        b"mod root"
    );
    fs::remove_file(fixture.0.join("mod/world/world.json"))?;
    assert_eq!(&*snapshot.read("world/world.json", 1024)?, b"mod root");
    assert_eq!(
        &*modified
            .snapshot()
            .read_optional("world/world.json", 1024)?
            .unwrap(),
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
    assert_eq!(library.snapshot().files_under("scripts")?.len(), 3);
    assert!(crate::preparation::scripts(&mut library.snapshot(), &|| true).is_err());
    Ok(())
}

#[test]
fn namespace_discovery_rejects_case_ambiguous_names_and_non_directories() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write("base/scripts/field/start.sym", b"base");
    fixture.write("mod/scripts/Field/start.sym", b"mod");
    let error = format!(
        "{:#}",
        fixture
            .open(&["mod"])?
            .snapshot()
            .files_under("scripts")
            .unwrap_err()
    );
    assert!(error.contains("case-conflicting content paths"), "{error}");
    fixture.write("invalid/scripts", b"not a directory");
    assert!(
        fixture
            .open(&["invalid"])?
            .snapshot()
            .files_under("scripts")
            .is_err()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn namespace_discovery_rejects_directory_cycles() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write("base/scripts/start.sym", b"base");
    std::os::unix::fs::symlink(".", fixture.0.join("base/scripts/loop"))?;
    let error = format!(
        "{:#}",
        fixture
            .open(&[])?
            .snapshot()
            .files_under("scripts")
            .unwrap_err()
    );
    assert!(error.contains("directory aliases"), "{error}");
    Ok(())
}

#[test]
fn captured_resources_reach_runtime_and_edits_invalidate_cached_bytes() -> Result<()> {
    use crate::{
        diagnostics::Diagnostics,
        prepared::{Cache, Files},
    };
    let fixture = Fixture::new();
    fixture.write("base/fields/iselia/messages.json", br#"["original"]"#);
    fixture.write("base/fields/iselia/wall.png", b"unchanged image");
    let library = fixture.open(&[])?;
    let mut snapshot = library.snapshot();
    snapshot.read("fields/iselia/messages.json", 4096)?;
    snapshot.read("fields/iselia/wall.png", 4096)?;
    fixture.write("base/fields/iselia/messages.json", br#"["edited"]"#);
    let mut cache = Cache::default();
    let first = Files::from_snapshot(snapshot, &mut cache, || false, Diagnostics::new(true))?;
    assert_eq!(
        &*first.read("fields/iselia/messages.json")?,
        br#"["original"]"#
    );
    let mut snapshot = library.snapshot();
    snapshot.read("fields/iselia/messages.json", 4096)?;
    snapshot.read("fields/iselia/wall.png", 4096)?;
    let second = Files::from_snapshot(snapshot, &mut cache, || false, Diagnostics::new(true))?;
    assert_eq!(
        &*second.read("fields/iselia/messages.json")?,
        br#"["edited"]"#
    );
    assert_ne!(
        first.digest("fields/iselia/messages.json")?,
        second.digest("fields/iselia/messages.json")?
    );
    assert!(Arc::ptr_eq(
        &first.read("fields/iselia/wall.png")?,
        &second.read("fields/iselia/wall.png")?
    ));
    assert_eq!(second.reused_bytes, b"unchanged image".len() as u64);
    assert!(
        Files::from_snapshot(
            library.snapshot(),
            &mut cache,
            || true,
            Diagnostics::new(true)
        )
        .is_err()
    );
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
fn mod_mapping_merges_by_id_and_references_inherit_base_files() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write(
        "base/fields/mappings.json",
        br#"{"1":"iselia-1","2":"triet"}"#,
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
    fixture.write(
        "world/fields/mappings.json",
        br#"{"2":"alternate","3":"new-room"}"#,
    );
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
                path: FIELDS.into(),
                id: Some(2),
                previous: "base".into(),
                replacement: "world".into()
            },
            Override {
                path: path.into(),
                id: None,
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
    for (layer, target) in [("base", "one"), ("first", "two"), ("last", "three")] {
        fixture.write(
            &format!("{layer}/fields/mappings.json"),
            format!(r#"{{"1":"{target}"}}"#).as_bytes(),
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
    fixture.write("base/fields/mappings.json", br#"{"1":"iselia"}"#);
    fixture.write("base/scripts/start.sym", b"before");
    let library = fixture.open(&[])?;
    let mut first = library.snapshot();
    let original = first.read("scripts/start.sym", 64)?;
    let old_digest = first.digest("scripts/start.sym")?.to_owned();
    assert_eq!(first.mappings(FIELDS)?.target(1)?, "iselia");
    fixture.write("base/scripts/start.sym", b"after");
    fixture.write("base/fields/mappings.json", br#"{"1":"triet"}"#);
    assert!(Arc::ptr_eq(
        &original,
        &first.read("scripts/start.sym", 64)?
    ));
    assert_eq!(first.digest("scripts/start.sym")?, old_digest);
    assert_eq!(first.mappings(FIELDS)?.target(1)?, "iselia");
    assert!(first.read("scripts/start.sym", 2).is_err());
    let mut next = library.snapshot();
    assert_eq!(&*next.read("scripts/start.sym", 64)?, b"after");
    assert_ne!(next.digest("scripts/start.sym")?, old_digest);
    assert_eq!(next.mappings(FIELDS)?.target(1)?, "triet");
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
    fixture.write("base/fields/mappings.json", br#"{"1":"iselia"}"#);
    fixture.write(
        "broken/fields/mappings.json",
        br#"{"1":"triet","1":"other"}"#,
    );
    let mut snapshot = fixture.open(&["broken"])?.snapshot();
    let error = format!("{:#}", snapshot.mappings(FIELDS).unwrap_err());
    assert!(error.contains("broken") && error.contains("duplicate mapping ID 1"));
    fixture.write(
        "base/content.json",
        br#"{"version":99,"game":"GQSEAF","revision":0}"#,
    );
    assert!(fixture.open(&[]).is_err());
    fs::remove_file(fixture.0.join("base/content.json"))?;
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

#[cfg(unix)]
#[test]
fn a_symlink_cannot_escape_the_selected_content_layer() -> Result<()> {
    let fixture = Fixture::new();
    fixture.write("outside/secret.txt", b"outside");
    std::os::unix::fs::symlink(fixture.0.join("outside"), fixture.0.join("base/escaped"))?;
    let mut snapshot = fixture.open(&[])?.snapshot();
    assert!(snapshot.read("escaped/secret.txt", 4096).is_err());
    assert!(snapshot.read("../outside/secret.txt", 4096).is_err());
    Ok(())
}

#[test]
fn equipment_mapping_preserves_item_keys_and_nonvisual_choices() -> Result<()> {
    use crate::battle_model::{Attachment, document};
    let fixture = Fixture::new();
    fixture.write(
        "base/meshes/equipment/mappings.json",
        br#"{"777":"sword","778":"shield","367":"none"}"#,
    );
    for (name, attachment) in [("sword", "weapon"), ("shield", "shield")] {
        fixture.write(
            &format!("base/meshes/equipment/{name}/equipment.json"),
            &serde_json::to_vec(&serde_json::json!({"version":1,"attachment":{
                attachment:{"parts":{},"trails":{},"uv_animation":{}}
            }}))?,
        );
    }
    fixture.write(
        "base/meshes/equipment/none/equipment.json",
        br#"{"version":1,"attachment":"nonvisual"}"#,
    );
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
    fixture.write(
        "redirect/meshes/equipment/mappings.json",
        br#"{"777":"none"}"#,
    );
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
