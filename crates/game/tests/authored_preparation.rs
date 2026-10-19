use anyhow::{Result, ensure};
use resonance_events::{EventRuntime, GameWorld, ResourceLibrary};
use resonance_game::authored::{Entry, PreparedEvent, Resources};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::Program;
use symphonia_script_compiler::AssetReference;
use symphonia_script_tools::PreparationCache;

struct Available;
impl Resources for Available {
    fn asset(&mut self, reference: &AssetReference) -> Result<()> {
        anyhow::bail!("unprepared asset {}", reference.path)
    }
    fn message(&mut self, text: &str) -> Result<()> {
        ensure!(text.is_ascii(), "font does not contain required glyphs");
        Ok(())
    }
    fn substitution(&mut self, ty: symphonia_script::authored::Type) -> Result<()> {
        ensure!(
            ty == symphonia_script::authored::Type::I32,
            "label glyphs are unavailable"
        );
        self.message("0123456789-")
    }
}

fn runtime() -> EventRuntime {
    let words = [4u16, 0, 0, 0, 0x20ff];
    let program = Program::decode(
        &words
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut world = GameWorld::default();
    world.input_enabled = true;
    EventRuntime::with_state(
        Arc::new(program),
        Arc::new(ResourceLibrary::default()),
        world,
        Default::default(),
    )
    .unwrap()
}

#[test]
fn prepare_edit_and_activate_use_immutable_programs_without_recooking() {
    let original = "script field; use game::story; use game::field; pub task run() { await field::wait_ticks(ticks(2)); story::set_flag(41, true); }";
    let mut sources = BTreeMap::from([("field::example".into(), original.into())]);
    let entry = || Entry {
        module: "field::example",
        task: "run",
        arguments: &[],
    };
    let mut cache = PreparationCache::default();
    let prepared = PreparedEvent::prepare(&mut cache, &sources, entry(), &mut Available).unwrap();
    let mut events = runtime();
    prepared.start(&mut events).unwrap();
    events.step().unwrap();
    sources.insert("field::example".into(), original.replace("41", "42"));
    let replacement =
        PreparedEvent::prepare(&mut cache, &sources, entry(), &mut Available).unwrap();
    events.step().unwrap();
    events.step().unwrap();
    assert!(events.world.event_flags.contains(&41));
    assert!(!events.world.event_flags.contains(&42));
    replacement.start(&mut events).unwrap();
    for _ in 0..3 {
        events.step().unwrap();
    }
    assert!(events.world.event_flags.contains(&42));
    assert!(events.player_has_control());
    sources.insert("field::example".into(), "broken edit".into());
    assert!(PreparedEvent::prepare(&mut cache, &sources, entry(), &mut Available).is_err());
    assert!(events.player_has_control());
}

#[test]
fn invalid_entry_or_missing_message_glyph_blocks_preparation() {
    let sources = BTreeMap::from([(
        "notice".into(),
        "script field; use game::field; pub task run() { await field::notice(\"Café\"); }".into(),
    )]);
    let mut cache = PreparationCache::default();
    let entry = |task, arguments| Entry {
        module: "notice",
        task,
        arguments,
    };
    for bad in [entry("missing", &[]), entry("run", &[1]), entry("run", &[])] {
        assert!(PreparedEvent::prepare(&mut cache, &sources, bad, &mut Available).is_err());
    }
    for kind in ["model", "library"] {
        let sources = BTreeMap::from([(
            "notice".into(),
            format!("script {kind}; pub task run() {{}}"),
        )]);
        let error = PreparedEvent::prepare(&mut cache, &sources, entry("run", &[]), &mut Available)
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "event entry must declare script field");
    }
}

#[test]
fn external_entry_arguments_are_validated_before_activation() {
    let mut cache = PreparationCache::default();
    for (ty, valid, invalid) in [
        ("bool", 1, 2),
        ("ticks", 0, -1),
        ("f32", 1.0f32.to_bits() as i32, f32::NAN.to_bits() as i32),
    ] {
        let sources = BTreeMap::from([(
            "entry".into(),
            format!("script field; pub task run(value: {ty}) {{}}"),
        )]);
        let entry = |arguments| Entry {
            module: "entry",
            task: "run",
            arguments,
        };
        let valid = [valid];
        let invalid = [invalid];
        PreparedEvent::prepare(&mut cache, &sources, entry(&valid), &mut Available).unwrap();
        let error = PreparedEvent::prepare(&mut cache, &sources, entry(&invalid), &mut Available)
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("invalid arguments for event entry")
        );
    }
}

