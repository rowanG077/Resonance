use super::*;
use crate::test_support::await_preparation;
use resonance_content::{
    diagnostics::Diagnostics,
    library::{Library, Mod},
};
use std::fs;

#[test]
#[ignore = "requires current named content at RESONANCE_TEST_ASSETS; uses small mod overlays over the installed library"]
fn original_named_field_prepares_and_restores_without_inventory() -> Result<()> {
    let source = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS to current named content")?;
    let work = tempfile::tempdir()?;
    let root = source.clone();
    let base = Library::open(&root, &[])?;
    assert!(available_fields(&root)?.contains(&332));

    let mut cache = Cache::default();
    let first = FieldPackage::capture(&base, 332, &mut cache, || false, Diagnostics::new(true))?;
    assert!(
        first
            .files
            .keys()
            .all(|path| !path.ends_with(".preload.json"))
    );
    assert!(
        first
            .files
            .keys()
            .all(|path| !path.starts_with("assets/") && !path.ends_with(".preload.json"))
    );
    assert!(!root.join("shared.preload.json").exists());
    assert!(!root.join("game/skits.json").exists());
    let checkpoint = crate::test_support::field_checkpoint(&first.files)?;
    let session = super::super::Session::load_prepared(
        Arc::new(first.clone()),
        Some(checkpoint.clone()),
        None,
    )?;
    assert_eq!(session.map_id(), 332);
    let header = resonance_persistence::Header {
        version: resonance_persistence::VERSION,
        label: "Named field".into(),
        location: "Iselia".into(),
        played_ticks: 0,
        saved_unix_seconds: 0,
    };
    let saved = resonance_persistence::encode(
        &header,
        &crate::saves::SceneCheckpoint::Field(checkpoint.clone()),
    )?;
    let resident = crate::loading::Resident::default();
    *resident.files.write().unwrap() = Some(first.files.clone());
    let pending =
        crate::loading::Pending::start(root.clone(), None, Some(saved.clone()), None, &resident)?;
    let loaded = await_preparation(&pending)?;
    assert_eq!(loaded.map_id(), 332);
    let (field, restored) = first.restore(&checkpoint, [332].into())?;
    assert!(field.player_has_control());
    assert_eq!(
        serde_json::to_value(restored.as_ref().context("missing restored checkpoint")?)?,
        serde_json::to_value(&field.checkpoint()?)?
    );
    let baseline_skits = resonance_content::skit::document::capture(
        &mut Library::open(&source, &[])?.snapshot(),
        &|| false,
    )?;
    assert_eq!(first.skits.preview_order, baseline_skits.preview_order);
    assert_eq!(first.skits.resources.len(), baseline_skits.resources.len());
    for (id, original) in &baseline_skits.resources {
        let prepared = &first.skits.resources[id];
        assert_eq!(
            prepared.allow_skip, original.allow_skip,
            "skit {id} skip policy"
        );
        assert_eq!(prepared.title, original.title, "skit {id} title");
        assert_eq!(
            &*first.files.read(&prepared.script)?,
            fs::read(source.join(&original.script))?,
            "skit {id} script"
        );
        assert_eq!(
            prepared.messages, original.messages,
            "skit {id} dialogue presence"
        );
        if let Some(path) = &prepared.messages {
            assert_eq!(
                &*first.files.read(path)?,
                fs::read(source.join(path))?,
                "skit {id} messages"
            );
        }
    }
    assert_eq!(first.skits.media.len(), baseline_skits.media.len());
    for (id, original) in &baseline_skits.media {
        let prepared = &first.skits.media[id];
        assert_eq!(
            (prepared.frames, prepared.sample_rate),
            (original.frames, original.sample_rate),
            "skit media {id}"
        );
        assert_eq!(prepared.voice.is_some(), original.voice.is_some());
    }

    // A one-file mod inherits the field, shared roots, scripts and artwork.
    let mod_root = work.path().join("dialogue");
    let messages_path = first
        .assets
        .messages
        .as_deref()
        .context("fixture has no dialogue")?;
    let mut messages: Vec<symphonia_script::message::Message> = first.messages.to_vec();
    let index = messages
        .iter()
        .position(|message| {
            message.tokens.iter().any(|token| {
                matches!(token,
                    symphonia_script::message::Token::Text { text } if text.contains(' ')
                )
            })
        })
        .context("field has no dialogue text")?;
    messages[index].tokens = vec![symphonia_script::message::Token::Text {
        text: "Edited village dialogue.".into(),
    }];
    fs::create_dir_all(mod_root.join(messages_path).parent().unwrap())?;
    fs::write(
        mod_root.join(messages_path),
        serde_json::to_vec_pretty(&messages)?,
    )?;
    let library = Library::open(
        &root,
        &[Mod {
            name: "dialogue".into(),
            root: mod_root,
        }],
    )?;
    let second =
        FieldPackage::capture(&library, 332, &mut cache, || false, Diagnostics::new(true))?;
    let (field, _) = second.restore(&checkpoint, [332].into())?;
    assert_eq!(field.events.resources().messages[index], messages[index]);
    assert_ne!(first.messages, second.messages);
    assert_eq!(first.script, second.script);
    assert!(!second.files.overrides().is_empty());

    // An unrelated, missing field must not block loading this save. Only the
    // saved location's dependencies are prepared.
    let gameplay = work.path().join("gameplay");
    fs::create_dir_all(gameplay.join("fields"))?;
    fs::write(
        gameplay.join("fields/mappings.json"),
        br#"{"65534":"unrelated-missing-field"}"#,
    )?;
    let edited = Library::open_paths(&root, &[gameplay.clone()])?;
    let reader = Files::from_snapshot(edited.snapshot(), || false, Diagnostics::new(true))?;
    *resident.files.write().unwrap() = Some(Arc::new(reader));
    let pending = crate::loading::Pending::start(root.clone(), None, Some(saved), None, &resident)?;
    assert_eq!(await_preparation(&pending)?.map_id(), 332);

    // A previously compiled service must not hide an invalid current edit.
    let service = gameplay.join("scripts/field/memory.sym");
    fs::create_dir_all(service.parent().unwrap())?;
    fs::write(&service, "this is not a valid script")?;
    let restarted = Library::open_paths(&root, &[gameplay])?;
    let error = match FieldPackage::capture(
        &restarted,
        332,
        &mut cache,
        || false,
        Diagnostics::new(true),
    ) {
        Ok(_) => anyhow::bail!("invalid edited service reused a previous compilation"),
        Err(error) => format!("{error:#}"),
    };
    assert!(
        error.contains("memory") || error.contains("script"),
        "{error}"
    );
    assert!(
        FieldPackage::capture(&library, 332, &mut cache, || true, Diagnostics::new(false)).is_err()
    );
    println!(
        "Named field 332 restored with {} captured files, {} skits and {} media clocks; a one-file dialogue mod and invalid service edit were observed",
        first.files.len(),
        first.skits.resources.len(),
        first.skits.media.len()
    );
    Ok(())
}

