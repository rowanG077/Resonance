//! Small end-to-end checks for an installed source or an already cooked library.
//! Source conversion uses private temporary output; cooked-library checks are read-only.
use super::*;
use resonance_content::library::Mappings;
use std::{collections::BTreeMap, fs, path::PathBuf};

fn json<T: serde::de::DeserializeOwned>(root: &Path, path: &str) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(root.join(path))?)?)
}

fn cooked_root() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    ))
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY with a complete Steam cook; read-only"]
fn cooked_fields_prepare_audio_movies_and_event_bindings() -> Result<()> {
    let library = resonance_content::library::Library::open(&cooked_root()?, &[])?;
    for id in [1, 8, 184, 332, 374, 535] {
        let mut snapshot = library.snapshot();
        let (definition, field) =
            resonance_content::field::document::capture_scene(&mut snapshot, id, || false)
                .with_context(|| format!("field {id}"))?;
        ensure!(!field.actors.is_empty(), "field {id} has no actors");
        ensure!(
            !definition.audio.is_empty(),
            "field {id} lost its audio selections"
        );
        definition.audio.resolve(&mut snapshot)?;
        for movie in &definition.movies {
            snapshot.movie(*movie)?;
        }
        if id == 184 {
            ensure!(
                field
                    .parts
                    .iter()
                    .flat_map(|part| &part.clips)
                    .any(|clip| clip.animation_resource.is_some()),
                "Tower of Salvation lost its event-driven scenery motion"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY pointing to the cooked Steam library"]
fn cooked_gameplay_definitions_initialize_and_save_party() -> Result<()> {
    let root = cooked_root()?;
    let library = resonance_content::library::Library::open(&root, &[])?;
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
    let mut snapshot = library.snapshot();
    resonance_content::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let files = resonance_content::prepared::Files::from_snapshot(snapshot, || false, diagnostics)?;
    let session = resonance_content::session::SessionData::load(&files)?;
    assert_eq!(session.monster_count(), 257);
    assert_eq!(session.figurine_count(), 297);
    let mut party = resonance_events::party::Party::new(&session, Default::default())?;
    party.monsters.entry(256).or_default().seen = true;
    party.figurines.insert(296);
    party.validate(&session)?;
    let saved = serde_json::to_vec(&party)?;
    let mut restored: resonance_events::party::Party = serde_json::from_slice(&saved)?;
    restored.validate(&session)?;
    assert!(restored.monsters[&256].seen);
    assert!(restored.figurines.contains(&296));
    restored.monsters.entry(257).or_default();
    assert!(restored.validate(&session).is_err());
    restored.monsters.remove(&257);
    restored.figurines.insert(297);
    assert!(restored.validate(&session).is_err());
    // Execute the ordinary native services against the final catalogue entries.
    // Completion must inspect record 256, rather than wrap the count to a byte.
    for id in 0..256 {
        party.monsters.entry(id).or_default().seen = true;
    }
    party.monsters.remove(&256);
    party.figurines = (0..296).collect();
    use symphonia_script::NativeCall as Call;
    let calls: &[(Call, &[i32])] = &[
        (Call::FieldSystem, &[4, 0]),
        (Call::ConfigureMonsterKnowledge, &[256, 63]),
        (Call::FieldSystem, &[4, 0]),
        (Call::FieldSystem, &[5, 0]),
        (Call::ConfigureFigurine, &[0, 296]),
        (Call::FieldSystem, &[5, 0]),
    ];
    let mut code = Vec::new();
    for (index, (call, args)) in calls.iter().enumerate() {
        for &arg in *args {
            code.extend([
                0x0200,
                arg as u16,
                (arg as u32 >> 16) as u16,
                0x3000,
                0x4000,
            ]);
        }
        code.push(0x2000 | *call as u16);
        code.extend([
            0x3000,
            0x1200,
            0x100 + index as u16 * 4,
            0x1200,
            0x20,
            0x3010,
            0x3000,
        ]);
    }
    code.push(0x20ff);
    let mut words = vec![10, 0, 0, 1, 0, 2, 0, 42, 0, code.len() as u16];
    words.extend(code);
    words.push(0x20ff);
    let program = symphonia_script::Program::decode(
        &words
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    )?;
    let resources = resonance_events::ResourceLibrary {
        session_data: Some(std::sync::Arc::new(session)),
        ..Default::default()
    };
    let mut world = resonance_events::GameWorld::default();
    world.party = Some(party);
    let events = resonance_events::EventRuntime::with_state(
        std::sync::Arc::new(program),
        std::sync::Arc::new(resources),
        world,
        Default::default(),
    )?;
    for (index, expected) in [0, 0, 1, 0, 0, 1].into_iter().enumerate() {
        assert_eq!(
            events
                .memory()
                .read(0x100 + index as u16 * 4, symphonia_script::Width::S32)?,
            expected
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY pointing to the cooked Steam library"]
fn cooked_audio_recordings_load_through_shared_players() -> Result<()> {
    let root = cooked_root()?;
    let root = root.as_path();
    use resonance_content::{
        field_audio::{document, mix::Mix, speech},
        library::Library,
    };
    let library = Library::open(root, &[])?;
    let mut snapshot = library.snapshot();
    let mix = Mix::read(&mut snapshot)?;
    mix.validate()?;
    let music: Mappings =
        snapshot.json(document::MUSIC, resonance_content::preparation::JSON_LIMIT)?;
    assert_eq!(music.iter().count(), 111);
    let mut looping = 0;
    for (id, _) in music.iter() {
        let path = music.resolve(document::MUSIC, id, None)?;
        let recording = resonance_audio::recording::Prepared::new(
            resonance_content::music::capture(&mut snapshot, &path, &|| false)?,
        )?;
        looping += usize::from(recording.repeat().is_some());
        let mut player = recording.start(32000)?;
        for _ in 0..320 {
            assert!(player.next_frame()?.is_some());
        }
    }
    assert!(looping > 0);
    let mut speech_count = 0;
    for language in ["english", "japanese"] {
        let mappings: speech::Mappings = snapshot.json(
            &format!("audio/speech/{language}/mappings.json"),
            resonance_content::preparation::JSON_LIMIT,
        )?;
        assert!(mappings.len() > 10000);
        assert!(mappings.contains_key(&0) && mappings.contains_key(&1946));
        for entry in mappings.values() {
            let source = entry.resolve()?;
            match source {
                speech::Source::Wave(path) => {
                    let recording =
                        resonance_audio::recording::Prepared::new(snapshot.open_stream(&path)?)?;
                    assert!(recording.frames() > 0);
                }
                speech::Source::Silence { silence, .. } => {
                    silence.validate()?;
                }
            }
        }
        speech_count += mappings.len();
    }
    let effects = snapshot.mappings(document::EFFECTS)?;
    let count = effects.iter().count();
    let mut cache = resonance_audio::package::SampleCache::default();
    // Do not retain all decoded effects in Snapshot's byte cache.
    let mut read = |path: &str, limit: usize| -> Result<std::sync::Arc<[u8]>> {
        let bytes = fs::read(root.join(path))?;
        ensure!(bytes.len() <= limit, "effect exceeds shared read budget");
        Ok(bytes.into())
    };
    for (id, _) in effects.iter() {
        let path = effects.resolve(document::EFFECTS, id, None)?;
        resonance_audio::effect::Definition::read(&path, &mut read)?.prepare_in(
            Some(mix.field_reverbs),
            &mut read,
            &mut cache,
        )?;
    }
    let cues: resonance_content::startup::Cues = json(root, "audio/menu.json")?;
    let mut studio = resonance_audio::cue::Studio::new(mix.field_reverbs)?;
    for name in ["navigate", "confirm", "back", "error"] {
        let prepared = resonance_audio::effect::Definition::read(&cues.cues[name], &mut read)?
            .prepare_in(Some(mix.field_reverbs), &mut read, &mut cache)?;
        studio.play(prepared)?;
    }
    let mut audible = false;
    for _ in 0..32000 {
        audible |= studio.next_frame(Err)? != [0; 2];
    }
    assert!(audible);
    eprintln!(
        "Loaded 111 music tracks ({looping} looping), {speech_count} speech bindings and {count} effects; rendered menu cues through the shared mixer"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY pointing to the cooked Steam library"]
fn cooked_party_battle_models_load_through_shared_reader() -> Result<()> {
    let root = cooked_root()?;
    let root = root.as_path();
    use resonance_content::{battle_model::document, diagnostics::Diagnostics, library::Library};
    let library = Library::open(&root, &[])?;
    let diagnostics = Diagnostics::new(true);
    let mut snapshot = library.snapshot();
    let mut count = 0;
    for character in 1..=9u8 {
        for costume in 0..7u8 {
            let (_, model) =
                document::capture_party(&mut snapshot, character, costume, &diagnostics, &|| {
                    false
                })?;
            assert_eq!(model.parts.len(), 2);
            assert!(model.parts[1].outline_color.is_some());
            assert!(
                model.parts[0]
                    .clips
                    .iter()
                    .any(|clip| clip.resource_slot == 0)
            );
            assert!(!model.body.attachments.is_empty());
            assert_eq!(
                model.body.attachments.len(),
                model
                    .body
                    .skeleton
                    .bones
                    .iter()
                    .filter(|bone| bone.name.to_ascii_lowercase().starts_with("kk"))
                    .count(),
            );
            if character == 1 {
                assert!(
                    model.parts[0]
                        .clips
                        .iter()
                        .any(|clip| clip.resource_slot == 100)
                );
            }
            if character == 2 {
                assert!(model.wings.is_some());
            }
            count += model.parts[0].clips.len();
        }
    }
    assert!(!diagnostics.has_errors());
    eprintln!("Loaded 63 battle body selections with {count} bound motions");
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM pointing to an installed Steam copy"]
fn installed_gameplay_menu_tables_prepare_in_all_languages() -> Result<()> {
    use resonance_content::{grade, menu_data, strategy as orders};
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let output = tempfile::tempdir()?;
    resonance_importer::cook_descriptor(output.path())?;
    let source = source::Source::open(&installation, Language::English)?;
    battle_martial::cook(&source, output.path())?;
    let font = dialogue_font::cook(&source, output.path())?;
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
    for language in [
        Language::English,
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
    ] {
        let source = source::Source::open(&installation, language)?;
        crate::items::cook(&source, output.path())?;
        techniques::cook(&source, output.path())?;
        let techniques: resonance_content::arte::document::Document =
            json(output.path(), resonance_content::arte::PATH)?;
        for path in techniques
            .techniques
            .values()
            .filter_map(|entry| entry.rules.action.as_deref())
        {
            let _: resonance_content::battle_martial::AuthoredAction = json(output.path(), path)?;
        }
        strategy::cook(&source, output.path())?;
        menu_catalogues::cook(&source, output.path())?;
        menu_settings::cook(&source, output.path())?;
        menu_synopsis::cook(&source, output.path(), &font)?;
        field_directory::cook(&source, output.path())?;
        world_landmarks::cook(&source, output.path())?;
        menu_world_map::cook(&source, output.path())?;
        menu_presentation::cook(&source, output.path())?;
        let strategy: orders::Document = json(output.path(), orders::PATH)?;
        let rules = strategy.prepare()?;
        rules.validate_rules()?;
        let captions = strategy.captions(&diagnostics)?;
        for (name, members) in captions
            .presets
            .context("missing preset captions")?
            .into_iter()
            .zip(rules.presets)
        {
            menu_data::StrategyPreset {
                name: name.clone(),
                members,
            }
            .validate()
            .with_context(|| format!("{language:?} strategy preset {name:?}"))?;
        }
        assert_eq!(strategy.groups.len(), 3);
        assert_eq!(strategy.presets.len(), 3);
        let shop: grade::Shop = json(output.path(), menu_data::GRADE_SHOP_PATH)?;
        shop.validate()?;
        assert_eq!(shop.options.len(), 26);
        let items: resonance_content::items::Document =
            json(output.path(), resonance_content::items::PATH)?;
        let crafting: menu_data::crafting::document::Document =
            json(output.path(), menu_data::CRAFTING_PATH)?;
        let crafting = crafting.prepare(&items.mappings)?;
        crafting.validate(531)?;
        assert_eq!(crafting.vendors.len(), 22);
        ensure!(!crafting.recipes.is_empty(), "missing crafting recipes");
        let synopsis: menu_data::SynopsisData = json(output.path(), menu_data::SYNOPSIS_PATH)?;
        synopsis.validate()?;
        assert_eq!(synopsis.entries.len(), 200);
        let landmarks: resonance_content::overworld::Landmarks =
            json(output.path(), "world/landmarks.json")?;
        landmarks.validate()?;
        assert_eq!(
            landmarks
                .worlds
                .iter()
                .flatten()
                .filter(|entry| entry.guidepost.is_some())
                .count(),
            12
        );
        let world_map: menu_data::world_map::document::Document =
            json(output.path(), menu_data::WORLD_MAP_PATH)?;
        let fields: Mappings = json(output.path(), resonance_content::library::FIELDS)?;
        let mut world_map = world_map.prepare(&items.mappings, &fields)?;
        let library = resonance_content::library::Library::open(output.path(), &[])?;
        let mut snapshot = library.snapshot();
        snapshot.read(
            "world/landmarks.json",
            resonance_content::preparation::JSON_LIMIT,
        )?;
        let files = resonance_content::prepared::Files::from_snapshot(
            snapshot,
            || false,
            diagnostics.clone(),
        )?;
        world_map.prepare_locations(&files)?;
        world_map.validate(531)?;
        assert_eq!(world_map.shops.len(), 52);
        let presentation: serde_json::Value = json(output.path(), menu_data::PRESENTATION_PATH)?;
        let labels: BTreeMap<String, String> =
            serde_json::from_value(presentation["labels"].clone())?;
        assert_eq!(labels.len(), 101);
        for (key, label) in &labels {
            ensure!(
                !label.is_empty() && label.chars().all(|ch| !ch.is_control() || ch == '\n'),
                "invalid {language:?} shared menu caption {key}: {label:?}"
            );
        }
        let captions: menu_data::ItemCaptions =
            serde_json::from_value(presentation["items"].clone())?;
        assert_eq!(captions.item_categories.len(), 48);
        assert_eq!(captions.inventory_categories.len(), 9);
        assert!(
            synopsis
                .entries
                .iter()
                .any(|entry| !entry.updates.is_empty())
        );
        if language == Language::English {
            assert_eq!(synopsis.entries[0].title, "Light of the Oracle");
            assert_eq!(synopsis.months[0], "Jan");
            let mut unicode = strategy.clone();
            unicode.keyboard["rows"][0] = serde_json::json!("éあCDEFGHIJ");
            let captions = unicode.captions(&diagnostics)?;
            assert_eq!(captions.keyboard()?.chars().count(), 90);
            assert!(captions.keyboard()?.starts_with("éあ"));
            unicode.keyboard["rows"][0] = serde_json::json!("short");
            assert!(unicode.captions(&diagnostics).is_err());
        }
    }
    Ok(())
}
