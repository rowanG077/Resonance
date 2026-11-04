use super::*;
use resonance_content::{arte, battle_rules, items, library::Mappings};
use std::{collections::BTreeMap, fs, path::PathBuf};
use symphonia_constants::{CharacterId, KnownId};

fn json<T: serde::de::DeserializeOwned>(root: &Path, path: &str) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(root.join(path))?)?)
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks enemy media"]
fn installed_enemy_battle_motion_models_use_shared_reader() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    monsters::models(&source, &root)?;
    check_enemy_battle_motions(&source, &root)
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with cooked enemy media"]
fn cooked_enemy_battle_motion_models_use_shared_reader() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    check_enemy_battle_motions(
        &source::Source::open(&installation, Language::English)?,
        &root,
    )
}

fn check_enemy_battle_motions(source: &source::Source, root: &Path) -> Result<()> {
    use resonance_content::model_preview::{ModelPreview, document};
    let image = pe::Image::parse(&source.executable)?;
    let mut motions = 0;
    let mut carried = 0;
    for id in 0..257u16 {
        let filename = image.text(pe::word(image.slice(0x8e5b50 + u32::from(id) * 4, 4)?, 0)?)?;
        let bytes = source.require(filename)?;
        let path = format!("meshes/monsters/{}/model.json", monsters::symbol(id));
        let mut preview: ModelPreview = json(root, &path)?;
        preview.validate_definition()?;
        ensure!(
            preview.parts[0]
                .scene
                .materials
                .iter()
                .all(|material| material.depth_write),
            "enemy {id} body must occlude its backface outline, including blended feather/wing edges"
        );
        let count = (pe::word(&bytes, 0x14)? as usize).max(31);
        let expected = (0..count)
            .filter_map(|slot| {
                preview_model::reference(&bytes, 0x20 + slot * 4)
                    .transpose()
                    .map(|result| result.map(|_| slot as u16))
            })
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        let actual = preview.parts[0]
            .scene
            .clips
            .iter()
            .map(|clip| clip.resource_slot)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            actual, expected,
            "enemy {id} body motion declarations differ"
        );
        assert_eq!(
            preview.parts[1]
                .scene
                .clips
                .iter()
                .map(|clip| clip.resource_slot)
                .collect::<std::collections::BTreeSet<_>>(),
            expected,
            "enemy {id} outline motion declarations differ"
        );
        assert_eq!(
            preview.parts[0].animation,
            expected.contains(&0).then_some(0),
            "enemy {id} initial pose differs"
        );
        motions += expected.len();
        carried += preview
            .parts
            .iter()
            .skip(2)
            .filter(|part| {
                part.attached_to
                    .as_ref()
                    .is_some_and(|bone| bone.to_ascii_lowercase().starts_with("kk"))
                    && !part.scene.clips.is_empty()
            })
            .count();
        // Exercise initial enemies, carried/secondary bodies, a cameo rig and the
        // added final boss through the same reader used by the model viewer.
        if [0, 58, 105, 122, 202, 205, 206, 207, 211, 219, 242, 256].contains(&id) {
            document::prepare(&mut preview, |path| Ok(fs::read(root.join(path))?.into()))?;
            for part in &preview.parts {
                for clip in &part.scene.clips {
                    assert!(clip.duration_seconds.is_finite());
                }
            }
        }
    }
    assert!(motions > 257);
    assert!(carried > 0);
    eprintln!(
        "257 enemy bodies retain {motions} native motion slots; {carried} carried layers have authored clips; representative shared-reader preparation passed"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with cooked party/common battle rules"]
fn installed_unison_completes_shared_battle_rule_capture() -> Result<()> {
    use resonance_content::{battle_voice::Sound, diagnostics::Diagnostics, library::Library};
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    battle_unison::cook(&source, &root)?;
    let library = Library::open_paths(&root, &[])?;
    let mut snapshot = library.snapshot();
    let captured = battle_rules::capture(&mut snapshot, &|| false)?;
    let unison = captured.unison;
    assert_eq!(unison.combinations.len(), 29);
    assert_eq!(
        unison
            .combinations
            .iter()
            .map(|row| row.native_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        26
    );
    for id in [303, 319, 322] {
        assert_eq!(
            unison
                .combinations
                .iter()
                .filter(|row| row.native_id == id)
                .count(),
            2
        );
    }
    assert_eq!(unison.placements.len(), 254);
    assert!(unison.placements.contains_key(&260) && unison.placements.contains_key(&261));
    assert!(!unison.placements.contains_key(&253));
    assert_eq!(unison.openers.len(), 9);
    assert_eq!(unison.openers[0].duration, 30);
    assert_eq!(unison.openers[0].recovery, 14);
    assert_eq!(unison.openers[2].contact_ticks, 45);
    assert!(unison.host.prelude.first_x < 0.);
    let scenes: Mappings = json(&root, resonance_content::battle_scene::document::TECHNIQUES)?;
    for row in &unison.combinations {
        scenes.target(u32::from(row.native_id))?;
    }
    for language in ["english", "japanese"] {
        let voices: resonance_content::field_audio::speech::Mappings =
            json(&root, &format!("audio/speech/{language}/mappings.json"))?;
        for sound in unison.voices.into_iter().flatten() {
            let Sound::Stream(id) = sound else {
                anyhow::bail!("Unison voice was not normalized")
            };
            let path = voices
                .get(&u32::from(id))
                .context("missing Unison voice")?
                .resolve()?;
            let path = path.path().context("Unison voice is unexpectedly silent")?;
            assert!(
                resonance_audio::recording::Prepared::new(fs::File::open(root.join(path))?)?
                    .frames()
                    > 0
            );
        }
    }
    let catalogue =
        resonance_content::battle_audio::document::Catalogue::capture(&mut snapshot, &|| false)?;
    for opener in &unison.openers {
        for (_, event) in &opener.events {
            if let resonance_content::battle_normal::Event::Contact(contact) = event {
                if let Some(sound) = contact.hit.sound {
                    catalogue.collect(
                        &mut Default::default(),
                        &Diagnostics::new(true),
                        Sound::Cue(sound),
                    )?;
                }
            }
        }
    }
    eprintln!(
        "All shared battle rule owners load: 29 Unison recipes, nine openings, 254 native placements and {} physical scene mappings; scene artwork is still pending",
        scenes.iter().count()
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with cooked audio and formations"]
fn installed_common_battle_resources_use_shared_readers_and_player() -> Result<()> {
    use resonance_content::{
        battle_audio::{Selection, document},
        battle_voice::Sound,
        diagnostics::Diagnostics,
        library::Library,
        prepared::Files,
    };
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    battle_projectiles::cook(&source, &root)?;
    battle_defense::cook(&source, &root)?;
    battle_audio::cook(&source, &root)?;
    let library = Library::open_paths(&root, &[])?;
    let mut snapshot = library.snapshot();
    for path in [
        resonance_content::battle_projectile::PATH,
        resonance_content::battle_defense_system::PATH,
        resonance_content::battle_enemy::document::MAPPINGS,
    ] {
        snapshot.read(path, resonance_content::preparation::JSON_LIMIT)?;
    }
    let files = Files::from_snapshot(snapshot, || false, Diagnostics::new(true))?;
    let projectiles = battle_rules::projectiles(&files)?;
    assert_eq!(projectiles.records.len(), 26);
    assert!(
        projectiles
            .records
            .iter()
            .all(|row| row.unsupported_reason.is_none())
    );
    assert_eq!(projectiles.resident_spells.len(), 6);
    assert_eq!(projectiles.support_durations.len(), 8);
    assert_eq!(projectiles.records[9].velocity, [0., 8., 16.]);
    assert_eq!(projectiles.records[9].spawn_offset, [0., 32., 90.]);
    assert!(projectiles.resident_spells[&220].followup_hit.is_some());
    for spell in projectiles.resident_spells.values() {
        assert!(spell.duration > 0);
    }
    let fire = projectiles.fire_ball.unwrap();
    assert!(fire.velocities.into_iter().flatten().all(f32::is_finite));
    let defense = battle_rules::defense(&files)?;
    assert_eq!(defense.repair_cycle.self_ticks, 1500);
    assert_eq!(defense.deployment.ticks, 120);
    assert_eq!(defense.orbit_count, 2);
    assert!(!defense.messages.automatic.is_empty());
    let mut snapshot = library.snapshot();
    let catalogue = document::Catalogue::capture(&mut snapshot, &|| false)?;
    assert_eq!(
        catalogue.definition.effect_spatial,
        [320., 5., 64., 0., 127.]
    );
    assert_eq!(
        catalogue.definition.voice_spatial,
        [320., 10., 64., 24., 104.]
    );
    assert!(catalogue.definition.speech_rates.is_empty());
    let diagnostics = Diagnostics::new(true);
    let mut selection = Selection::default();
    for (cue, native) in battle_audio::CUES {
        let native = audio_effects::id(native)?;
        assert_eq!(
            catalogue.resolved.effects[&(cue as i16)],
            catalogue.resolved.effects[&(native as i16)]
        );
        catalogue.collect(&mut selection, &diagnostics, Sound::Cue(cue))?;
    }
    for cue in [3, 0x8003] {
        catalogue.collect(&mut selection, &diagnostics, Sound::Stream(cue))?;
    }
    let selected = catalogue.select(&selection, &diagnostics)?;
    assert_eq!(selected.effects.len(), 20);
    assert_eq!(selected.voices.len(), 2);
    // Existing menu/field service meanings also remain valid in this catalogue.
    for (cue, native) in [(1, 28), (2, 27), (3, 26), (4, 7), (38, 31)] {
        assert_eq!(
            catalogue.resolved.effects[&cue],
            catalogue.resolved.effects[&(audio_effects::id(native)? as i16)]
        );
    }
    let mix = resonance_content::field_audio::mix::Mix::read(&mut snapshot)?;
    let mut read = |path: &str, _: usize| -> Result<std::sync::Arc<[u8]>> {
        Ok(fs::read(root.join(path))?.into())
    };
    let mut cache = resonance_audio::package::SampleCache::default();
    let mut studio = resonance_audio::cue::Studio::new(mix.battle_reverbs)?;
    for path in selected.effects.values() {
        let effect = resonance_audio::effect::Definition::read(path, &mut read)?.prepare_in(
            Some(mix.battle_reverbs),
            &mut read,
            &mut cache,
        )?;
        studio.play(effect)?;
    }
    let mut audible = false;
    for _ in 0..3200 {
        audible |= studio.next_frame(Err)? != [0; 2];
    }
    assert!(audible);
    eprintln!(
        "26 projectiles, six resident spells, eight support clocks, named Defense System and 20 common battle recordings loaded through shared readers/player"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with cooked audio and party attacks"]
fn installed_party_rules_use_shared_encounter_reader() -> Result<()> {
    use resonance_content::{
        battle_voice::Sound, diagnostics::Diagnostics, library::Library, prepared::Files,
    };
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    battle_party_rules::cook(&source, &root)?;
    let library = Library::open_paths(&root, &[])?;
    let mut snapshot = library.snapshot();
    for path in [
        battle_rules::PARTY,
        resonance_content::battle_model::document::PARTY,
        battle_rules::NORMAL_ATTACKS,
        battle_rules::MARTIAL_ATTACKS,
    ] {
        snapshot.read(path, resonance_content::preparation::JSON_LIMIT)?;
    }
    let files = Files::from_snapshot(snapshot, || false, Diagnostics::new(true))?;
    let table = battle_rules::party(&files)?;
    assert_eq!(table.records.len(), 9);
    let mut models = library.snapshot();
    for (index, profile) in table.records.iter().enumerate() {
        let (_, model) = resonance_content::battle_model::document::capture_party(
            &mut models,
            index as u8 + 1,
            0,
            &Diagnostics::new(true),
            &|| false,
        )?;
        assert!(
            model
                .body
                .skeleton
                .bone(&profile.head_anchor.bone)
                .is_some(),
            "missing party head anchor {}",
            index + 1
        );
        assert!(
            model.body.skeleton.bone(&profile.overlimit_bone).is_some(),
            "missing party Over Limit anchor {}",
            index + 1
        );
    }
    assert_eq!(table.normal_attacks.len(), 9);
    assert_eq!(table.default_strategy[8], [8, 6, 2]);
    assert_eq!(table.companion_policy[8].buff_max_level_advantage, 8);
    assert_eq!(table.companion_positioning.approach_timeout_ticks, 300);
    assert_eq!(table.contact_sounds.elements[0], 0x316);
    assert_eq!(table.item_use_offsets[0], [-10., 100., 10.]);
    assert_eq!(table.chant_sequences[&3].len(), 3);
    assert_eq!(table.records[0].run_speed, 10.);
    let voices = |index: usize| table.records[index].voices.as_ref().unwrap();
    assert_eq!(voices(0).hurt[0], Some(Sound::Stream(0x8003)));
    assert_eq!(voices(0).defeat, Some(Sound::Stream(2)));
    assert_eq!(voices(6).charge, Some(Sound::Stream(586)));
    assert_eq!(voices(6).charge_failed, Some(Sound::Stream(34326)));
    assert_eq!(voices(1).stunned, Some(Sound::Stream(33046)));
    assert_eq!(voices(3).item_discovery, Some(Sound::Stream(33546)));
    assert_eq!(table.overlimit_voices[0], Some(Sound::Stream(0x4f6)));
    assert!(table.overlimit_voices.iter().all(Option::is_some));
    let policies = table
        .voice_sequences
        .iter()
        .map(|policy| policy.techniques.len())
        .sum::<usize>();
    assert!(policies > 100);
    let document: resonance_content::battle_rules::Party = json(&root, battle_rules::PARTY)?;
    fn streams(value: &serde_json::Value, output: &mut std::collections::BTreeSet<u16>) {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(id) = fields.get("stream").and_then(serde_json::Value::as_u64) {
                    output.insert(id.try_into().unwrap());
                }
                for value in fields.values() {
                    streams(value, output);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    streams(value, output);
                }
            }
            _ => (),
        }
    }
    let mut lines = std::collections::BTreeSet::new();
    streams(&serde_json::to_value(&document)?, &mut lines);
    for language in ["english", "japanese"] {
        let voices: resonance_content::field_audio::speech::Mappings =
            json(&root, &format!("audio/speech/{language}/mappings.json"))?;
        for &id in &lines {
            let recording = voices
                .get(&u32::from(id))
                .with_context(|| format!("missing {language} party rule voice {id}"))?
                .resolve()?;
            let path = recording.path().context("party rule voice is silent")?;
            let wave = resonance_audio::recording::Prepared::new(fs::File::open(root.join(path))?)?;
            assert!(wave.frames() > 0);
        }
    }
    eprintln!(
        "Nine shared profiles, {policies} casting overrides, {} referenced voice recordings in each language",
        lines.len()
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optionally writes RESONANCE_TEST_LIBRARY"]
fn installed_martial_actions_use_shared_decoder_and_documents() -> Result<()> {
    use resonance_content::{
        battle_martial::document::Catalogue, battle_normal::Event, battle_voice::Sound,
    };
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let catalogue = battle_martial::read(&source)?;
    let document = resonance_importer::martial_attack_document(&catalogue)?;
    let encoded = resonance_asset_writer::json::encode(&document)?;
    let prepared = serde_json::from_slice::<Catalogue>(&encoded)?.prepare()?;
    assert_eq!(
        serde_json::to_value(&catalogue)?,
        serde_json::to_value(&prepared)?
    );
    assert_eq!(prepared.members.len(), 125);
    for (technique, character) in [
        (1, 1),
        (2, 1),
        (4, 1),
        (40, 2),
        (41, 2),
        (42, 2),
        (45, 2),
        (223, 2),
        (224, 2),
        (125, 5),
        (207, 6),
        (207, 9),
        (154, 7),
        (176, 8),
        (196, 8),
        (198, 8),
        (199, 8),
        (30, 1),
    ] {
        let action = prepared
            .attack(technique, character)
            .with_context(|| format!("technique {technique}, character {character}"))?;
        assert!(action.duration > 0);
        assert!(!action.events.is_empty());
    }
    assert!(prepared.attack(45,2)?.events.iter().any(|(_, event)| matches!(event,
        Event::VoiceChoice(choice) if choice.first_chance == 75 && choice.sounds == [Some(Sound::Stream(144)), Some(Sound::Stream(179))])));
    assert_eq!(prepared.bindings[&260][&7].member, 146);
    assert_eq!(prepared.bindings[&261][&7].member, 147);
    assert!(!prepared.bindings.contains_key(&253));
    let temporary = tempfile::tempdir()?;
    let root = std::env::var_os("RESONANCE_TEST_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_owned());
    battle_martial::cook(&source, &root)?;
    let written: Catalogue = json(&root, battle_rules::MARTIAL_ATTACKS)?;
    assert_eq!(
        serde_json::to_value(written)?,
        serde_json::to_value(document)?
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with cooked stream audio"]
fn installed_party_speech_covers_attack_voices() -> Result<()> {
    use resonance_content::{
        battle_normal::{Event, document::NormalAttacks},
        battle_voice::Sound,
        field_audio::speech,
    };
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    battle_normal::cook(&source, &root)?;
    battle_voices::cook(&source, &root)?;
    battle_martial::cook(&source, &root)?;
    let attacks =
        json::<NormalAttacks>(&root, resonance_content::battle_rules::NORMAL_ATTACKS)?.prepare()?;
    let martial = json::<resonance_content::battle_martial::document::Catalogue>(
        &root,
        resonance_content::battle_rules::MARTIAL_ATTACKS,
    )?
    .prepare()?;
    let mut events: Vec<_> = attacks
        .iter()
        .flatten()
        .flat_map(|attack| attack.events.iter().map(|(_, event)| event))
        .collect();
    for member in martial.members.values() {
        for action in member.variants.values() {
            let resonance_content::battle_scene::SceneAction::Prepared(attack) = action else {
                continue;
            };
            events.extend(attack.events.iter().map(|(_, event)| event));
            if let Some(hammer) = &attack.hammer {
                if let resonance_content::battle_martial::HammerPattern::Single {
                    alternate_events,
                } = &hammer.pattern
                {
                    events.extend(alternate_events.iter().map(|(_, event)| event));
                }
            }
            if let Some(steal) = &attack.steal {
                events.extend(steal.success_events.iter().map(|(_, event)| event));
            }
        }
    }
    for language in ["english", "japanese"] {
        let voices: speech::Mappings =
            json(&root, &format!("audio/speech/{language}/mappings.json"))?;
        let mut count = 0;
        for (&id, voice) in voices.range(0x8000..0x8900) {
            let path = voice
                .resolve()?
                .path()
                .context("actor voice unexpectedly silent")?
                .to_owned();
            let recording =
                resonance_audio::recording::Prepared::new(fs::File::open(root.join(path))?)?;
            assert!(recording.frames() > 0, "empty actor voice {id}");
            count += 1;
        }
        assert!(count > 100);
        for event in &events {
            let lines = match event {
                Event::Voice(sound) => [*sound, None],
                Event::VoiceChoice(choice) => choice.sounds,
                _ => continue,
            };
            for line in lines.into_iter().flatten() {
                let Sound::Stream(id) = line else {
                    anyhow::bail!("party speech was not normalized");
                };
                assert!(
                    voices.contains_key(&u32::from(id)),
                    "missing {language} attack voice {id}"
                );
            }
        }
        eprintln!("Loaded {count} {language} party voice recordings");
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optionally writes RESONANCE_TEST_LIBRARY"]
fn installed_normal_attacks_use_shared_decoder_and_documents() -> Result<()> {
    use resonance_content::{
        battle_normal::{Event, document::NormalAttacks},
        battle_rules,
    };
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let temporary = tempfile::tempdir()?;
    let root = std::env::var_os("RESONANCE_TEST_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_owned());
    battle_normal::cook(
        &source::Source::open(&installation, Language::English)?,
        &root,
    )?;
    let document: NormalAttacks = json(&root, battle_rules::NORMAL_ATTACKS)?;
    let attacks = document.prepare()?;
    assert_eq!(attacks.len(), 9);
    let mut contacts = 0;
    for attack in attacks.iter().flatten() {
        assert!(attack.duration > 0);
        for (_, event) in &attack.events {
            match event {
                Event::Contact(contact) => {
                    assert!(contact.duration > 0);
                    contacts += 1;
                }
                _ => (),
            }
        }
    }
    assert!(contacts > 60);
    eprintln!("63 normal attacks, {contacts} contacts");
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with cooked audio and fields"]
fn installed_field_audio_uses_shared_selections() -> Result<()> {
    use resonance_content::{field::document::Definition, field_audio::ServiceCue};
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    // Recook scenarios from source so a focused rerun never applies source ID
    // conversion to an already converted program. Geometry/audio are retained.
    if std::env::var_os("RESONANCE_TEST_REUSE_SCENARIOS").is_none() {
        field_core::cook(&source, &root, None)?;
    }
    audio_fields::cook(&source, &root)?;
    let library = resonance_content::library::Library::open(&root, &[])?;
    let fields: Mappings = json(&root, resonance_content::library::FIELDS)?;
    let (mut music, mut voices, mut effects) = (0, 0, 0);
    for (id, name) in fields.iter() {
        let field: Definition = json(&root, &format!("fields/{name}/field.json"))?;
        let audio = field
            .audio
            .resolve(&mut library.snapshot())
            .with_context(|| format!("field {id} audio"))?;
        assert!(
            ServiceCue::ALL
                .iter()
                .all(|cue| audio.effects.contains_key(&(*cue as i16)))
        );
        music += usize::from(!audio.music.is_empty());
        voices += usize::from(!audio.voices.is_empty());
        effects += audio.effects.len();
    }
    assert!(music > 100 && voices > 100);
    eprintln!(
        "Field audio: {music} music owners, {voices} speech owners, {effects} effect selections"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY with cooked movie 1"]
fn cooked_movie_plays_to_completion() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let library = resonance_content::library::Library::open(&root, &[])?;
    let movie = resonance_media::MovieDecoder::load(&mut library.snapshot(), 1)?;
    let began = std::time::Instant::now();
    let (mut video_frames, mut audio_samples) = (0, 0);
    loop {
        ensure!(began.elapsed().as_secs() < 120, "movie playback stalled");
        match movie.try_next()? {
            Some(resonance_media::MovieEvent::Video(frame)) => {
                assert_eq!(frame.index, video_frames);
                video_frames += 1;
            }
            Some(resonance_media::MovieEvent::Audio(chunk)) => audio_samples += chunk.samples.len(),
            Some(resonance_media::MovieEvent::End) => break,
            None => std::thread::sleep(std::time::Duration::from_millis(1)),
        }
    }
    assert_eq!(video_frames, 1918);
    assert!(audio_samples > 48_000 * 2 * 60);
    eprintln!("Played {video_frames} video frames and {audio_samples} audio samples");
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with cooked fields"]
fn installed_field_services_use_shared_models() -> Result<()> {
    use resonance_content::field::{
        SAVE_POINT_RESOURCE, TREASURE_RESOURCE_BASE,
        document::{self, ModelMappings},
    };
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    field_services::cook(&source, &root)?;
    let mappings: ModelMappings = json(&root, document::MODELS)?;
    for id in [
        SAVE_POINT_RESOURCE,
        TREASURE_RESOURCE_BASE,
        TREASURE_RESOURCE_BASE + 1,
        TREASURE_RESOURCE_BASE + 2,
    ] {
        let model: document::Model = json(&root, &mappings.model_path(id)?)?;
        model.validate()?;
        assert!(
            model
                .parts
                .iter()
                .any(|part| part.clips.iter().any(|clip| clip.resource_slot == 12))
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with cooked fields"]
fn installed_movies_use_shared_player() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let selected = std::env::var("RESONANCE_TEST_MOVIE")
        .ok()
        .map(|id| id.parse::<u32>())
        .transpose()?;
    let source = source::Source::open(&installation, Language::English)?;
    movies::cook(&source, &root, selected)?;
    let library = resonance_content::library::Library::open(&root, &[])?;
    let mut snapshot = library.snapshot();
    let movie = resonance_media::MovieDecoder::load(&mut snapshot, selected.unwrap_or(1))?;
    assert_eq!(movie.format().width, 640);
    assert_eq!(movie.format().sample_rate, 48_000);
    assert!(movie.format().audio_frames.is_none_or(|frames| frames > 0));
    let times = movie.video_timestamps()?;
    assert!(times.len() > 100);
    let field_names: Mappings = json(&root, resonance_content::library::FIELDS)?;
    let mut bindings = 0;
    for (_, name) in field_names.iter() {
        let field: resonance_content::field::document::Definition =
            json(&root, &format!("fields/{name}/field.json"))?;
        bindings += usize::from(field.movies.contains(&selected.unwrap_or(1)));
    }
    assert!(bindings > 0, "cooked movie has no field bindings");
    eprintln!("Movie: {} frames, {bindings} field bindings", times.len());
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with cooked Steam audio"]
fn installed_skits_use_shared_catalogue_and_vm() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    skits::cook(&source, &root)?;
    skit_portraits::cook(&source, &root)?;
    let library = resonance_content::library::Library::open(&root, &[])?;
    let mut snapshot = library.snapshot();
    let catalogue = std::sync::Arc::new(resonance_content::skit::document::capture(
        &mut snapshot,
        &|| false,
    )?);
    catalogue.validate()?;
    assert!(catalogue.resources.len() > 400 && catalogue.skits.len() > 300);
    assert_eq!(catalogue.portrait_recipes.len(), 230);
    assert!(
        !catalogue.media.is_empty() && catalogue.media.values().all(|media| media.voice.is_none())
    );
    for resource in catalogue.resources.values() {
        symphonia_script::Program::decode(&fs::read(root.join(&resource.script))?)?;
    }
    let resource = &catalogue.resources[&1];
    let program = symphonia_script::Program::decode(&fs::read(root.join(&resource.script))?)?;
    let mut world = resonance_events::GameWorld::default();
    world.skit = Some(Default::default());
    let resources = resonance_events::ResourceLibrary {
        skits: Some(catalogue.clone()),
        messages: resource
            .messages
            .as_deref()
            .map(|path| json(&root, path))
            .transpose()?
            .unwrap_or_default(),
        actor_names: resonance_events::ResourceLibrary::character_names(),
        ..Default::default()
    };
    let mut events = resonance_events::EventRuntime::with_state(
        std::sync::Arc::new(program),
        std::sync::Arc::new(resources),
        world,
        Default::default(),
    )?;
    for _ in 0..120 {
        events.step()?;
    }
    eprintln!(
        "Loaded {} skit scripts, {} availability rules, {} portraits, {} timed media; advanced native skit 1 through the shared VM",
        catalogue.resources.len(),
        catalogue.skits.len(),
        catalogue.portraits.len(),
        catalogue.media.len()
    );
    Ok(())
}

#[test]
#[ignore = "requires installed Steam and a cooked library; converts source skit bytecode privately without recooking portrait media"]
fn installed_skit_sources_preserve_the_shared_vm_contract() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let baseline = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let temporary = tempfile::tempdir()?;
    let source = source::Source::open(&installation, Language::English)?;
    for name in [
        resonance_content::library::FIELDS,
        "audio/speech/english/mappings.json",
    ] {
        output::write(
            &temporary.path().join(name),
            &fs::read(baseline.join(name))?,
        )?;
    }
    skits::cook(&source, temporary.path())?;
    let mappings: Mappings = json(
        temporary.path(),
        resonance_content::skit::document::MAPPINGS,
    )?;
    let mut converted = 0;
    for (_, name) in mappings.iter() {
        let bytes = fs::read(
            temporary
                .path()
                .join("skits")
                .join(name)
                .join("scenario.ssb"),
        )?;
        symphonia_script::Program::decode(&bytes)?;
        converted += 1;
    }
    ensure!(
        converted > 400,
        "source skit catalogue unexpectedly incomplete"
    );
    eprintln!("Converted {converted} source skit programs through ordinary shared bytecode");
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; recooks scenery and local overlays"]
fn installed_field_overlays_load_through_shared_catalogue() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    field_scene::cook(&source, &root, None)?;
    let fields: Mappings = json(&root, resonance_content::library::FIELDS)?;
    let mut count = 0;
    for (_, name) in fields.iter() {
        let scene: resonance_content::field::document::Scene =
            json(&root, &format!("fields/{name}/scene.json"))?;
        for path in scene.overlays.values() {
            let art = resonance_content::effect::OverlayArt::load(path, |path| {
                Ok(fs::read(root.join(path))?)
            })?;
            for path in art.references() {
                resonance_asset_writer::texture::ktx2(&fs::read(root.join(path))?)?;
            }
            count += 1;
        }
    }
    assert!(count > 20);
    eprintln!("Loaded {count} local overlay banks through the shared catalogue");
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optionally writes audio to RESONANCE_TEST_AUDIO_OUTPUT"]
fn installed_audio_recordings_load_through_shared_players() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let temporary = tempfile::tempdir()?;
    let root = std::env::var_os("RESONANCE_TEST_AUDIO_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_owned());
    let source = source::Source::open(&installation, Language::English)?;
    resonance_importer::cook_descriptor(&root)?;
    let streams = audio_streams::cook(&source, &root)?;
    audio_effects::cook(&source, &root, streams)?;
    audio_settings::cook(&root)?;
    check_audio_recordings(&root)
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY pointing to the cooked Steam library"]
fn cooked_audio_recordings_load_through_shared_players() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    check_audio_recordings(&root)
}

fn check_audio_recordings(root: &Path) -> Result<()> {
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
                    assert_eq!(language, "english");
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
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with cooked Steam field models"]
fn installed_party_battle_models_load_through_shared_reader() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    battle_party_mappings::cook(&source, &root)?;
    battle_party_model::cook(&source, &root)?;
    check_party_battle_models(&root)
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY pointing to the cooked Steam library"]
fn cooked_party_battle_models_load_through_shared_reader() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    check_party_battle_models(&root)
}

fn check_party_battle_models(root: &Path) -> Result<()> {
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
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY pointing to the cooked Steam library"]
fn installed_localized_gameplay_loads_with_utf8_names() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let overlay = tempfile::tempdir()?;
    let source = source::Source::open(&installation, Language::French)?;
    cook_tables(&source, overlay.path())?;
    fs::remove_file(overlay.path().join(resonance_content::library::DESCRIPTOR))?;
    menu_settings::cook(&source, overlay.path())?;
    menu_catalogues::cook(&source, overlay.path())?;
    menu_presentation::cook(&source, overlay.path())?;
    menu_world_map::cook(&source, overlay.path())?;
    world_landmarks::cook(&source, overlay.path())?;
    strategy::cook(&source, overlay.path())?;
    let font = json(&root, "fonts/dialogue.json")?;
    menu_synopsis::cook(&source, overlay.path(), &font)?;
    monsters::catalogue(&source, overlay.path())?;
    figurines::catalogue(&source, overlay.path())?;
    let library =
        resonance_content::library::Library::open_paths(&root, &[overlay.path().to_owned()])?;
    let mut snapshot = library.snapshot();
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
    resonance_content::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let files = resonance_content::prepared::Files::from_snapshot(snapshot, || false, diagnostics)?;
    let session = resonance_content::session::SessionData::load(&files)?;
    let names = &session.rules.as_ref().unwrap().initial_names;
    assert_eq!(names[2], "Génis");
    assert_eq!(names[6], "Préséa");
    resonance_events::party::Party::new(&session, Default::default())?.validate(&session)?;
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optionally writes mappings to RESONANCE_TEST_CATALOGUE_OUTPUT"]
fn installed_battle_party_mappings_cover_native_costumes() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let temporary = tempfile::tempdir()?;
    let output = std::env::var_os("RESONANCE_TEST_CATALOGUE_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_owned());
    let source = source::Source::open(&installation, Language::English)?;
    battle_party_mappings::cook(&source, &output)?;
    let characters: Mappings = json(&output, "battle/party/mappings.json")?;
    assert_eq!(characters.iter().count(), 9);
    for (_, name) in characters.iter() {
        let costumes: Mappings = json(&output, &format!("battle/party/{name}/mappings.json"))?;
        assert_eq!(costumes.iter().count(), 7);
        assert_eq!(costumes.target(5)?, "costume-5");
        assert_eq!(costumes.target(6)?, "costume-6");
        if name != "colette" && name != "presea" {
            assert_eq!(costumes.target(3)?, "default");
        }
        if name == "kratos" {
            assert_eq!(costumes.target(4)?, "default");
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY pointing to the cooked Steam library"]
fn cooked_gameplay_definitions_initialize_and_save_party() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
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
#[ignore = "requires RESONANCE_TEST_STEAM; optionally writes catalogue owners to RESONANCE_TEST_CATALOGUE_OUTPUT"]
fn installed_catalogues_and_models_load_through_shared_readers() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let temporary = tempfile::tempdir()?;
    let output = std::env::var_os("RESONANCE_TEST_CATALOGUE_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_owned());
    let source = source::Source::open(&installation, Language::English)?;
    battle_party_mappings::cook(&source, &output)?;
    monsters::models(&source, &output)?;
    figurines::models(&source, &output)?;
    for language in [
        Language::English,
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
    ] {
        let source = source::Source::open(&installation, language)?;
        monsters::catalogue(&source, &output)?;
        figurines::catalogue(&source, &output)?;
        for (path, count) in [
            (resonance_content::menu_data::MONSTER_BOOK_PATH, 257),
            (resonance_content::menu_data::FIGURINES_PATH, 297),
        ] {
            let value: serde_json::Value = json(&output, path)?;
            let (expanded, _) =
                resonance_content::model_preview::document::expand_catalogue(value, |path| {
                    Ok(fs::read(output.join(path))?.into())
                })?;
            if count == 257 {
                let book: resonance_content::monster::MonsterBook =
                    serde_json::from_value(expanded)?;
                book.validate(531)?;
                assert_eq!(book.records.len(), count);
                if language == Language::English {
                    assert_eq!(book.records[256].name, "Mithos the Hero");
                }
            } else {
                let book: resonance_content::figurine::FigurineBook =
                    serde_json::from_value(expanded)?;
                book.validate()?;
                assert_eq!(book.records.len(), count);
            }
        }
    }
    // Leave an explicitly requested library in its original English language.
    monsters::catalogue(&source, &output)?;
    figurines::catalogue(&source, &output)?;
    for path in [
        "meshes/monsters/model-0256/model.json".to_owned(),
        format!(
            "meshes/figurines/{}/model.json",
            symphonia_constants::FigurineId::try_from(287_u16)?
                .resource_name()
                .unwrap()
        ),
    ] {
        let mut model: resonance_content::model_preview::ModelPreview = json(&output, &path)?;
        resonance_content::model_preview::document::prepare(&mut model, |path| {
            Ok(fs::read(output.join(path))?.into())
        })?;
    }
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
        strategy::cook(&source, output.path())?;
        menu_catalogues::cook(&source, output.path())?;
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

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM pointing to an installed Steam copy"]
fn installed_startup_and_dialogue_load_through_shared_readers() -> Result<()> {
    use resonance_content::{font, library::Library, startup as title};
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let output = tempfile::tempdir()?;
    resonance_importer::cook_descriptor(output.path())?;
    let source = source::Source::open(&installation, Language::English)?;
    let font = dialogue_font::cook(&source, output.path())?;
    let decoded: font::BitmapFont = json(output.path(), "fonts/dialogue.json")?;
    decoded.validate()?;
    decoded.validate_text("New Game — Génis, Colette & Préséa · あいうえお ｱｲｳｴｵ")?;
    assert_eq!(decoded.glyphs, font.glyphs);
    let pixels =
        resonance_asset_writer::texture::ktx2(&fs::read(output.path().join(&font.texture))?)?;
    assert_eq!(
        [pixels[0].width, pixels[0].height],
        [font.width, font.height]
    );
    startup::cook(&source, output.path())?;
    let library = Library::open(output.path(), &[])?;
    let (definition, boot) = title::capture(&mut library.snapshot(), true, &|| false)?;
    assert_eq!((definition.day_field, definition.night_field), (474, 531));
    assert_eq!(definition.artwork.len(), 5);
    let boot = boot.context("missing boot sequence")?;
    assert_eq!(boot.0.len(), 3);
    for path in definition
        .artwork
        .iter()
        .flat_map(title::Artwork::images)
        .chain(boot.0.iter().map(|logo| logo.image.as_str()))
    {
        resonance_asset_writer::texture::ktx2(&fs::read(output.path().join(path))?)?;
    }
    cook_tables(&source, output.path())?;
    dialogue::cook(&source, output.path(), &font)?;
    menu_art::cook(&source, output.path())?;
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
    let mut menu = resonance_content::menu::MenuArt::decode(
        &fs::read(output.path().join("ui/menu.json"))?,
        &diagnostics,
    )?;
    menu.prepare_resources(|path| fs::read(output.path().join(path)).map_err(anyhow::Error::from));
    menu.validate_layout(531)?;
    assert_eq!(
        menu.sprites
            .group(resonance_content::menu::Sprite::ItemImages)?
            .len(),
        531
    );
    assert_eq!(menu.labels["talk"], "Talk");
    resonance_content::preparation::ui(&mut library.snapshot(), &diagnostics, &|| false)?;
    for language in [
        Language::English,
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
    ] {
        let source = source::Source::open(&installation, language)?;
        dialogue::cook(&source, output.path(), &font)?;
        let mut art: font::DialogueArt = json(output.path(), "ui/dialogue.json")?;
        art.prepare_images(|path| fs::read(output.path().join(path)).map_err(anyhow::Error::from))?;
        let track: font::MovieSubtitles = json(output.path(), "ui/story-subtitles.json")?;
        track.validate()?;
        assert_eq!(track.cues.len(), 15);
        for line in track.cues.iter().flat_map(|cue| &cue.lines) {
            font.validate_text(&line.text)?;
        }
        let bytes = source.require("TOS_TEXT.TXB")?;
        let labels = menu_labels::read(
            &source,
            &pe::Image::parse(&source.executable)?,
            &text::Bank::parse(&bytes)?,
            language.slot(),
        )?;
        for key in resonance_content::menu::SHOP_LABELS {
            ensure!(labels.contains_key(key), "missing shop caption {key}");
        }
        ensure!(
            !labels["confirm_save_a"].contains('%'),
            "unresolved save caption"
        );
        game_over::cook(&source, output.path())?;
        let defeat: resonance_content::game_over::Art =
            json(output.path(), resonance_content::game_over::document::PATH)?;
        let defeat = defeat.prepare(
            |path| fs::read(output.path().join(path)).map_err(anyhow::Error::from),
            &diagnostics,
        )?;
        font.validate_text(&defeat.caption)?;
        for choice in &defeat.choices {
            font.validate_text(choice)?;
        }
        if language == Language::English {
            assert_eq!(track.cues[1].frame, 154);
            assert_eq!(track.cues[1].lines[0].text, "Once upon a time,");
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM pointing to an installed Steam copy"]
fn installed_categories_load_in_current_content_format() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let output = tempfile::tempdir()?;
    cook_tables(
        &source::Source::open(&installation, Language::English)?,
        output.path(),
    )?;
    let enemies: Mappings = json(
        output.path(),
        resonance_content::battle_enemy::document::MAPPINGS,
    )?;
    let formations: battle_rules::Formations = json(output.path(), "battle/formations.json")?;
    let formations = formations.prepare(&enemies)?;
    assert_eq!(formations.records.len(), 1000);
    assert_eq!(formations.pools.len(), 736);
    for (id, expected) in [(801, [251, 252, 253]), (802, [254, 255, 256])] {
        assert_eq!(
            formations.records[id]
                .resources
                .iter()
                .map(|r| r.enemy)
                .collect::<Vec<_>>(),
            expected
        );
    }
    let items: items::Document = json(output.path(), items::PATH)?;
    assert_eq!(items.prepare()?.inventory.len(), 531);
    for id in 528..531 {
        let name = items.mappings.target(id)?;
        let expected = ["Free Pass", "Theater Ticket", "Forget-Me-Not"][(id - 528) as usize];
        assert_eq!(items.items[name].text["name"], expected);
    }
    let techniques: arte::document::Document = json(output.path(), arte::PATH)?;
    let prepared = techniques.prepare()?;
    assert_eq!(techniques.mappings.target(253)?, "lightning-thrust");
    assert_eq!(techniques.mappings.target(260)?, "infernal-ruin");
    assert_eq!(techniques.mappings.target(261)?, "crimson-devastation");
    assert_eq!(prepared.catalogue.definitions.len(), 262);
    let sword_rain = techniques.mappings.id("sword-rain-alpha")? as usize;
    assert_eq!(
        prepared.menu[sword_rain]
            .alternatives
            .iter()
            .filter(|&&id| id != 0)
            .count(),
        5
    );
    let fields: Mappings = json(output.path(), resonance_content::library::FIELDS)?;
    assert_eq!(fields.iter().count(), 509);
    let forage: resonance_content::foraging::Document =
        json(output.path(), resonance_content::foraging::PATH)?;
    assert_eq!(forage.prepare(&items.mappings, &fields)?.pools.len(), 6);
    let ex: resonance_content::ex_skills::Document =
        json(output.path(), resonance_content::ex_skills::PATH)?;
    let ex = ex.prepare(&items.mappings)?;
    ex.validate_rules(531)?;
    let names = Mappings::new(
        CharacterId::ALL
            .iter()
            .filter_map(|id| id.resource_name().map(|n| (id.value(), n.to_owned())))
            .collect::<BTreeMap<_, _>>(),
    )?;
    let recoil: battle_rules::Recoil = json(output.path(), resonance_content::battle_recoil::PATH)?;
    assert_eq!(recoil.prepare(&names)?.impulses.len(), 19);
    // Reruns overwrite interrupted output directly, without freshness state.
    let before = fs::read(output.path().join(arte::PATH))?;
    fs::write(output.path().join(arte::PATH), b"interrupted cook")?;
    cook_tables(
        &source::Source::open(&installation, Language::English)?,
        output.path(),
    )?;
    assert_eq!(fs::read(output.path().join(arte::PATH))?, before);
    for language in [
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
    ] {
        cook_tables(
            &source::Source::open(&installation, language)?,
            output.path(),
        )?;
        let translated: items::Document = json(output.path(), items::PATH)?;
        translated.prepare()?;
        assert_ne!(
            translated.items[translated.mappings.target(1)?].text,
            items.items[items.mappings.target(1)?].text
        );
        let techniques: arte::document::Document = json(output.path(), arte::PATH)?;
        techniques.prepare()?;
        let ex: resonance_content::ex_skills::Document =
            json(output.path(), resonance_content::ex_skills::PATH)?;
        ex.prepare(&translated.mappings)?.validate_rules(531)?;
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optionally RESONANCE_TEST_STEAM_FIELD"]
fn installed_field_sources_convert() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let language = std::env::var("RESONANCE_TEST_STEAM_LANGUAGE")
        .ok()
        .map(|value| value.parse::<Language>())
        .transpose()?
        .unwrap_or_default();
    let source = source::Source::open(&installation, language)?;
    let temporary = tempfile::tempdir()?;
    let output = std::env::var_os("RESONANCE_TEST_STEAM_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_path_buf());
    if output != temporary.path() {
        ensure!(
            !output.exists(),
            "private field test output must not already exist"
        );
        fs::create_dir_all(&output)?;
    }
    field_directory::cook(&source, &output)?;
    let selected = std::env::var("RESONANCE_TEST_STEAM_FIELD")
        .ok()
        .map(|id| id.parse::<u32>())
        .transpose()?;
    field_core::cook(&source, &output, selected)?;
    let fields: Mappings = json(&output, resonance_content::library::FIELDS)?;
    for (id, name) in fields
        .iter()
        .filter(|(id, _)| selected.is_none_or(|only| only == *id))
    {
        let base = output.join("fields").join(name);
        let bytes = fs::read(base.join("scripts/scenario.ssb"))?;
        symphonia_script::Program::decode(&bytes).with_context(|| format!("cooked field {id}"))?;
        if base.join("messages.json").exists() {
            let _: Vec<symphonia_script::message::Message> =
                serde_json::from_slice(&fs::read(base.join("messages.json"))?)?;
        }
        if base.join("collision.glb").exists() {
            resonance_content::field::collision_mesh::decode(&fs::read(
                base.join("collision.glb"),
            )?)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optionally RESONANCE_TEST_STEAM_FIELD"]
fn installed_scenery_and_skinned_body_use_shared_assets() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let output = tempfile::tempdir()?;
    field_directory::cook(&source, output.path())?;
    field_settings::cook(&source, output.path())?;
    let selected = std::env::var("RESONANCE_TEST_STEAM_FIELD")
        .ok()
        .map(|id| id.parse::<u32>())
        .transpose()?
        .unwrap_or(332);
    field_scene::cook(&source, output.path(), Some(selected))?;
    let names: Mappings = json(output.path(), resonance_content::library::FIELDS)?;
    let scene: resonance_content::field::document::Scene = json(
        output.path(),
        &format!("fields/{}/scene.json", names.target(selected)?),
    )?;
    assert!(!scene.parts.is_empty());
    for mut part in scene.parts {
        assert!(
            part.materials
                .iter()
                .all(|material| material.depth_write == (part.resource != 2)),
            "scenery must use GC stage depth writes, including translucent classroom windows"
        );
        let bytes = fs::read(output.path().join(&part.mesh))?;
        part.prepare_bones(&bytes)?;
        assert!(!part.bone_names.is_empty());
        assert!(!part.materials.is_empty());
        let skeleton = resonance_content::animation::Skeleton::from_glb(&bytes)?;
        for clip in &part.clips {
            let motion =
                clip.decode_for_skeleton(&fs::read(output.path().join(&clip.motion))?, &skeleton)?;
            motion.validate(&skeleton)?;
            resonance_content::animation::pose::sample(
                &skeleton,
                &motion,
                motion.duration_frames * 0.5,
            )?;
        }
        for image in part.textures {
            resonance_asset_writer::texture::ktx2(&fs::read(output.path().join(image))?)?;
        }
    }
    for path in scene.camera_tracks.values() {
        let camera = resonance_content::camera_motion::CameraMotion::decode(&fs::read(
            output.path().join(path),
        )?)?;
        camera.sample(0.)?;
    }
    // The same converter must retain weighted joints for party bodies.
    let image = pe::Image::parse(&source.executable)?;
    let name = image.text(pe::word(image.slice(0x8cfe80, 4)?, 0)?)?;
    let bytes = source.require(name)?;
    let package = package::Package::parse(&bytes)?;
    let descriptor = package.reference(0)?.context("party package has no body")?;
    let (part, native) = model::cook(
        &source,
        output.path(),
        descriptor,
        "characters/lloyd",
        "body",
    )?;
    assert!(native.meshes.iter().any(|mesh| mesh.joints.is_some()));
    let bytes = fs::read(output.path().join(part.mesh))?;
    let skeleton = resonance_content::animation::Skeleton::from_glb(&bytes)?;
    skeleton.bind_pose()?;
    assert!(skeleton.bones.len() > 10);
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optionally RESONANCE_TEST_STEAM_CHARACTER"]
fn installed_party_models_and_motions_use_shared_reader() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let output = tempfile::tempdir()?;
    let selected = std::env::var("RESONANCE_TEST_STEAM_CHARACTER")
        .ok()
        .map(|id| id.parse::<u8>())
        .transpose()?;
    character_models::cook(&source, output.path(), selected)?;
    let mappings: resonance_content::field::document::ModelMappings =
        json(output.path(), resonance_content::field::document::MODELS)?;
    assert_eq!(mappings.0.len(), if selected.is_some() { 7 } else { 63 });
    let owners = mappings
        .0
        .values()
        .map(|target| target.model())
        .collect::<std::collections::BTreeSet<_>>();
    let mut clips = 0;
    for owner in owners {
        let mut model: resonance_content::field::document::Model =
            json(output.path(), &format!("meshes/{owner}/model.json"))?;
        model.validate()?;
        for part in &mut model.parts {
            let bytes = fs::read(output.path().join(&part.mesh))?;
            part.prepare_bones(&bytes)?;
            let skeleton = resonance_content::animation::Skeleton::from_glb(&bytes)?;
            assert!(!part.clips.is_empty());
            for clip in &part.clips {
                let motion = clip
                    .decode_for_skeleton(&fs::read(output.path().join(&clip.motion))?, &skeleton)?;
                motion.validate(&skeleton)?;
                for frame in [0., motion.duration_frames * 0.5, motion.duration_frames] {
                    resonance_content::animation::pose::sample(&skeleton, &motion, frame)?;
                }
                clips += 1;
            }
        }
        model.hidden_nodes(&model.hidden_geometry)?;
        assert!(model.parts.iter().any(|p| p.outline_color.is_some()));
    }
    assert!(clips > 30);
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM"]
fn installed_field_settings_use_shared_contract() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let output = tempfile::tempdir()?;
    for language in [
        Language::English,
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
    ] {
        let source = source::Source::open(&installation, language)?;
        field_settings::cook(&source, output.path())?;
        let settings: resonance_content::field::settings::Settings =
            json(output.path(), resonance_content::field::settings::PATH)?;
        settings.effects.validate()?;
        settings.blink.validate()?;
        settings.contact_shadow.validate()?;
        assert!(!settings.save_point.tutorial.0.is_empty());
        assert!(!settings.save_point.no_gem.0.is_empty());
        for path in [
            &settings.toon_ramp,
            &settings.contact_shadow.texture,
            &settings.effects.emote_texture,
            &settings.effects.status_texture,
        ] {
            resonance_asset_writer::texture::ktx2(&fs::read(output.path().join(path))?)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM"]
fn installed_global_actor_models_use_shared_documents() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let output = tempfile::tempdir()?;
    character_models::cook(&source, output.path(), None)?;
    let models = field_models::cook(&source, output.path())?;
    assert!(models.mappings.0.len() > 200);
    for resource in [131072, 131073, 131074] {
        let path = models.mappings.model_path(resource)?;
        let mut model: resonance_content::field::document::Model = json(output.path(), &path)?;
        model.validate()?;
        for part in &mut model.parts {
            let bytes = fs::read(output.path().join(&part.mesh))?;
            part.prepare_bones(&bytes)?;
            let skeleton = resonance_content::animation::Skeleton::from_glb(&bytes)?;
            for clip in &part.clips {
                clip.decode_for_skeleton(&fs::read(output.path().join(&clip.motion))?, &skeleton)?
                    .validate(&skeleton)?;
            }
        }
        model.hidden_nodes(&model.hidden_geometry)?;
        if let Some(path) = model.collision {
            resonance_content::field::collision_mesh::decode_model(&fs::read(
                output.path().join(path),
            )?)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY pointing to a cooked Steam library"]
fn cooked_field_actor_documents_assemble() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let selected = std::env::var("RESONANCE_TEST_STEAM_FIELD")
        .ok()
        .map(|id| id.parse::<u32>())
        .transpose()?
        .unwrap_or(332);
    let fields: Mappings = json(&root, resonance_content::library::FIELDS)?;
    let path = format!("fields/{}/field.json", fields.target(selected)?);
    let definition: resonance_content::field::document::Definition = json(&root, &path)?;
    definition.validate()?;
    let assets = definition.assemble(selected, |path| {
        Ok(std::sync::Arc::from(fs::read(root.join(path))?))
    })?;
    assert!(assets.actors.len() >= 63);
    assert!(assets.actors.iter().any(|actor| actor.resource == 1));
    assert!(
        assets
            .actors
            .iter()
            .any(|actor| actor.resource > 9 && actor.resource >> 24 != 0x23)
    );
    assert!(
        assets
            .actors
            .iter()
            .flat_map(|actor| &actor.parts)
            .flat_map(|part| &part.clips)
            .any(|clip| clip.animation_resource.is_some())
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; writes the actor category into that library"]
fn installed_field_actors_cook() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let resources = resources::Resources::read(&source)?;
    let mut mappings: resonance_content::field::document::ModelMappings =
        json(&root, resonance_content::field::document::MODELS)?;
    // Start from the party/global model passes after an interrupted actor cook.
    mappings.0.retain(|id, _| {
        (1..=9).contains(id)
            || matches!(id >> 24, 0x23 | 0x24)
            || (*id >= 0x10000 && resources.entries.contains_key(id))
    });
    let mut sources = BTreeMap::new();
    for &resource in mappings.0.keys() {
        if (1..=9).contains(&resource) || resource >> 24 == 0x23 {
            continue;
        }
        let id = if resource >> 24 == 0x24 {
            resource & 0xffff
        } else {
            resource
        };
        if let Ok(name) = resources.loaded(id) {
            sources.insert(resource, name.to_owned());
        }
    }
    let standalone = json(&root, "meshes/standalone/mappings.json")?;
    let mut models = field_models::Models {
        resources,
        mappings,
        standalone,
        sources,
    };
    let selected = std::env::var("RESONANCE_TEST_STEAM_FIELD")
        .ok()
        .map(|id| id.parse::<u32>())
        .transpose()?;
    field_actors::cook(&source, &root, &mut models, selected)
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM"]
fn installed_retained_actor_motion_uses_shared_reader() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let output = tempfile::tempdir()?;
    for name in ["ask_t01_b000_b002.pac", "faa_d02_b000_b003.pac"] {
        let (model, _) = actor_model::cook(&source, output.path(), name, "meshes/retained-actor")?;
        assert!(!model.parts[0].clips.is_empty());
        for part in &model.parts {
            let skeleton = resonance_content::animation::Skeleton::from_glb(&fs::read(
                output.path().join(&part.mesh),
            )?)?;
            for clip in &part.clips {
                let motion = clip
                    .decode_for_skeleton(&fs::read(output.path().join(&clip.motion))?, &skeleton)?;
                motion.validate(&skeleton)?;
                assert!(!motion.tracks.is_empty());
                for frame in [0., motion.duration_frames * 0.5, motion.duration_frames] {
                    resonance_content::animation::pose::sample(&skeleton, &motion, frame)?;
                }
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM"]
fn installed_menu_settings_use_current_documents_in_all_languages() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let output = tempfile::tempdir()?;
    for language in [
        Language::English,
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
    ] {
        let source = source::Source::open(&installation, language)?;
        menu_settings::cook(&source, output.path())
            .with_context(|| format!("menu settings {language:?}"))?;
        let manual: resonance_content::menu_data::TrainingManual =
            json(output.path(), resonance_content::menu_data::MANUAL_PATH)?;
        manual.validate()?;
        assert_eq!(manual.chapters.len(), 9);
        let rename: resonance_content::menu_data::RenameData =
            json(output.path(), resonance_content::menu_data::RENAME_PATH)?;
        rename.validate()?;
        assert_eq!(rename.keyboard.chars().count(), 104);
        assert_eq!(rename.defaults[0], "Lloyd");
        assert_eq!(
            rename.defaults[2],
            if language == Language::French {
                "Génis"
            } else {
                "Genis"
            }
        );
        let options: resonance_content::options::Document =
            json(output.path(), resonance_content::options::PATH)?;
        let options = options.prepare()?;
        options.validate()?;
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM"]
fn installed_character_and_recipe_tables() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let output = tempfile::tempdir()?;
    let mut title_mappings = None;
    for language in [
        Language::English,
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
    ] {
        let source = source::Source::open(&installation, language)?;
        cook_tables(&source, output.path())?;
        let items: items::Document = json(output.path(), items::PATH)?;
        let ex: resonance_content::ex_skills::Document =
            json(output.path(), resonance_content::ex_skills::PATH)?;
        let recipes: resonance_content::cooking::Document =
            json(output.path(), resonance_content::cooking::PATH)?;
        recipes
            .prepare(&items.mappings, &ex.mappings)?
            .validate(531)?;
        assert_eq!(recipes.mappings.iter().count(), 24);
        let characters: resonance_content::characters::Document =
            json(output.path(), resonance_content::characters::PATH)?;
        assert_eq!(
            characters.initial_names()?[2],
            if language == Language::French {
                "Génis"
            } else {
                "Genis"
            }
        );
        let titles: resonance_content::titles::Document =
            json(output.path(), resonance_content::titles::PATH)?;
        let identities = serde_json::to_value(&titles.mappings)?;
        if let Some(previous) = &title_mappings {
            assert_eq!(previous, &identities);
        }
        title_mappings = Some(identities);
        assert_eq!(
            titles.mappings[&resonance_content::overworld::PartyCharacter::Kratos]
                .id("kratos/tetra-slash")?,
            11
        );
        let costume_names: BTreeMap<u8, BTreeMap<u8, serde_json::Value>> =
            serde_json::from_str(include_str!("../../importer/src/costume-names.json"))?;
        let titles = titles.prepare(&items.mappings, |member, name| {
            use resonance_content::menu_data::Costume;
            let id = if let Some(id) = name.strip_prefix("costume-") {
                id.parse::<u8>()?
            } else {
                *costume_names[&(member as u8)]
                    .iter()
                    .find(|(_, entry)| entry["name"] == name)
                    .context("unknown native costume name")?
                    .0
            };
            Ok(match id {
                0 => Costume::Standard,
                1 => Costume::Variant1,
                2 => Costume::Variant2,
                3 => Costume::Story,
                4 => Costume::Variant4,
                5 => Costume::Variant5,
                6 => Costume::Variant6,
                _ => anyhow::bail!("unknown costume slot"),
            })
        })?;
        assert_eq!(titles.rules.iter().map(Vec::len).sum::<usize>(), 177);
        let techniques: resonance_content::arte::document::Document =
            json(output.path(), resonance_content::arte::PATH)?;
        let definitions = characters.prepare(
            &items.mappings,
            &techniques.prepare()?,
            &titles.rules,
            &recipes.mappings,
            &ex,
        )?;
        let session = resonance_content::session::SessionData {
            version: 1,
            items: items.prepare()?.inventory,
            characters: definitions,
            experience: characters.experience_curve()?,
            rules: None,
        };
        session.validate()?;
        let mut party = resonance_events::party::Party::new(&session, Default::default())?;
        for costume in [5, 6] {
            party.members[0].costume = costume;
            party.validate(&session)?;
            let restored: resonance_events::party::Party =
                serde_json::from_slice(&serde_json::to_vec(&party)?)?;
            restored.validate(&session)?;
            assert_eq!(restored.members[0].costume, costume);
        }
        party.members[0].costume = 7;
        assert!(party.validate(&session).is_err());
        assert_eq!(session.characters.len(), 9);
        assert_eq!(characters.experience_curve()?.len(), 252);
        assert!(
            characters
                .full_names(&resonance_content::diagnostics::Diagnostics::new(true))?
                .unwrap()
                .iter()
                .all(|format| format.contains("{name}"))
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; cooks only the original cup prop"]
fn installed_cup_keeps_source_visible_geometry_and_numbered_anchors_hidden() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let output = tempfile::tempdir()?;
    let (document, mut cup) = actor_model::cook(
        &source,
        output.path(),
        "npc_all_b217.pac",
        "meshes/props/mug",
    )?;
    assert_eq!(cup.rig.nodes.len(), 4);
    assert_eq!(cup.meshes.len(), 4);
    for (i, node) in cup.rig.nodes.iter().enumerate() {
        assert_eq!(node.name, format!("KK_OBJ000_{:02}", i + 1));
        assert!(node.visible);
    }
    assert!(document.hidden_geometry.is_empty());
    // A native hidden flag still wins for ordinary prop names.
    cup.rig.nodes[0].visible = false;
    assert_eq!(cup.rig.hidden_geometry(), ["KK_OBJ000_01"]);

    // A real installed Lloyd rig retains its numbered equipment/effect anchors.
    let image = pe::Image::parse(&source.executable)?;
    let package_name = image.text(pe::word(image.slice(0x8cfe80, 4)?, 0)?)?;
    let bytes = source.require(package_name)?;
    let package = package::Package::parse(&bytes)?;
    let descriptor = source.require(package.reference(0)?.context("Lloyd body")?)?;
    let lloyd = model::Rig::read(&descriptor)?;
    let hidden = lloyd.hidden_geometry();
    assert!(lloyd.joint_names().iter().any(|name| name == "kk00"));
    assert!(hidden.iter().any(|name| name == "kk00"));
    Ok(())
}
