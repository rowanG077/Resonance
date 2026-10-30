//! Persistent native fixture publication is separate from repeated encounter
//! checks, so a playback failure never requires rendering the soundtrack again.
use super::*;
use resonance_content::{menu_data::MenuData, session::SessionData};
use resonance_events::{
    battle::{DefeatPolicy, Encounter, Setup},
    party::Party,
};
use resonance_game::battle::encounter::{Assets, PrepareOptions};
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
    let mut snapshot = resonance_content::library::Library::open(&source, &[])?.snapshot();
    let mut mappings = std::collections::BTreeMap::new();
    for id in [474, 531] {
        let field = snapshot.field(id)?;
        let folder = field
            .strip_suffix("/field.json")
            .context("field document path")?;
        let name = folder.strip_prefix("fields/").context("field category")?;
        mappings.insert(id, name.to_owned());
        // This is a runtime fixture: reuse field-owned files and inherit shared
        // character/audio/UI resources from the base library, without recooking.
        for path in snapshot.files_under(folder)?.clone() {
            let target = output.join(&path);
            fs::create_dir_all(target.parent().context("fixture parent")?)?;
            fs::write(target, snapshot.read(&path, 128 * 1024 * 1024)?)?;
        }
    }
    let mappings = resonance_content::library::Mappings::new(mappings)?;
    fs::write(
        output.join("fields/mappings.json"),
        serde_json::to_vec_pretty(&mappings)?,
    )?;
    eprintln!(
        "Published title fields for layered runtime checks to {}",
        output.display()
    );
    Ok(())
}

#[test]
#[ignore = "publishes native data to a new RESONANCE_NAMED_BATTLE_ASSETS directory; requires extracted GameCube files"]
fn publish_complete_named_battle_fixture() -> Result<()> {
    let output = directory("RESONANCE_NAMED_BATTLE_ASSETS")?;
    let extracted = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    resonance_importer::cook_all(&extracted, &output)?;
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
#[ignore = "requires installed assets; prepares event actors using shared field settings without PCM"]
fn named_event_actors_capture_shared_field_settings() -> Result<()> {
    let library = named_library()?;
    let diagnostics = Diagnostics::new(true);
    let mut snapshot = library.snapshot();
    resonance_content::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let files = Files::from_snapshot(snapshot, || false, diagnostics.clone())?;
    let data = SessionData::load(&files)?;
    let menus = MenuData::load(&files)?;
    let mut party = Party::new(&data, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![1, 2, 3, 9];
    let mut snapshot = library.snapshot();
    let catalogue =
        resonance_content::battle_audio::document::Catalogue::capture(&mut snapshot, &|| false)?;
    let inputs = Assets::capture(
        &mut snapshot,
        &menus,
        &data,
        &party,
        Setup {
            route: [2, 3, 600, 0, 3],
            encounter: Encounter::Formation(4),
            arena: 26,
            defeat: DefeatPolicy::GameOver,
            music: Some(91),
        },
        diagnostics.clone(),
        &|| false,
    )?;
    let (assets, mut prepared, _) = inputs.prepare_named(&menus, options(4), &catalogue)?;
    assert!(
        assets
            .files
            .contains_key(resonance_content::field::settings::PATH)
    );
    let _actors = crate::battle::event_models::State::new(
        &assets.files,
        prepared.lifecycle.event_scenes(),
        &prepared.core,
        &prepared.characters,
        &mut prepared.models,
    )?;
    assert!(!diagnostics.has_errors());
    Ok(())
}

#[test]
#[ignore = "requires complete RESONANCE_NAMED_BATTLE_ASSETS; exercises the production battle worker with a mod-owned recording"]
fn named_battle_worker_retains_mod_selection_and_rechecks_edits() -> Result<()> {
    use crate::{battle::Entry, loading, new_game};
    let root = directory("RESONANCE_NAMED_BATTLE_ASSETS")?;
    let modification = tempfile::tempdir()?;
    let owner = modification.path().join("audio/music");
    fs::create_dir_all(&owner)?;
    fs::write(
        modification.path().join("audio/music/mappings.json"),
        br#"{"91":"replacement.wav"}"#,
    )?;
    let mut wave = hound::WavWriter::create(
        owner.join("replacement.wav"),
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
    resonance_asset_writer::wav::append_loop(
        &mut fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(owner.join("replacement.wav"))?,
        32000,
        0..8,
    )?;
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
    let selected = resonance_audio::recording::Prepared::new(resonance_content::music::capture(
        &mut library.snapshot(),
        "audio/music/replacement.wav",
        &|| false,
    )?)?;
    assert_eq!(selected.repeat(), Some(0..8));
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
    fs::rename(owner.join("new.wav"), owner.join("replacement.wav"))?;
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
#[ignore = "requires complete RESONANCE_NAMED_BATTLE_ASSETS; CPU preparation without PCM"]
fn named_encounter_models_and_simulation_prepare_all_costumes() -> Result<()> {
    let root = directory("RESONANCE_NAMED_BATTLE_ASSETS")?;
    verify_encounters(Library::open(&root, &[])?, false)
}

fn verify_encounters(library: Library, with_audio: bool) -> Result<()> {
    let diagnostics = Diagnostics::new(true);
    let mut snapshot = library.snapshot();
    resonance_content::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let mut cache = crate::loading::Cache::default();
    let files = Files::from_snapshot(snapshot, || false, diagnostics.clone())?;
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
        let inputs = Assets::capture(
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
                .collect::<std::result::Result<Vec<_>, _>>()?
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
