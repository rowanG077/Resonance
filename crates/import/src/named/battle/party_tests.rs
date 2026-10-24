use super::tests::{equivalent, no_inventory};
use super::*;
use resonance_content::{
    battle_model::document as models,
    battle_victory::document as victory,
    diagnostics::Diagnostics,
    library::{Library, Mod},
};

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and both original discs; checks every party costume and victory owner"]
fn original_party_costumes_and_victories_have_explicit_editable_owners() -> Result<()> {
    let source = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS")?;
    let output = tempfile::tempdir()?;
    let selections = (1..=9)
        .flat_map(|character| {
            (0..=4)
                .filter(move |&costume| character != 9 || costume != 4)
                .map(move |costume| (character, costume))
        })
        .collect::<Vec<_>>();
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let native = tempfile::tempdir()?;
    let records = crate::battle_model::party::prepare_all(&extracted.join("disc1"), native.path())?;
    let original_victory = crate::battle_victory::prepare(&extracted.join("disc1"), native.path())?;
    assert_eq!(records.keys().copied().collect::<Vec<_>>(), selections);
    let mut publisher = Publisher::new(native.path(), output.path())?;
    publisher.battle_party(records.clone())?;
    publisher.battle_victories(&original_victory, "battle")?;
    let mut field_publisher = Publisher::new(&source, output.path())?.with_audio_source(
        AudioSource::read(&extracted.join("disc1"), Some(&extracted.join("disc2")))?,
    );
    let mut snapshot = Library::open(&source, &[])?.snapshot();
    let (_, field) =
        resonance_content::field::document::capture_scene(&mut snapshot, 332, || false)?;
    let art = FieldArt::capture(&mut snapshot, &field)?;
    field_publisher.field("iselia-village", field, art, Default::default())?;
    field_publisher.finish_fields()?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    let mut digests = BTreeMap::new();
    let mut clip_count = 0;
    let mut victory_count = 0;
    for &(character, costume) in &selections {
        let mut original = records[&(character, costume)].clone();
        let (definition, named) =
            models::capture_party(&mut snapshot, character, costume, &diagnostics, &|| false)?;
        let selected = victory::capture(
            &mut snapshot,
            [(character, costume, definition.victory.as_str())],
            &diagnostics,
            &|| false,
        )?;
        let mut expected = original_victory
            .ordinary
            .iter()
            .filter(|row| row.character == character && row.costume == Some(costume))
            .cloned()
            .collect::<Vec<_>>();
        for row in &mut expected {
            row.body_sha256.clear();
            row.source_sha256.clear();
        }
        equivalent(
            &expected,
            &selected.ordinary,
            native.path(),
            output.path(),
            &mut digests,
        )?;
        assert!(
            selected
                .ordinary
                .iter()
                .all(|row| row.costume == Some(costume))
        );
        assert_eq!(selected.ordinary.len(), 5);
        assert_eq!(
            serde_json::to_value(&selected.groups)?,
            serde_json::to_value(&original_victory.groups)?
        );
        original.body_sha256.clear();
        original.motion_sha256.clear();

        equivalent(
            &original,
            &named,
            native.path(),
            output.path(),
            &mut digests,
        )?;
        clip_count += named
            .parts
            .iter()
            .map(|part| part.clips.len())
            .sum::<usize>();
        victory_count += selected.ordinary.len();

        for part in &named.parts {
            assert!(part.mesh.starts_with("meshes/characters/"));
            assert!(
                part.clips
                    .iter()
                    .all(|clip| clip.motion.starts_with("battle/party/"))
            );
        }
    }
    for path in publisher
        .published
        .keys()
        .filter(|path| path.starts_with("battle/party/") && path.ends_with(".json"))
    {
        no_inventory(
            &serde_json::from_slice(&fs::read(output.path().join(path))?)?,
            path,
        );
    }
    // Both lookup scopes can be renamed independently. The new party entry
    // inherits shared geometry and explicitly references its victory owner.
    let modification = tempfile::tempdir()?;
    let write = |path: &str, bytes: &[u8]| -> Result<()> {
        fs::create_dir_all(modification.path().join(path).parent().unwrap())?;
        Ok(fs::write(modification.path().join(path), bytes)?)
    };
    let (mut definition, original) =
        models::capture_party(&mut snapshot, 1, 0, &diagnostics, &|| false)?;
    write(models::PARTY, br#"{"1":"hero"}"#)?;
    write("battle/party/hero/mappings.json", br#"{"0":"traveller"}"#)?;
    let mut visuals: models::Visuals = snapshot.json(&definition.model, 16 * 1024 * 1024)?;
    let mut motion = resonance_content::animation::Motion::decode(
        &snapshot.read(&visuals.parts[0].clips[0].motion, 16 * 1024 * 1024)?,
    )?;
    motion.duration_frames += 30.;
    let clip = "battle/party/hero/traveller/edited.motion.json";
    visuals.parts[0].clips[0].motion = clip.into();
    write(clip, &motion.encode()?)?;
    definition.model = "battle/party/hero/traveller/model.json".into();
    write(&definition.model, &serde_json::to_vec(&visuals)?)?;
    write(
        "battle/party/hero/traveller/party.json",
        &serde_json::to_vec(&definition)?,
    )?;
    let modified = Library::open(
        output.path(),
        &[Mod {
            name: "costume edit".into(),
            root: modification.path().into(),
        }],
    )?;
    let mut edited_snapshot = modified.snapshot();
    let (edited_definition, edited) =
        models::capture_party(&mut edited_snapshot, 1, 0, &diagnostics, &|| false)?;
    assert_eq!(
        edited.parts[0].clips[0].duration_seconds,
        motion.duration_frames / resonance_content::animation::FRAME_HZ
    );
    let selected = victory::capture(
        &mut edited_snapshot,
        [(1, 0, edited_definition.victory.as_str())],
        &diagnostics,
        &|| false,
    )?;
    assert_eq!(selected.ordinary.len(), 5);
    assert!(
        selected
            .ordinary
            .iter()
            .all(|row| row.character == 1 && row.costume == Some(0) && row.body_sha256.is_empty())
    );
    assert_eq!(
        models::capture_party(&mut snapshot, 1, 0, &diagnostics, &|| false)?
            .1
            .parts[0]
            .clips[0]
            .motion,
        original.parts[0].clips[0].motion
    );
    assert!(models::capture_party(&mut modified.snapshot(), 1, 0, &diagnostics, &|| true).is_err());
    definition.rig = "battle/party/hero/traveller/missing.json".into();
    write(
        "battle/party/hero/traveller/party.json",
        &serde_json::to_vec(&definition)?,
    )?;
    let error =
        models::capture_party(&mut modified.snapshot(), 1, 0, &diagnostics, &|| false).unwrap_err();
    assert!(format!("{error:#}").contains("missing.json"));
    assert!(models::capture_party(&mut edited_snapshot, 1, 0, &diagnostics, &|| false).is_ok());
    eprintln!(
        "verified {} costumes, {clip_count} body clips, {victory_count} costume-owned performances, 54 groups, {} publications including field 332",
        selections.len(),
        publisher.published.len()
    );
    Ok(())
}
