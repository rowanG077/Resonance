use super::*;
use resonance_content::{
    diagnostics::Diagnostics,
    library::{Library, Mod},
};
use std::fs;

#[test]
#[ignore = "requires current named content at RESONANCE_TEST_ASSETS; prepares an independent editable copy"]
fn original_named_field_prepares_and_restores_without_inventory() -> Result<()> {
    let source = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS to current named content")?;
    let work = tempfile::tempdir()?;
    let copied = crate::test_support::named_opening_fixture()?;
    let root = copied.path().to_path_buf();
    assert!(available_fields(&root)?.contains(&332));

    let mut cache = Cache::default();
    let first = FieldPackage::prepare(&root, 332, &mut cache, || false)?;
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
    let identity = resonance_persistence::Identity::load(&first.files)?;
    assert_eq!(identity.schema, resonance_content::save_identity::SCHEMA);
    let (browse_identity, _) = super::super::save_context(&root, None, Diagnostics::new(true))?;
    assert_eq!(browse_identity, identity);
    let session = super::super::Session::load_prepared(
        Arc::new(first.clone()),
        Some(checkpoint.clone()),
        None,
    )?;
    assert_eq!(session.identity, identity);
    assert_eq!(session.map_id(), 332);
    let header = resonance_persistence::Header {
        identity: identity.clone(),
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
    let pending =
        crate::loading::Pending::start(root.clone(), None, Some(saved.clone()), None, &resident)?;
    let loaded = await_prepared(&pending)?;
    assert_eq!(loaded.identity, identity);
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
            &*first.files.read(&prepared.messages)?,
            fs::read(source.join(&original.messages))?,
            "skit {id} messages"
        );
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
    let messages_path = &first.assets.messages;
    let mut messages: Vec<symphonia_script::message::Message> =
        serde_json::from_slice(&first.messages)?;
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
    assert_eq!(
        resonance_persistence::Identity::load(&second.files)?,
        identity
    );
    assert_eq!(field.events.resources().messages[index], messages[index]);
    assert_ne!(first.messages, second.messages);
    assert!(
        Arc::ptr_eq(&first.script, &second.script),
        "unchanged bytes should remain shareable"
    );
    assert!(!second.files.overrides().is_empty());

    verify_identity_edits(
        &root,
        &first
            .skits
            .resources
            .values()
            .next()
            .context("no native skits")?
            .script,
        &identity,
    )?;

    // Admission must use newly captured gameplay, even if an old authoring file
    // claims that the save's fingerprint remains valid.
    let native = root.join(&first.assets.script);
    let original_script = fs::read(&native)?;
    let mut changed = original_script.clone();
    changed.push(0);
    fs::write(&native, changed)?;
    fs::write(
        root.join("game/save-identity.json"),
        serde_json::to_vec(&identity)?,
    )?;
    let pending = crate::loading::Pending::start(root.clone(), None, Some(saved), None, &resident)?;
    let error = match await_prepared(&pending) {
        Ok(_) => anyhow::bail!("changed scenario admitted a save using stale identity"),
        Err(error) => format!("{error:#}"),
    };
    assert!(error.contains("incompatible save"), "{error}");
    fs::write(&native, original_script)?;

    // A previously compiled service must not hide an invalid current edit.
    let service = root.join("scripts/field/memory.sym");
    fs::write(&service, "this is not a valid script")?;
    let error = match FieldPackage::prepare(&root, 332, &mut cache, || false) {
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

fn await_prepared<T: Send + 'static>(pending: &crate::loading::Task<T>) -> Result<T> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if let Some(result) = pending.poll()? {
            return result;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "session preparation timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
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
    let original: Vec<symphonia_script::message::Message> =
        snapshot.json(&definition.messages, 16 * 1024 * 1024)?;
    let work = tempfile::tempdir()?;
    let mods = [work.path().join("first"), work.path().join("second")];
    let write_message = |root: &Path, text: &str| -> Result<_> {
        let mut messages = original.clone();
        messages[0].tokens = vec![symphonia_script::message::Token::Text { text: text.into() }];
        let path = root.join(&definition.messages);
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
    assert_eq!(
        serde_json::from_slice::<Vec<symphonia_script::message::Message>>(&first.messages)?,
        selected
    );
    let identity = resonance_persistence::Identity::load(&first.files)?;
    assert_eq!(
        super::super::save_context_in(&library, None, Diagnostics::new(true))?.0,
        identity
    );
    assert_eq!(
        super::super::save_context(&root, None, Diagnostics::new(true))?.0,
        identity
    );
    let saved = resonance_persistence::encode(
        &resonance_persistence::Header {
            identity: identity.clone(),
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
    let fresh = await_prepared(&task)?;
    assert_eq!(
        serde_json::from_slice::<Vec<symphonia_script::message::Message>>(&fresh.messages)?,
        current
    );
    assert_eq!(
        serde_json::from_slice::<Vec<symphonia_script::message::Message>>(&first.messages)?,
        selected
    );
    let task = crate::loading::Pending::start(root, None, Some(saved), None, &resident)?;
    let restored = await_prepared(&task)?;
    assert_eq!(restored.identity, identity);
    assert_eq!(restored.field().events.resources().messages[0], current[0]);
    assert_eq!(
        restored.files().library().unwrap().snapshot().field(332)?,
        "fields/renamed-village/field.json"
    );
    Ok(())
}

#[test]
#[ignore = "requires a complete RESONANCE_NAMED_BATTLE_ASSETS fixture; validates external projects through field preparation and real save admission"]
fn named_field_project_identity_matches_browsing_and_restore() -> Result<()> {
    let root = std::env::var_os("RESONANCE_NAMED_BATTLE_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_NAMED_BATTLE_ASSETS")?;
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
    let package = FieldPackage::prepare(&root, 332, &mut cache, || false)?;
    let identity = resonance_persistence::Identity::load(&package.files)?;
    assert_eq!(
        super::super::save_context(&root, Some(&project), Diagnostics::new(true))?.0,
        identity
    );
    let saved = resonance_persistence::encode(
        &resonance_persistence::Header {
            identity: identity.clone(),
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
    let load = |project| -> Result<super::super::Session> {
        let task = crate::loading::Pending::start(
            root.clone(),
            project,
            Some(saved.clone()),
            None,
            &resident,
        )?;
        await_prepared(&task)
    };
    let initial = load(Some(project.clone()))?;
    assert_eq!(initial.identity, identity);
    assert_eq!(initial.map_id(), 332);
    fs::write(project.join("entry.sym"), source.replace("41", "42"))?;
    for candidate in [Some(project.clone()), None] {
        let error = load(candidate)
            .err()
            .context("incompatible project save was admitted")?;
        ensure!(
            format!("{error:#}").contains("incompatible save"),
            "unexpected admission error: {error:#}"
        );
    }
    fs::write(
        project.join("entry.sym"),
        format!("// Modder's notes\n{source}"),
    )?;
    let relocated = work.path().join("relocated");
    fs::rename(&project, &relocated)?;
    assert_eq!(load(Some(relocated))?.identity, identity);
    assert_eq!(
        initial.identity, identity,
        "retained field identity is immutable"
    );
    Ok(())
}

fn verify_identity_edits(
    root: &Path,
    skit_script: &str,
    expected: &resonance_persistence::Identity,
) -> Result<()> {
    use resonance_content::{
        field::document::Definition,
        library::{FIELDS, Mappings},
    };
    use resonance_persistence::Identity;
    let identity = || Identity::capture(&mut Library::open(root, &[])?.snapshot(), &|| false);
    let mut pinned = Library::open(root, &[])?.snapshot();
    assert_eq!(&Identity::capture(&mut pinned, &|| false)?, expected);

    let service = root.join("scripts/field/memory.sym");
    let source = fs::read_to_string(&service)?;
    fs::write(&service, format!("// Modder's notes\n{source}\n"))?;
    assert_eq!(&identity()?, expected, "comments preserve compatibility");
    fs::write(
        &service,
        format!("{source}\nconst ModGameplayValue: i32 = 17;\n"),
    )?;
    assert_ne!(
        &identity()?,
        expected,
        "service source changes are detected"
    );
    assert_eq!(
        &Identity::capture(&mut pinned, &|| false)?,
        expected,
        "snapshot remains pinned"
    );
    let declarations = format!(
        "{source}\nmessage ModCaption = \"Welcome\";\nconst ModKey: string = \"original-key\";\n"
    );
    fs::write(&service, &declarations)?;
    let declared = identity()?;
    fs::write(&service, declarations.replace("Welcome", "Bienvenue"))?;
    assert_eq!(identity()?, declared, "typed display text is presentation");
    fs::write(
        &service,
        declarations.replace("original-key", "changed-key"),
    )?;
    assert_ne!(
        identity()?,
        declared,
        "ordinary string keys retain their saved meaning"
    );
    fs::write(&service, source)?;

    let menu_path = root.join("game/menu-data.json");
    let menu = fs::read(&menu_path)?;
    let mut document: serde_json::Value = serde_json::from_slice(&menu)?;
    document["presentation"]["monsters"]["labels"]["title"] = "Edited catalogue label".into();
    fs::write(&menu_path, serde_json::to_vec(&document)?)?;
    assert_eq!(
        &identity()?,
        expected,
        "presentation and JSON formatting are not gameplay"
    );
    fs::write(&menu_path, &menu)?;
    let mut document: serde_json::Value = serde_json::from_slice(&menu)?;
    let hp = document["presentation"]["monsters"]["records"][7]["statistics"][0]["hp"]
        .as_u64()
        .context("missing monster HP")?;
    document["presentation"]["monsters"]["records"][7]["statistics"][0]["hp"] = (hp + 1).into();
    fs::write(&menu_path, serde_json::to_vec(&document)?)?;
    assert_ne!(
        &identity()?,
        expected,
        "Monster List statistics also govern combat"
    );
    fs::write(&menu_path, &menu)?;
    let session_path = root.join("game/session-data.json");
    let session = fs::read(&session_path)?;
    let mut document: serde_json::Value = serde_json::from_slice(&session)?;
    let limit = document["items"][0]["stack_limit"]
        .as_u64()
        .context("missing item limit")?;
    document["items"][0]["stack_limit"] = (limit + 1).into();
    fs::write(&session_path, serde_json::to_vec(&document)?)?;
    assert_ne!(
        &identity()?,
        expected,
        "saved inventory rules affect compatibility"
    );
    fs::write(&session_path, session)?;

    let mut current = Library::open(root, &[])?.snapshot();
    let path = current.field(332)?;
    let field: Definition = current.json(&path, 1024 * 1024)?;
    let original_collision = fs::read(root.join(&field.collision))?;
    let mut collision: resonance_content::field::document::Collision =
        serde_json::from_slice(&original_collision)?;
    collision
        .ground
        .first_mut()
        .context("fixture needs native ground")?
        .surface ^= 1 << 19;
    fs::write(root.join(&field.collision), serde_json::to_vec(&collision)?)?;
    assert_ne!(
        &identity()?,
        expected,
        "collision edits change saved gameplay"
    );
    assert_eq!(
        &Identity::capture(&mut pinned, &|| false)?,
        expected,
        "retained collision stays pinned"
    );
    fs::write(root.join(&field.collision), original_collision)?;
    let original_scene = fs::read(root.join(&field.scene))?;
    let mut scene: resonance_content::field::document::Scene =
        serde_json::from_slice(&original_scene)?;
    scene.contact_shadow.half_size += 1.;
    fs::write(root.join(&field.scene), serde_json::to_vec_pretty(&scene)?)?;
    assert_eq!(
        &identity()?,
        expected,
        "visual field settings preserve saved gameplay"
    );
    fs::write(root.join(&field.scene), original_scene)?;

    let skit_path = root.join(skit_script);
    let skit_bytes = fs::read(&skit_path)?;
    let mut changed = skit_bytes.clone();
    changed.push(0);
    fs::write(&skit_path, changed)?;
    assert_ne!(
        &identity()?,
        expected,
        "native skit edits affect compatibility"
    );
    fs::write(&skit_path, skit_bytes)?;

    let mapping = fs::read(root.join(FIELDS))?;
    let mut snapshot = Library::open(root, &[])?.snapshot();
    let field_path = snapshot.field(332)?;
    let mut field: Definition = snapshot.json(&field_path, 1024 * 1024)?;
    fs::create_dir_all(root.join("fields/renamed"))?;
    fs::copy(
        root.join(&field.script),
        root.join("fields/renamed/scenario.ssb"),
    )?;
    field.script = "fields/renamed/scenario.ssb".into();
    fs::write(
        root.join("fields/renamed/field.json"),
        serde_json::to_vec(&field)?,
    )?;
    fs::write(
        root.join(FIELDS),
        serde_json::to_vec(&Mappings::new([(332, "renamed".into())].into())?)?,
    )?;
    assert_eq!(
        &identity()?,
        expected,
        "mapping and native script path changes preserve IDs"
    );
    fs::write(root.join(FIELDS), b"{}")?;
    assert_ne!(&identity()?, expected, "removing a field ID is detected");
    fs::write(root.join(FIELDS), mapping)?;
    assert_eq!(&identity()?, expected);
    assert!(Identity::capture(&mut Library::open(root, &[])?.snapshot(), &|| true).is_err());
    Ok(())
}

/// Compatibility has a smaller preparation boundary than a playable field: it
/// needs current gameplay and scripts, independently of audio/art conversion.
#[test]
#[ignore = "requires current gameplay documents in RESONANCE_TEST_ASSETS; no audio, renderer or finished movie cook"]
fn named_save_identity_tracks_gameplay_and_preserves_presentation_edits() -> Result<()> {
    use resonance_content::{
        field::document::Definition,
        library::{Descriptor, FIELDS, Mappings},
        save_identity::Identity,
        skit::document as skits,
    };
    let source = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS to the gameplay cook")?;
    let work = tempfile::tempdir()?;
    let root = work.path().join("base");
    for directory in ["game", "fields/village", "skits/first", "scripts/field"] {
        fs::create_dir_all(root.join(directory))?;
    }
    for path in ["game/session-data.json", "game/menu-data.json"] {
        fs::copy(source.join(path), root.join(path)).with_context(|| format!("copy {path}"))?;
    }
    // Only model descriptors are needed for slot metadata; their geometry and
    // media remain deliberately absent from this compatibility fixture.
    let menu: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("game/menu-data.json"))?)?;
    for record in menu["presentation"]["monsters"]["records"]
        .as_array()
        .context("missing monster records")?
    {
        let path = record["preview"]
            .as_str()
            .context("preview must name a current model definition")?;
        fs::create_dir_all(root.join(path).parent().unwrap())?;
        fs::copy(source.join(path), root.join(path))?;
    }
    fs::write(
        root.join("content.json"),
        serde_json::to_vec(&Descriptor {
            version: resonance_content::library::VERSION,
            game: "GQSEAF".into(),
            revision: 0,
        })?,
    )?;
    let mut original_snapshot = Library::open(&source, &[])?.snapshot();
    let original_path = original_snapshot.field(332)?;
    let original: Definition = original_snapshot.json(&original_path, 1024 * 1024)?;
    let field = Definition {
        version: resonance_content::field::document::VERSION,
        script: "fields/village/scenario.ssb".into(),
        messages: "fields/village/messages.json".into(),
        scene: "fields/village/scene.json".into(),
        collision: "fields/village/collision.json".into(),
        actors: "fields/village/actors.json".into(),
        audio: "fields/village/audio.json".into(),
        movies: BTreeSet::new(),
    };
    fs::copy(source.join(&original.script), root.join(&field.script))?;
    let mut scene: resonance_content::field::document::Scene =
        original_snapshot.json(&original.scene, 64 * 1024 * 1024)?;
    scene.parts.clear();
    scene.doors.clear();
    let collision: resonance_content::field::document::Collision =
        original_snapshot.json(&original.collision, 64 * 1024 * 1024)?;
    fs::write(root.join(&field.scene), serde_json::to_vec(&scene)?)?;
    fs::write(root.join(&field.collision), serde_json::to_vec(&collision)?)?;
    fs::write(root.join(&field.actors), b"[]")?;
    fs::write(
        root.join("fields/village/field.json"),
        serde_json::to_vec(&field)?,
    )?;
    fs::write(
        root.join(FIELDS),
        serde_json::to_vec(&Mappings::new([(332, "village".into())].into())?)?,
    )?;
    let original = skits::capture(&mut original_snapshot, &|| false)?;
    let (id, original) = original
        .resources
        .into_iter()
        .next()
        .context("no native skits")?;
    let mut skit = original.clone();
    skit.script = "skits/first/scenario.ssb".into();
    skit.messages = "skits/first/messages.json".into();
    fs::copy(source.join(original.script), root.join(&skit.script))?;
    fs::write(
        root.join("skits/first/skit.json"),
        serde_json::to_vec(&skit)?,
    )?;
    fs::write(
        root.join(skits::MAPPINGS),
        serde_json::to_vec(&Mappings::new([(u32::from(id), "first".into())].into())?)?,
    )?;
    fs::write(
        root.join("scripts/field/memory.sym"),
        "script library; const Value: i32 = 7;",
    )?;
    let mut snapshot = Library::open(&root, &[])?.snapshot();
    let identity = Identity::capture(&mut snapshot, &|| false)?;
    let files = Files::from_snapshot(
        snapshot,
        &mut Default::default(),
        || false,
        Diagnostics::new(true),
    )?
    .with_identity(identity.clone())?;
    assert_eq!(Identity::load(&files)?, identity);
    assert!(!files.contains_key("game/save-identity.json"));
    assert_eq!(
        super::super::save_context(&root, None, Diagnostics::new(true))?.0,
        identity
    );
    fs::write(
        root.join("game/save-identity.json"),
        serde_json::to_vec(&identity)?,
    )?;
    verify_identity_edits(&root, &skit.script, &identity)?;

    let project = work.path().join("external-project");
    fs::create_dir(&project)?;
    fs::write(
        project.join("fields.json"),
        br#"{"332":{"module":"entry","task":"run","on":"entry"}}"#,
    )?;
    let source = "script field; use game::story; pub task run() { story::set_flag(41, true); }";
    fs::write(project.join("entry.sym"), source)?;
    let project_identity =
        resonance_game::authored::FieldProject::load(&project)?.bind_identity(&identity)?;
    assert_ne!(project_identity, identity);
    let context = || {
        super::super::save_context(&root, Some(&project), Diagnostics::new(true))
            .map(|value| value.0)
    };
    assert_eq!(
        context()?,
        project_identity,
        "title slot admission includes the selected external project"
    );
    fs::write(
        project.join("entry.sym"),
        format!("// Editing notes\n{source}"),
    )?;
    assert_eq!(context()?, project_identity);
    fs::write(project.join("entry.sym"), source.replace("41", "42"))?;
    assert_ne!(
        context()?,
        project_identity,
        "external project edits participate before save admission"
    );
    assert_eq!(
        super::super::save_context(&root, None, Diagnostics::new(true))?.0,
        identity
    );

    let modification = work.path().join("gameplay-mod");
    fs::create_dir_all(modification.join("fields/village"))?;
    let mut scenario = fs::read(root.join(&field.script))?;
    scenario.push(0);
    fs::write(modification.join(&field.script), scenario)?;
    let library = Library::open(
        &root,
        &[Mod {
            name: "gameplay".into(),
            root: modification,
        }],
    )?;
    let changed = Identity::capture(&mut library.snapshot(), &|| false)?;
    assert_ne!(
        changed, identity,
        "effective mod gameplay must override the stale authoring fingerprint"
    );
    assert_eq!(
        Identity::capture(&mut Library::open(&root, &[])?.snapshot(), &|| false)?,
        identity,
        "disabling the mod restores compatibility"
    );
    Ok(())
}
