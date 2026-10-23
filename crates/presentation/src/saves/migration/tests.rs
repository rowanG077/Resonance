use super::*;

fn libraries() -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    Ok((
        std::env::var_os("RESONANCE_MIGRATION_SOURCE")
            .context("set RESONANCE_MIGRATION_SOURCE to the old library")?
            .into(),
        std::env::var_os("RESONANCE_TEST_ASSETS")
            .context("set RESONANCE_TEST_ASSETS to the current library")?
            .into(),
    ))
}

fn fixture(source: &Path, target: &Path) -> Result<Vec<u8>> {
    let files = Files::load(
        target,
        &["fields/map-332.preload.json"],
        &mut Default::default(),
        || false,
    )?;
    let mut field = crate::test_support::field_checkpoint(&files)?;
    field
        .progress
        .party
        .settings
        .preferences
        .learn_both_arte_paths = true;
    field.progress.party.settings.preferences.battle_rank = 2;
    resonance_persistence::encode(
        &resonance_persistence::Header {
            identity: read_json(source, PATH)?,
            label: "Identity repair fixture".into(),
            location: "Field 332".into(),
            played_ticks: 1234,
            saved_unix_seconds: 5678,
        },
        &Checkpoint::Field(field),
    )
}

#[test]
#[ignore = "requires old/current complete libraries; creates only temporary synthetic saves"]
fn equivalent_libraries_repair_a_copy_without_changing_saved_state_or_overwriting() -> Result<()> {
    let (source, target) = libraries()?;
    let temporary = tempfile::tempdir()?;
    let input = temporary.path().join("original.json");
    let output = temporary.path().join("repaired.json");
    let bytes = fixture(&source, &target)?;
    fs::write(&input, &bytes)?;
    let checked = check_checkpoint_identity(&input, &source, &target)?;
    assert_eq!(fs::read_dir(temporary.path())?.count(), 1);
    let report = repair_checkpoint_identity(&input, &source, &target, &output)?;
    assert_eq!(checked.output_sha256, report.output_sha256);
    let old = resonance_persistence::decode::<serde_json::Value>(&bytes)?;
    let repaired_bytes = fs::read(&output)?;
    let repaired = resonance_persistence::decode::<serde_json::Value>(&repaired_bytes)?;
    let mut expected_header = old.header;
    expected_header.identity = report.current.clone();
    assert_eq!(repaired.header, expected_header);
    assert_eq!(repaired.state, old.state);
    assert_eq!(fs::read(&input)?, bytes);
    assert!(report.fields_compared > 0 && report.shared_scripts_compared > 0);
    resonance_persistence::decode::<Checkpoint>(&repaired_bytes)?.admit(&report.current)?;

    for destination in [&input, &output] {
        assert!(repair_checkpoint_identity(&input, &source, &target, destination).is_err());
    }
    assert_eq!(fs::read(&input)?, bytes);
    assert_eq!(fs::read(&output)?, repaired_bytes);
    Ok(())
}

