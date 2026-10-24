//! Persistent native fixture publication is separate from repeated encounter
//! checks, so a playback failure never requires rendering the soundtrack again.
use super::*;
use resonance_content::{menu_data::MenuData, session::SessionData};
use resonance_events::{
    battle::{DefeatPolicy, Encounter, Setup},
    party::Party,
};
use resonance_game::battle::encounter::{Inputs, PrepareOptions};
use std::path::PathBuf;

fn directory(variable: &str) -> Result<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .with_context(|| format!("set {variable}"))
}

#[test]
#[ignore = "publishes title fields to a new RESONANCE_NAMED_TITLE_FIELDS mod directory; uses shared resources from the named battle fixture"]
fn publish_named_title_fields_fixture() -> Result<()> {
    let source = directory("RESONANCE_TEST_ASSETS")?;
    let output = directory("RESONANCE_NAMED_TITLE_FIELDS")?;
    fs::create_dir(&output).context("title fixture destination must be new")?;
    let extracted = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut publisher = resonance_import::named::Publisher::new(&source, &output)?
        .with_audio_source(resonance_import::named::AudioSource::read(
            &extracted.join("disc1"),
            Some(&extracted.join("disc2")),
        )?);
    let mut snapshot = resonance_content::library::Library::open(&source, &[])?.snapshot();
    for (id, name) in [(474, "title-day"), (531, "title-night")] {
        let (definition, field) =
            resonance_content::field::document::capture_scene(&mut snapshot, id, || false)?;
        let art = resonance_import::named::FieldArt::capture(&mut snapshot, &field)?;
        publisher.field(name, field, art, definition.movies)?;
    }
    publisher.finish_fields()?;
    eprintln!(
        "Published title fields for layered runtime checks to {}",
        output.display()
    );
    Ok(())
}

#[test]
#[ignore = "publishes original data to a new RESONANCE_NAMED_BATTLE_ASSETS directory; requires both original discs"]
fn publish_complete_named_battle_fixture() -> Result<()> {
    let output = directory("RESONANCE_NAMED_BATTLE_ASSETS")?;
    let extracted = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let discs = [extracted.join("disc1"), extracted.join("disc2")];
    let report = resonance_import::all_assets::cook(&resonance_import::all_assets::Options {
        discs: &discs,
        output: &output,
        jobs: 3,
        memory_budget: 2 * 1024 * 1024 * 1024,
    })?;
    ensure!(
        report.failures.is_empty(),
        "native fixture publication failed: {:?}",
        report.failures
    );
    Ok(())
}

fn options(formation: u16) -> PrepareOptions {
    PrepareOptions {
        hunting_symbol: None,
        event_flags: if formation == 2 {
            [19, 24, 29].into()
        } else {
            Default::default()
        },
        devils_arms_unlocked: false,
        victory_story_flags: [false; 2],
        random_seed: 0x2345,
        map: 332,
        world_music: 0,
        story: 2500,
        colette_state: if formation == 2 { 1000 } else { 0 },
        overlimit_boost: false,
    }
}

