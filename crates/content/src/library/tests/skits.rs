use super::*;
use crate::skit::document;

fn wave(rate: u32, frames: u32) -> Result<Vec<u8>> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(
        &mut bytes,
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for _ in 0..frames {
        writer.write_sample(123i16)?;
    }
    writer.finalize()?;
    Ok(bytes.into_inner())
}

fn fixture() -> Result<Fixture> {
    let fixture = Fixture::new();
    fixture.write(
        "base/fields/mappings.json",
        br#"{"81":"house-of-salvation"}"#,
    );
    fixture.write(
        "base/skits/catalogue.json",
        &serde_json::to_vec(&serde_json::json!({
            "skits": [], "preview_order": [1],
            "resources": {"1": {"allow_skip": false}},
            "media": {"ids": [100, 101, 102], "speed": 1.000875}
        }))?,
    );
    fixture.write(
        "base/skits/portraits.json",
        br#"{"mappings":{},"images":{},"animations":{}}"#,
    );
    fixture.write("base/skits/mappings.json", br#"{"1":"arrival"}"#);
    fixture.write("base/skits/arrival/scenario.ssb", b"scenario");
    fixture.write(
        "base/audio/speech/mappings.json",
        br#"{"100":"skits/line.wav","101":"skits/line.wav", "102":{
            "name":"skits/arrival/colette-001", "silence":{"frames":1000,"sample_rate":32000,"channels":1}
        }, "999":"unselected-missing.wav"}"#,
    );
    fixture.write("base/audio/speech/skits/line.wav", &wave(32000, 16)?);
    Ok(fixture)
}