#[test]
#[ignore = "requires complete RESONANCE_NAMED_BATTLE_ASSETS; follows ordered mod layers through real field preparation and save admission"]
fn named_mod_layers_survive_worker_preparation_and_restore() -> Result<()> {
    let root = std::env::var_os("RESONANCE_NAMED_BATTLE_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_NAMED_BATTLE_ASSETS")?;
    let base = Library::open(&root, &[])?;
    let mut snapshot = base.snapshot();
    let path = snapshot.field(332)?;
    let definition: resonance_content::field::document::Definition =
        snapshot.json(&path, 1024 * 1024)?;
    let original: Vec<symphonia_script::message::Message> = snapshot.json(
        definition
            .messages
            .as_deref()
            .context("fixture has no dialogue")?,
        16 * 1024 * 1024,
    )?;
    let work = tempfile::tempdir()?;
    let mods = [work.path().join("first"), work.path().join("second")];
    let write_message = |root: &Path, text: &str| -> Result<_> {
        let mut messages = original.clone();
        messages[0].tokens = vec![symphonia_script::message::Token::Text { text: text.into() }];
        let path = root.join(definition.messages.as_deref().unwrap());
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, serde_json::to_vec(&messages)?)?;
        Ok(messages)
    };
    write_message(&mods[0], "First translation")?;
    let selected = write_message(&mods[1], "Selected translation")?;
    fs::write(
        mods[1].join("fields/mappings.json"),
        br#"{"332":"renamed-village"}"#,
    )?;
    fs::create_dir_all(mods[1].join("fields/renamed-village"))?;
    fs::write(
        mods[1].join("fields/renamed-village/field.json"),
        serde_json::to_vec(&definition)?,
    )?;
    let library = Library::open_paths(&root, &mods)?;
    let first = FieldPackage::capture(
        &library,
        332,
        &mut Default::default(),
        || false,
        Diagnostics::new(true),
    )?;
    assert_eq!(first.messages.to_vec(), selected);
    let saved = resonance_persistence::encode(
        &resonance_persistence::Header {
            version: resonance_persistence::VERSION,
            label: "Mod fixture".into(),
            location: "Iselia".into(),
            played_ticks: 0,
            saved_unix_seconds: 0,
        },
        &crate::saves::SceneCheckpoint::Field(crate::test_support::field_checkpoint(&first.files)?),
    )?;
    let resident = crate::loading::Resident::default();
    *resident.files.write().unwrap() = Some(first.files.clone());
    let current = write_message(&mods[1], "Newly edited translation")?;
    let task = crate::loading::FieldPending::field(root.clone(), None, 332, &resident)?;
    let edited = await_preparation(&task)?;
    assert_eq!(edited.messages.to_vec(), current);

    // Restart is the supported reload boundary. A new reader keeps the same
    // ordered mod layers but starts a fresh gameplay view and runtime caches.
    let restarted = Library::open_paths(&root, &mods)?;
    let reader = Files::from_snapshot(restarted.snapshot(), || false, Diagnostics::new(true))?;
    let resident = crate::loading::Resident::default();
    *resident.files.write().unwrap() = Some(Arc::new(reader));
    let task = crate::loading::FieldPending::field(root.clone(), None, 332, &resident)?;
    let fresh = await_preparation(&task)?;
    *resident.files.write().unwrap() = Some(fresh.files.clone());
    assert_eq!(fresh.messages.to_vec(), current);
    assert_eq!(first.messages.to_vec(), selected);
    // A prepared field belongs to the running application. Revisit and quickload
    // reuse that exact package; edits are picked up after restarting, as above.
    write_message(&mods[1], "Edit for the next application restart")?;
    let task = crate::loading::FieldPending::field(root.clone(), None, 332, &resident)?;
    let revisited = await_preparation(&task)?;
    assert!(Arc::ptr_eq(&fresh, &revisited));
    let task = crate::loading::Pending::start(root, None, Some(saved), None, &resident)?;
    let restored = await_preparation(&task)?;
    assert_eq!(restored.field().events.resources().messages[0], current[0]);
    assert_eq!(
        restored.files().library().unwrap().snapshot().field(332)?,
        "fields/renamed-village/field.json"
    );
    Ok(())
}