#[test]
fn message_preparation_checks_literals_and_typed_label_banks() {
    let mut cache = PreparationCache::default();
    let mut sources = BTreeMap::from([("message".into(), "script field; use game::field; message count(value:i32) = \"Count: {value}\"; pub task run() { await field::notice(count(-2)); }".into())]);
    let entry = || Entry {
        module: "message",
        task: "run",
        arguments: &[],
    };
    PreparedEvent::prepare(&mut cache, &sources, entry(), &mut Available).unwrap();
    sources.insert("message".into(), "script field; use game::field; use game::text; message found(value:text::Item) = \"Found: {value}\"; pub task run() { await field::notice(found(text::item(7))); }".into());
    let error = PreparedEvent::prepare(&mut cache, &sources, entry(), &mut Available)
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "label glyphs are unavailable");
    sources.insert("message".into(), "script field; use game::field; message count(value:i32) = \"Café {value}\"; pub task run() { await field::notice(count(2)); }".into());
    assert!(PreparedEvent::prepare(&mut cache, &sources, entry(), &mut Available).is_err());
}

#[test]
fn field_bindings_reload_edits_and_define_restore_timing() {
    use resonance_game::{authored::FieldScripts, field::EntryKind};
    use std::fs;
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = Directory(
        std::env::temp_dir().join(format!("resonance-field-scripts-{}", std::process::id())),
    );
    fs::create_dir(&root.0).unwrap();
    fs::write(
        root.0.join("fields.json"),
        r#"{"340":{"module":"entry","task":"run"}}"#,
    )
    .unwrap();
    let standard = BTreeMap::from([(
        "std::flags".into(),
        "script library; pub const First: i32 = 41; pub const Second: i32 = 42;".into(),
    )]);
    let source = "script field; use game::story; use std::flags; pub task run() { story::set_flag(flags::First, true); }";
    fs::write(root.0.join("entry.sym"), source).unwrap();
    let mut scripts = FieldScripts::new(root.0.clone());
    assert!(
        scripts
            .prepare(330, &standard, &mut Available)
            .unwrap()
            .is_none()
    );
    let original = scripts
        .prepare(340, &standard, &mut Available)
        .unwrap()
        .unwrap();
    assert!(original.for_entry(EntryKind::Restore).is_none());
    let original = original.for_entry(EntryKind::Arrival).unwrap();
    let mut events = runtime();
    original.start(&mut events).unwrap();
    fs::write(root.0.join("entry.sym"), source.replace("First", "Second")).unwrap();
    fs::write(
        root.0.join("fields.json"),
        r#"{"340":{"module":"entry","task":"run","on":"entry"}}"#,
    )
    .unwrap();
    let updated = scripts
        .prepare(340, &standard, &mut Available)
        .unwrap()
        .unwrap()
        .for_entry(EntryKind::Restore)
        .unwrap();
    events.step().unwrap();
    assert!(events.world.event_flags.contains(&41));
    assert!(!events.world.event_flags.contains(&42));
    updated.start(&mut events).unwrap();
    events.step().unwrap();
    assert!(events.world.event_flags.contains(&42));
    fs::create_dir(root.0.join("std")).unwrap();
    let override_path = root.0.join("std/flags.sym");
    fs::write(&override_path, [0xff, 0xfe]).unwrap();
    let cooked = scripts
        .prepare(340, &standard, &mut Available)
        .unwrap()
        .unwrap()
        .for_entry(EntryKind::Restore)
        .unwrap();
    let mut fresh = runtime();
    cooked.start(&mut fresh).unwrap();
    fresh.step().unwrap();
    assert!(fresh.world.event_flags.contains(&42));
    fs::write(
        &override_path,
        "script library; pub const Second: i32 = 99;",
    )
    .unwrap();
    let error = scripts
        .prepare(340, &BTreeMap::<String, String>::new(), &mut Available)
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("std::flags"));
    fs::remove_file(override_path).unwrap();
    fs::write(root.0.join("entry.sym"), "invalid source").unwrap();
    assert!(scripts.prepare(340, &standard, &mut Available).is_err());
    fs::write(
        root.0.join("fields.json"),
        r#"{"340":{"module":"entry","task":"run","on":"silently_ignore_errors"}}"#,
    )
    .unwrap();
    assert!(scripts.prepare(340, &standard, &mut Available).is_err());
}