#[test]
fn skit_media_uses_current_pcm_and_merged_mappings_with_shared_silent_clocks() -> Result<()> {
    let fixture = fixture()?;
    fixture.write(
        "mod/audio/speech/mappings.json",
        br#"{"100":"skits/replacement.wav"}"#,
    );
    fixture.write("mod/audio/speech/skits/replacement.wav", &wave(24000, 32)?);
    let library = fixture.open(&["mod"])?;
    let mut snapshot = library.snapshot();
    let captured = document::capture(&mut snapshot, &|| false)?;
    assert_eq!(
        (
            captured.media[&100].frames,
            captured.media[&100].sample_rate
        ),
        (32, 24021)
    );
    assert_eq!(
        (
            captured.media[&101].frames,
            captured.media[&101].sample_rate
        ),
        (16, 32028)
    );
    assert_eq!(
        captured.media[&101]
            .voice
            .as_ref()
            .unwrap()
            .source_sample_rate,
        32000
    );
    assert_eq!(
        (
            captured.media[&102].frames,
            captured.media[&102].sample_rate
        ),
        (1000, 32028)
    );
    assert!(captured.media[&102].voice.is_none());
    assert!(!captured.resources[&1].allow_skip);
    fixture.write("mod/audio/speech/skits/replacement.wav", &wave(48000, 64)?);
    let held = document::capture(&mut snapshot, &|| false)?;
    assert_eq!(held.media[&100].frames, 32);
    let fresh = document::capture(&mut library.snapshot(), &|| false)?;
    assert_eq!(
        (fresh.media[&100].frames, fresh.media[&100].sample_rate),
        (64, 48042)
    );
    let files = crate::prepared::Files::from_snapshot(
        snapshot,
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    let resident = document::read(|path, _| {
        files
            .contains_key(path)
            .then(|| files.read(path))
            .transpose()
    })?;
    assert_eq!(resident.media[&100].frames, 32);
    assert!(files.keys().all(|path| !path.ends_with(".preload.json")));
    Ok(())
}

#[test]
fn silent_skit_can_be_voiced_by_changing_only_the_shared_mapping() -> Result<()> {
    let fixture = fixture()?;
    fs::create_dir(fixture.0.path().join("mod"))?;
    let library = fixture.open(&["mod"])?;
    let mut snapshot = library.snapshot();
    assert!(
        document::capture(&mut snapshot, &|| false)?.media[&102]
            .voice
            .is_none()
    );
    fixture.write(
        "mod/audio/speech/mappings.json",
        br#"{"102":"skits/arrival/colette-001.wav"}"#,
    );
    fixture.write(
        "mod/audio/speech/skits/arrival/colette-001.wav",
        &wave(44100, 4410)?,
    );
    let fresh = document::capture(&mut library.snapshot(), &|| false)?;
    let line = &fresh.media[&102];
    assert_eq!((line.frames, line.sample_rate), (4410, 44138));
    assert_eq!(
        line.voice.as_ref().unwrap().path,
        "audio/speech/skits/arrival/colette-001.wav"
    );
    assert!(
        document::capture(&mut snapshot, &|| false)?.media[&102]
            .voice
            .is_none()
    );
    assert_eq!(fresh.media.len(), 3); // The missing, unselected WAV is never opened.
    Ok(())
}

#[test]
fn invalid_skit_replacements_and_cancellation_do_not_reuse_old_media() -> Result<()> {
    let fixture = fixture()?;
    let library = fixture.open(&[])?;
    assert!(document::capture(&mut library.snapshot(), &|| true).is_err());
    fixture.write("base/audio/speech/skits/line.wav", b"invalid WAV");
    let error = format!(
        "{:#}",
        document::capture(&mut library.snapshot(), &|| false).unwrap_err()
    );
    assert!(
        error.contains("decode skit speech 100: audio/speech/skits/line.wav"),
        "{error}"
    );
    Ok(())
}

#[test]
fn skit_dialogue_can_be_added_by_a_mod_without_changing_the_catalogue() -> Result<()> {
    let fixture = fixture()?;
    fs::create_dir(fixture.0.path().join("mod"))?;
    let library = fixture.open(&["mod"])?;
    let mut snapshot = library.snapshot();
    let absent = document::capture(&mut snapshot, &|| false)?;
    assert!(absent.resources[&1].messages.is_none());
    let path = "skits/arrival/messages.json";
    fixture.write(&format!("mod/{path}"), br#"["", "New dialogue", ""]"#);
    // Absence belongs to the preparation too; a disk addition cannot change it.
    assert!(
        document::capture(&mut snapshot, &|| false)?.resources[&1]
            .messages
            .is_none()
    );
    let mut fresh = library.snapshot();
    let added = document::capture(&mut fresh, &|| false)?;
    assert_eq!(added.resources[&1].messages.as_deref(), Some(path));
    let files = crate::prepared::Files::from_snapshot(
        fresh,
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    assert_eq!(
        files.json_or_default::<Vec<String>>(added.resources[&1].messages.as_deref())?,
        ["", "New dialogue", ""]
    );
    let files = crate::prepared::Files::from_snapshot(
        snapshot,
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    let held = document::read(|path, _| {
        files
            .contains_key(path)
            .then(|| files.read(path))
            .transpose()
    })?;
    assert!(held.resources[&1].messages.is_none());
    assert!(
        files
            .json_or_default::<Vec<String>>(held.resources[&1].messages.as_deref())?
            .is_empty()
    );

    fixture.write(&format!("mod/{path}"), b"malformed JSON");
    let mut snapshot = library.snapshot();
    let malformed = document::capture(&mut snapshot, &|| false)?;
    let files = crate::prepared::Files::from_snapshot(
        snapshot,
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    assert!(
        files
            .json_or_default::<Vec<String>>(malformed.resources[&1].messages.as_deref())
            .is_err()
    );
    Ok(())
}

#[test]
fn optional_dialogue_does_not_make_the_scenario_optional() -> Result<()> {
    let fixture = fixture()?;
    fs::remove_file(fixture.0.path().join("base/skits/arrival/scenario.ssb"))?;
    let error = format!(
        "{:#}",
        document::capture(&mut fixture.open(&[])?.snapshot(), &|| false).unwrap_err()
    );
    assert!(
        error.contains("missing skit resource skits/arrival/scenario.ssb"),
        "{error}"
    );
    Ok(())
}

#[test]
fn shared_skit_owners_keep_ids_and_allow_independent_mod_overrides() -> Result<()> {
    let fixture = fixture()?;
    fixture.write(
        "base/skits/mappings.json",
        br#"{"1":"arrival","2":"arrival"}"#,
    );
    fs::create_dir(fixture.0.path().join("mod"))?;
    let library = fixture.open(&["mod"])?;
    let mut held = library.snapshot();
    let shared = document::capture(&mut held, &|| false)?;
    assert_eq!(shared.resources[&1].script, shared.resources[&2].script);
    assert!(!shared.resources[&1].allow_skip);
    assert!(shared.resources[&2].allow_skip);
    fixture.write("mod/skits/mappings.json", br#"{"2":"new-conversation"}"#);
    fixture.write("mod/skits/new-conversation/scenario.ssb", b"replacement");
    fixture.write("mod/skits/new-conversation/messages.json", br#"["Hello"]"#);
    let fresh = document::capture(&mut library.snapshot(), &|| false)?;
    assert_eq!(fresh.resources[&1].script, "skits/arrival/scenario.ssb");
    assert_eq!(
        fresh.resources[&2].script,
        "skits/new-conversation/scenario.ssb"
    );
    assert!(fresh.resources[&1].messages.is_none());
    assert_eq!(
        fresh.resources[&2].messages.as_deref(),
        Some("skits/new-conversation/messages.json")
    );
    assert_eq!(
        document::capture(&mut held, &|| false)?.resources[&2].script,
        "skits/arrival/scenario.ssb"
    );
    Ok(())
}

#[test]
fn skit_field_names_follow_modded_mappings_and_retained_snapshots() -> Result<()> {
    let fixture = fixture()?;
    fixture.write(
        "base/skits/catalogue.json",
        br#"{
        "skits":[{"id":1,"title":"Arrival","trigger":"entry",
            "location":{"field":"house-of-salvation"},
            "condition":{"fields":{"from":"house-of-salvation","through":{"id":94}}}}],
        "preview_order":[1]
    }"#,
    );
    let mut retained = fixture.open(&[])?.snapshot();
    let old = document::capture(&mut retained, &|| false)?;
    fixture.write(
        "mod/fields/mappings.json",
        br#"{"81":"another-place","82":"house-of-salvation"}"#,
    );
    let current = document::capture(&mut fixture.open(&["mod"])?.snapshot(), &|| false)?;
    let field = |catalog: &crate::skit::SkitCatalog| -> Result<u16> {
        let crate::skit::SkitLocation::Map(field) = &catalog.skits[0].location else {
            anyhow::bail!("expected field")
        };
        field.id()
    };
    assert_eq!(field(&old)?, 81);
    assert_eq!(field(&current)?, 82);
    assert_eq!(field(&document::capture(&mut retained, &|| false)?)?, 81);
    let crate::skit::SkitCondition::Maps(range) = &current.skits[0].condition else {
        anyhow::bail!("expected range")
    };
    assert_eq!((range.from.id()?, range.through.id()?), (82, 94));
    fixture.write("mod/fields/mappings.json", br#"{"81":"another-place"}"#);
    assert!(
        format!(
            "{:#}",
            document::capture(&mut fixture.open(&["mod"])?.snapshot(), &|| false).unwrap_err()
        )
        .contains("unknown skit field house-of-salvation")
    );
    Ok(())
}