#[test]
#[ignore = "requires complete RESONANCE_NAMED_BATTLE_ASSETS; exercises the production battle worker with a mod-owned recording"]
fn named_battle_worker_retains_mod_selection_and_rechecks_edits() -> Result<()> {
    use crate::{battle::Entry, loading, new_game};
    let root = directory("RESONANCE_NAMED_BATTLE_ASSETS")?;
    let modification = tempfile::tempdir()?;
    let owner = modification.path().join("audio/music/replacement");
    fs::create_dir_all(&owner)?;
    fs::write(
        modification.path().join("audio/music/mappings.json"),
        br#"{"91":"replacement"}"#,
    )?;
    fs::write(owner.join("music.json"), br#"{"version":1,"audio":"audio/music/replacement/music.wav","loop":{"start_frame":0,"end_frame":8}}"#)?;
    let mut wave = hound::WavWriter::create(
        owner.join("music.wav"),
        hound::WavSpec {
            channels: 2,
            sample_rate: 32000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for frame in 0..8i16 {
        wave.write_sample(frame * 100)?;
        wave.write_sample(-frame * 100)?;
    }
    wave.finalize()?;
    let library = Library::open_paths(&root, &[modification.path().into()])?;
    let field = new_game::FieldPackage::capture(
        &library,
        332,
        &mut Default::default(),
        || false,
        Diagnostics::new(true),
    )?;
    let checkpoint = crate::test_support::field_checkpoint(&field.files)?;
    let mut session = new_game::Session::load_prepared(Arc::new(field), Some(checkpoint), None)?;
    session
        .field_mut()
        .events
        .world
        .request_battle(Setup {
            route: [0; 5],
            encounter: Encounter::Formation(1),
            arena: 13,
            defeat: DefeatPolicy::GameOver,
            music: Some(91),
        })
        .map_err(anyhow::Error::msg)?;
    let request = session
        .field_mut()
        .events
        .world
        .battle_request
        .take()
        .unwrap();
    let before = serde_json::to_value(&session.events().world.party)?;
    let resident = loading::Resident::default();
    *resident.files.write().unwrap() = Some(session.files());
    let task = loading::BattlePending::battle(Entry::capture(&session, &request)?, &resident)?;
    let package = crate::test_support::await_preparation(&task)?;
    assert_eq!(package.prepared.music, 91);
    let selected: resonance_content::music::Definition = package
        .assets
        .files
        .json("audio/music/replacement/music.json")?;
    assert_eq!(selected.repeat.unwrap().end_frame, 8);
    assert!(
        package
            .assets
            .files
            .overrides()
            .iter()
            .any(
                |choice| choice.path == resonance_content::music::MAPPINGS && choice.id == Some(91)
            )
    );
    assert!(!package.prepared.models.is_empty());
    assert_eq!(serde_json::to_value(&session.events().world.party)?, before);
    assert!(!resident.diagnostics.has_errors());

    // A new preparation must validate current mod bytes even though the same
    // logical song was successfully prepared above and still has a live owner.
    fs::write(owner.join("new.wav"), b"invalid current recording")?;
    fs::rename(owner.join("new.wav"), owner.join("music.wav"))?;
    let task = loading::BattlePending::battle(Entry::capture(&session, &request)?, &resident)?;
    let error = crate::test_support::await_preparation(&task)
        .err()
        .context("invalid mod recording silently used the base or old cache")?;
    ensure!(
        format!("{error:#}").contains("music resource 91"),
        "unexpected recording failure: {error:#}"
    );
    assert_eq!(serde_json::to_value(&session.events().world.party)?, before);
    Ok(())
}

#[test]
#[ignore = "requires complete RESONANCE_NAMED_BATTLE_ASSETS; prepares native encounters and their WAV audio"]
fn named_complete_encounters_prepare_from_current_library() -> Result<()> {
    let root = directory("RESONANCE_NAMED_BATTLE_ASSETS")?;
    verify_encounters(Library::open(&root, &[])?, true)
}

#[test]
#[ignore = "requires published named battle rules, models and audio mappings; CPU preparation without PCM"]
fn named_encounter_models_and_simulation_prepare_all_costumes() -> Result<()> {
    let root = directory("RESONANCE_NAMED_BATTLE_ASSETS")?;
    // A component fixture can be checked while the separate soundtrack job is
    // still running. Its own base descriptor does not mark that publication
    // complete, and this check never reads the unfinished audio namespace.
    let base = tempfile::tempdir()?;
    fs::write(
        base.path().join("content.json"),
        br#"{"version":1,"game":"GQSEAF","revision":0}"#,
    )?;
    let library = Library::open(
        base.path(),
        &[resonance_content::library::Mod {
            name: "named-battle-components".into(),
            root,
        }],
    )?;
    verify_encounters(library, false)
}

fn verify_encounters(library: Library, with_audio: bool) -> Result<()> {
    let diagnostics = Diagnostics::new(true);
    let mut snapshot = library.snapshot();
    resonance_content::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let mut cache = crate::loading::Cache::default();
    let files = Files::from_snapshot(snapshot, &mut cache.bytes, || false, diagnostics.clone())?;
    let data = Arc::new(SessionData::load(&files)?);
    let menus = Arc::new(MenuData::load(&files)?);
    let mut cases = vec![(1, vec![1, 2, 3], 0), (2, vec![1, 2, 3], 0)];
    for costume in 0..5 {
        for members in [vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9]] {
            if costume == 4 && members == [9] {
                continue;
            }
            cases.push((2, members, costume));
        }
    }
    cases.push((43, vec![1, 2, 3], 0));
    cases.push((4, vec![1, 2, 3, 9], 0));
    for (formation, members, costume) in cases {
        let started = std::time::Instant::now();
        let mut party = Party::new(&data, Default::default()).map_err(anyhow::Error::msg)?;
        party.formation = members.clone();
        party.field_leader = members[0];
        for &character in &members {
            let index = usize::from(character - 1);
            let member = &mut party.members[index];
            member.costume = costume;
            if let Some(title) = menus.titles[index]
                .iter()
                .position(|title| title.costume.is_some_and(|value| value as u8 == costume))
            {
                member.title = (title + 1).try_into()?;
            }
        }
        for (index, level) in [(0, 3), (1, 1), (2, 2)] {
            party
                .raise_level(&data, index, level, None, || 7)
                .map_err(anyhow::Error::msg)?;
        }
        let event = formation == 4;
        let mut snapshot = library.snapshot();
        let catalogue =
            resonance_content::battle_audio::document::Catalogue::capture(&mut snapshot, &|| {
                false
            })?;
        let inputs = Inputs::capture(
            &mut snapshot,
            &menus,
            &data,
            &party,
            Setup {
                route: if event { [2, 3, 600, 0, 3] } else { [0; 5] },
                encounter: Encounter::Formation(formation),
                arena: if event { 26 } else { 13 },
                defeat: DefeatPolicy::GameOver,
                music: event.then_some(91),
            },
            &mut cache.bytes,
            diagnostics.clone(),
            &|| false,
        )?;
        let (assets, mut prepared, selected) =
            inputs.prepare_named(&menus, options(formation), &catalogue)?;
        let audio = if with_audio {
            Some(crate::battle_audio::Assets::capture_named(
                &catalogue,
                &selected,
                &mut snapshot,
                diagnostics.clone(),
                &mut cache.audio,
                &|| false,
            )?)
        } else {
            None
        };
        assert_eq!(
            prepared.characters,
            members
                .iter()
                .map(|&id| resonance_battle::Character::try_from(id))
                .collect::<Result<Vec<_>>>()?
        );
        assert_eq!(prepared.results.formation, formation);
        let scenery = stage::Model::load(&assets.files, &assets.stage)?;
        assert!(!scenery.is_empty());
        assert!(!prepared.models.is_empty());
        let mut combat = false;
        for _ in 0..600 {
            let current = prepared.core.step(Default::default())?;
            combat |= current.combat_active;
        }
        assert!(event || combat, "ordinary encounter never entered combat");
        // Capturing the selected stream proves current WAV bounds and ownership;
        // the field/battle audio tests exercise its mixer and handoff clocks.
        drop(audio);
        assert!(!diagnostics.has_errors());
        eprintln!(
            "Named formation {formation}, party {members:?}, costume {costume} (recorded audio={with_audio}): 600 simulation frames advanced, {} render models, {} effect banks, {} songs, {} cues, {} voices, checked in {:.2}s",
            prepared.models.len(),
            prepared.effect_banks.len(),
            selected.music.len(),
            selected.sounds.len(),
            selected.streams.len() + selected.voices.len(),
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