/// Read-only aliases except for one explicitly replaced file.
#[cfg(unix)]
fn overlay_file(root: &Path, overlay: &Path, relative: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::symlink;
    let root = root.canonicalize()?;
    let mut components = relative.components();
    let name = components
        .next()
        .context("overlay path is empty")?
        .as_os_str();
    let remainder = components.as_path();
    fs::create_dir(overlay)?;
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let destination = overlay.join(entry.file_name());
        if entry.file_name() == name {
            if remainder.as_os_str().is_empty() {
                fs::write(destination, bytes)?;
            } else {
                overlay_file(&entry.path(), &destination, remainder, bytes)?;
            }
        } else {
            symlink(entry.path(), destination)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
#[ignore = "requires old/current complete libraries; negative cases use temporary overlays"]
fn repair_rejects_changed_content_stale_target_foreign_save_and_invalid_state() -> Result<()> {
    let (source, target) = libraries()?;
    let temporary = tempfile::tempdir()?;
    let input = temporary.path().join("original.json");
    let output = temporary.path().join("must-not-exist.json");
    let bytes = fixture(&source, &target)?;
    fs::write(&input, &bytes)?;

    let mut menu: serde_json::Value = read_json(&source, "game/menu-data.json")?;
    let price = menu["items"][1]["price"].as_u64().unwrap();
    menu["items"][1]["price"] = (price + 1).into();
    let changed_source = temporary.path().join("changed-source");
    overlay_file(
        &source,
        &changed_source,
        Path::new("game/menu-data.json"),
        &serde_json::to_vec(&menu)?,
    )?;
    let error = repair_checkpoint_identity(&input, &changed_source, &target, &output).unwrap_err();
    assert!(error.to_string().contains("definitions or scripts differ"));

    let shared: Shared = read_json(&source, resonance_content::field_preload::SHARED_PATH)?;
    let script = shared
        .files
        .keys()
        .find(|path| {
            (path.starts_with("scripts/battle/") || path.starts_with("scripts/std/"))
                && path.ends_with(".sym")
        })
        .context("fixture has no saved-state scripts")?;
    let mut script_bytes = fs::read(source.join(script))?;
    script_bytes.push(b'\n');
    let changed_script = temporary.path().join("changed-script");
    overlay_file(&source, &changed_script, Path::new(script), &script_bytes)?;
    let error = repair_checkpoint_identity(&input, &changed_script, &target, &output).unwrap_err();
    assert!(error.to_string().contains("definitions or scripts differ"));

    let stale_target = temporary.path().join("stale-target");
    let mut identity: Identity = read_json(&target, PATH)?;
    identity.content[0] ^= 1;
    overlay_file(
        &target,
        &stale_target,
        Path::new(PATH),
        &serde_json::to_vec(&identity)?,
    )?;
    let error = repair_checkpoint_identity(&input, &source, &stale_target, &output).unwrap_err();
    assert!(error.to_string().contains("target library has a stale"));

    let mut document = resonance_persistence::decode::<serde_json::Value>(&bytes)?;
    document.header.identity.content[0] ^= 1;
    fs::write(
        &input,
        resonance_persistence::encode(&document.header, &document.state)?,
    )?;
    let error = repair_checkpoint_identity(&input, &source, &target, &output).unwrap_err();
    assert!(error.to_string().contains("incompatible save"));

    let mut document = resonance_persistence::decode::<serde_json::Value>(&bytes)?;
    document.state["field"]["position"] = serde_json::json!([1e10, 1e10, 1e10]);
    fs::write(
        &input,
        resonance_persistence::encode(&document.header, &document.state)?,
    )?;
    let error = repair_checkpoint_identity(&input, &source, &target, &output).unwrap_err();
    assert!(format!("{error:#}").contains("outside the field ground"));
    assert!(!output.exists());
    Ok(())
}

#[test]
#[ignore = "requires GPU, libraries, RESONANCE_MIGRATION_SAVE and fresh RESONANCE_MIGRATION_REPLAY output; read-only save rehearsal"]
fn existing_save_repair_rehearsal_preserves_restored_party_without_writing_a_save() -> Result<()> {
    let (source, target) = libraries()?;
    let input = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_MIGRATION_SAVE").context("set RESONANCE_MIGRATION_SAVE")?,
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_MIGRATION_REPLAY").context("set RESONANCE_MIGRATION_REPLAY")?,
    );
    let original_bytes = resonance_persistence::read_bounded(&input)?;
    let original = resonance_persistence::decode::<Checkpoint>(&original_bytes)?;
    let Checkpoint::Field(expected) = original.state else {
        anyhow::bail!("this rehearsal requires a field checkpoint");
    };
    let (repair, candidate) = prepare(&input, &source, &target)?;
    let mut app = super::super::probe::app_with_saves_mode(
        &target,
        &output,
        crate::SaveOptions {
            directory: Some(output.join("slots")),
            quick_slot: Some("unused".into()),
            load: Some(input.clone()),
        },
        crate::Resolution::default(),
        true,
    )?;
    // The normal loader admits the fully verified repair candidate. Only its
    // in-memory request changes; neither the original nor a copied save is written.
    app.insert_resource(crate::new_game::Request(Some(candidate)));
    let scenario = serde_json::from_value(serde_json::json!({
        "version": 2,
        "steps": [
            {"do":"wait","until":{"kind":"field","map_id":expected.map_id,"free_control":true,"story":null,"max_x":null},"keys":[],"tap_every":null,"max_updates":1800},
            {"do":"capture","name":"restored-field"},
            {"do":"hold","keys":[],"updates":120},
            {"do":"capture","name":"resumed-field"}
        ]
    }))?;
    super::super::record_app(app, &output, &scenario, false)?;
    let recording: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("recording.json"))?)?;
    assert_eq!(recording["complete"], true);
    assert_eq!(recording["valid"], true);
    assert_eq!(recording["audio_device"], false);
    assert_eq!(recording["unprepared_reads"], 0);
    let initial = serde_json::from_value(recording["initial"].clone())?;
    super::super::assert_checkpoint(&initial, &expected)?;
    let captures = recording["captures"].as_array().unwrap();
    let restored = &captures[0]["checkpoint"]["progress"]["party"];
    ensure!(
        restored == &serde_json::to_value(&expected.progress.party)?,
        "restoration changed saved party state; inspect recording.json"
    );
    let resumed = &captures[1]["checkpoint"]["progress"]["party"];
    let elapsed = captures[1]["checkpoint"]["progress"]["tick"]
        .as_u64()
        .unwrap()
        - captures[0]["checkpoint"]["progress"]["tick"]
            .as_u64()
            .unwrap();
    ensure!(elapsed >= 120, "rehearsal did not resume 120 updates");
    let mut before = restored.clone();
    let mut after = resumed.clone();
    let mut clock_changes = BTreeMap::new();
    for clock in [
        "field_ticks",
        "scenario_ticks",
        "field_countdown",
        "ring_timer",
    ] {
        let old = before["travel"][clock].as_u64().unwrap();
        let new = after["travel"][clock].as_u64().unwrap();
        ensure!(
            old.abs_diff(new) <= elapsed,
            "saved {clock} jumped during rehearsal"
        );
        clock_changes.insert(clock, [old, new]);
        before["travel"][clock] = serde_json::Value::Null;
        after["travel"][clock] = serde_json::Value::Null;
    }
    ensure!(
        before == after,
        "resumed play changed party state beyond its clocks"
    );
    assert_eq!(fs::read(&input)?, original_bytes);
    assert!(!output.join("slots").exists());
    fs::write(
        output.join("repair-rehearsal.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "repair": repair,
            "save_written": false,
            "original_unchanged": true,
            "published_checkpoint_preserved": true,
            "restored_party_preserved": true,
            "resumed_party_preserved_except_clocks": true,
            "resumed_ticks": elapsed,
            "clock_changes": clock_changes
        }))?,
    )?;
    Ok(())
}