#[test]
#[ignore = "requires a complete RESONANCE_NAMED_BATTLE_ASSETS fixture; validates external projects through field preparation and real save admission"]
fn named_field_save_loads_after_authored_gameplay_edits() -> Result<()> {
    let root = std::env::var_os("RESONANCE_NAMED_BATTLE_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_NAMED_BATTLE_ASSETS")?;
    // Only the external project changes in this test. Reuse the unchanged
    // library session while each preparation captures the current project.
    let library = Library::open(&root, &[])?;
    let work = tempfile::tempdir()?;
    let project = work.path().join("project");
    fs::create_dir(&project)?;
    fs::write(
        project.join("fields.json"),
        br#"{"332":{"module":"entry","task":"run","on":"entry"}}"#,
    )?;
    let source = "script field; use game::story; pub task run() { story::set_flag(41, true); }";
    fs::write(project.join("entry.sym"), source)?;
    let mut cache = Cache {
        scripts: Some(resonance_game::authored::FieldScripts::new(project.clone())),
        ..Default::default()
    };
    let package =
        FieldPackage::capture(&library, 332, &mut cache, || false, Diagnostics::new(true))?;
    let saved = resonance_persistence::encode(
        &resonance_persistence::Header {
            version: resonance_persistence::VERSION,
            label: "Project fixture".into(),
            location: "Iselia".into(),
            played_ticks: 0,
            saved_unix_seconds: 0,
        },
        &crate::saves::SceneCheckpoint::Field(crate::test_support::field_checkpoint(
            &package.files,
        )?),
    )?;
    let resident = crate::loading::Resident::default();
    *resident.files.write().unwrap() = Some(package.files.clone());
    let load = |project| -> Result<super::super::Session> {
        let task = crate::loading::Pending::start(
            root.clone(),
            project,
            Some(saved.clone()),
            None,
            &resident,
        )?;
        await_preparation(&task)
    };
    let mut initial = load(Some(project.clone()))?;
    assert_eq!(initial.map_id(), 332);
    for _ in 0..8 {
        initial.field_mut().step(Default::default())?;
    }
    assert!(initial.field().events.world.event_flags.contains(&41));
    assert!(!initial.field().events.world.event_flags.contains(&42));
    fs::write(project.join("entry.sym"), source.replace("41", "42"))?;
    let mut changed = load(Some(project.clone()))?;
    for _ in 0..8 {
        changed.field_mut().step(Default::default())?;
    }
    assert!(changed.field().events.world.event_flags.contains(&42));
    assert!(!changed.field().events.world.event_flags.contains(&41));
    assert_eq!(load(None)?.map_id(), 332);
    // The same save runs the current project's instructions.
    let edited =
        FieldPackage::capture(&library, 332, &mut cache, || false, Diagnostics::new(true))?;
    let checkpoint = crate::test_support::field_checkpoint(&edited.files)?;
    let (mut field, _) = edited.restore(&checkpoint, edited.fields.clone())?;
    for _ in 0..8 {
        field.step(Default::default())?;
    }
    assert!(field.events.world.event_flags.contains(&42));
    assert!(!field.events.world.event_flags.contains(&41));
    fs::write(
        project.join("entry.sym"),
        format!("// Modder's notes\n{source}"),
    )?;
    let relocated = work.path().join("relocated");
    fs::rename(&project, &relocated)?;
    assert_eq!(load(Some(relocated))?.map_id(), 332);
    Ok(())
}
