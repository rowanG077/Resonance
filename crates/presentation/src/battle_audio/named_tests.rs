use super::super::named_tests::{fixture, looped_wav, wav, write};
use super::*;
use resonance_content::{
    battle_audio::{Selection, document},
    field_audio::document as field,
    library::Library,
};

#[test]
fn recorded_music_volume_curve_survives_battle_entry_song_changes_and_return() -> Result<()> {
    let root = fixture()?;
    let path = resonance_audio::effect::recipe::TABLES;
    let mut tables: serde_json::Value = serde_json::from_slice(&fs::read(root.path().join(path))?)?;
    let gains: Vec<_> = (0..129)
        .map(|i| (i.min(127) as f32 / 127.).powi(2))
        .collect();
    tables["mix"]["volume"] = serde_json::to_value(&gains)?;
    write(root.path(), path, &serde_json::to_vec(&tables)?)?;
    let mixer = Cache::default().capture_test_field(
        "fields/iselia/field.json",
        &mut Library::open(root.path(), &[])?.snapshot(),
        Diagnostics::new(true),
        || false,
    )?;
    let battle = Arc::new(Assets {
        diagnostics: Diagnostics::new(true),
        mixer: mixer.clone(),
        effect_spatial: DEFAULT_EFFECT_SPATIAL,
        voice_spatial: DEFAULT_VOICE_SPATIAL,
    });
    for level in [0, 32, 64, 127] {
        let (source, mut control) = mixer.clone().session();
        control.levels([level, 0, 0])?;
        let mut frames = source.decoder();
        frames.command(AudioCommand::Music(MusicCommand::Play(7)))?;
        for phase in 0..4 {
            match phase {
                1 => frames.apply_battle_command(Command::Begin(
                    battle.clone(),
                    Settings {
                        music: level,
                        effects: 0,
                        battle_effects: 0,
                        voice: 0,
                        stereo: true,
                    },
                    Some(7),
                ))?,
                2 => {
                    frames.command(AudioCommand::Music(MusicCommand::Stop))?;
                    frames.command(AudioCommand::Music(MusicCommand::Play(7)))?;
                }
                3 => frames.apply_battle_command(Command::End(true))?,
                _ => (),
            }
            frames.command(AudioCommand::MusicVolume {
                volume: 127,
                duration_ticks: 0,
            })?;
            let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2 * gains[usize::from(level)];
            for _ in 0..BLOCK_FRAMES * 2 {
                for sample in frames.frame()?.unwrap() {
                    assert!(
                        (sample - expected).abs() < 1e-7,
                        "level {level}, phase {phase}: {sample} != {expected}"
                    );
                }
            }
        }
        control.check()?;
    }
    Ok(())
}

#[test]
fn a_shared_mix_edit_reaches_field_and_battle_and_preserves_captured_settings() -> Result<()> {
    use resonance_content::field_audio::mix::{Mix, PATH};
    let root = fixture()?;
    let definition = document::Definition {
        speech_rates: vec![],
        reverbs: None,
        effect_spatial: DEFAULT_EFFECT_SPATIAL,
        voice_spatial: DEFAULT_VOICE_SPATIAL,
    };
    write(
        root.path(),
        document::PATH,
        &serde_json::to_vec(&definition)?,
    )?;
    let library = Library::open(root.path(), &[])?;
    let mut pinned = library.snapshot();
    let before = document::Catalogue::capture(&mut pinned, &|| false)?;
    let mut mix = Mix::read(&mut pinned)?;
    mix.field_reverbs[0][2] = 3.;
    mix.battle_reverbs[0][2] = 4.;
    mix.music_reverbs.presets = vec![[0.5, 0.5, 5., 0.5, 0.]];
    mix.music_reverbs.selectors[7] = 1;
    mix.voice_gains[64] = 0.5;
    let layer = tempfile::tempdir()?;
    write(layer.path(), PATH, &serde_json::to_vec(&mix)?)?;
    let edited = Library::open(
        root.path(),
        &[resonance_content::library::Mod {
            name: "shared-mix".into(),
            root: layer.path().into(),
        }],
    )?;
    let mut snapshot = edited.snapshot();
    let battle = document::Catalogue::capture(&mut snapshot, &|| false)?;
    let field = Cache::default().capture_test_field(
        "fields/iselia/field.json",
        &mut snapshot,
        Diagnostics::new(true),
        || false,
    )?;
    assert_eq!(battle.audio.reverbs, mix.battle_reverbs);
    assert_eq!(field.reverbs, mix.field_reverbs);
    assert_eq!(battle.audio.voice_gains[64], 0.5);
    assert_eq!(field.voice_gains[64], 0.5);
    assert_eq!(field.music_reverbs.as_ref(), Some(&mix.music_reverbs));
    assert_eq!(battle.audio.music_reverbs, mix.music_reverbs);
    assert_eq!(before.audio.reverbs[0][2], 2.);
    assert_eq!(
        document::Catalogue::capture(&mut pinned, &|| false)?
            .audio
            .reverbs[0][2],
        2.
    );
    let mut choices = snapshot
        .json::<resonance_content::field::document::Definition>(
            "fields/iselia/field.json",
            1024 * 1024,
        )?
        .audio;
    choices.reverbs = Some([[0.5, 0.5, 6., 0.5, 0.]; 2]);
    let local = choices.environment(&mut snapshot)?;
    assert_eq!(local.reverbs[0][2], 6.);
    assert_eq!(local.voice_gains[64], 0.5);
    Ok(())
}

#[test]
fn recorded_battle_handoff_preserves_queued_pcm_position_and_inherited_environment() -> Result<()> {
    let root = fixture()?;
    let samples: Vec<_> = (0..10000)
        .map(|i| ((i % 1000) * 20 - 9000) as i16)
        .collect();
    write(
        root.path(),
        "audio/music/test-song.wav",
        &looped_wav(&samples, RATE)?,
    )?;
    write(
        root.path(),
        "audio/music/battle-song.wav",
        &looped_wav(&[-8000; 128], RATE)?,
    )?;
    write(
        root.path(),
        "audio/music/mappings.json",
        br#"{"7":"test-song.wav","8":"battle-song.wav"}"#,
    )?;
    let mut definition = crate::field_audio::named_tests::read_choices(root.path())?;
    definition.music.insert(8);
    let mut mix: resonance_content::field_audio::mix::Mix = serde_json::from_slice(&fs::read(
        root.path().join(resonance_content::field_audio::mix::PATH),
    )?)?;
    mix.music_reverbs.presets = vec![mix.field_reverbs[0], mix.battle_reverbs[0]];
    mix.music_reverbs.selectors[7] = 1;
    mix.music_reverbs.selectors.push(2);
    write(
        root.path(),
        resonance_content::field_audio::mix::PATH,
        &serde_json::to_vec(&mix)?,
    )?;
    crate::field_audio::named_tests::write_choices(root.path(), &definition)?;
    let mixer = Cache::default().capture_test_field(
        "fields/iselia/field.json",
        &mut Library::open(root.path(), &[])?.snapshot(),
        Diagnostics::new(true),
        || false,
    )?;
    let battle = Arc::new(Assets {
        diagnostics: Diagnostics::new(true),
        mixer: mixer.clone(),
        effect_spatial: DEFAULT_EFFECT_SPATIAL,
        voice_spatial: DEFAULT_VOICE_SPATIAL,
    });
    let (source, _control) = mixer.clone().session();
    let mut frames = source.decoder();
    let value = |sample: i16| [f32::from(sample) * std::f32::consts::FRAC_1_SQRT_2 / 32768.; 2];
    let full = |frames: &mut Frames| {
        frames.command(AudioCommand::MusicVolume {
            volume: 127,
            duration_ticks: 0,
        })
    };
    frames.command(AudioCommand::Music(MusicCommand::Play(7)))?;
    full(&mut frames)?;
    for &sample in &samples[..357] {
        assert_eq!(frames.frame()?.unwrap(), value(sample));
    }
    frames.apply_battle_command(Command::Begin(
        battle,
        Settings {
            music: 127,
            effects: 127,
            battle_effects: 127,
            voice: 127,
            stereo: true,
        },
        Some(8),
    ))?;
    full(&mut frames)?;
    for &sample in &samples[357..480] {
        assert_eq!(frames.frame()?.unwrap(), value(sample));
    }
    for _ in 0..340 {
        assert_eq!(frames.frame()?.unwrap(), value(-8000));
    }
    let player = &frames
        .battle
        .as_ref()
        .unwrap()
        .field
        .music
        .as_ref()
        .unwrap()
        .player
        .player;
    assert_eq!(
        player.position(),
        480,
        "battle must not advance the retained field song"
    );
    frames.apply_battle_command(Command::End(true))?;
    for _ in 820..960 {
        assert_eq!(frames.frame()?.unwrap(), value(-8000));
    }
    for &sample in &samples[480..880] {
        assert_eq!(frames.frame()?.unwrap(), value(sample));
    }
    assert_eq!(frames.studio.music_reverb(), mix.music_reverbs.presets[0]);
    assert!(frames.battle.is_none());

    // A selected song is the same recording in field and battle contexts.
    frames.command(AudioCommand::Music(MusicCommand::Play(8)))?;
    full(&mut frames)?;
    while !frames.frame.is_multiple_of(BLOCK_FRAMES as u64) {
        frames.frame()?;
    }
    assert_eq!(frames.frame()?.unwrap(), value(-8000));
    frames.command(AudioCommand::Music(MusicCommand::Play(7)))?;
    full(&mut frames)?;
    while !frames.frame.is_multiple_of(BLOCK_FRAMES as u64) {
        frames.frame()?;
    }
    assert_eq!(frames.frame()?.unwrap(), value(samples[0]));
    Ok(())
}

#[test]
fn named_battle_audio_uses_current_samples_context_and_aliases() -> Result<()> {
    let root = fixture()?;
    let program_path = "audio/effects/test-cue.json";
    let mut program = serde_json::from_slice::<resonance_audio::effect::Definition>(&fs::read(
        root.path().join(program_path),
    )?)?
    .into_program(&mut |path, _| Ok(fs::read(root.path().join(path))?.into()))?;
    program.reverbs = None;
    write(
        root.path(),
        program_path,
        &serde_json::to_vec(&resonance_audio::effect::Definition::Synthesized(
            resonance_audio::effect::Recipe::from_program(&program)?,
        ))?,
    )?;
    let (resources, score, tables) =
        test_score_data(resonance_audio::data::ScoreOrigin::SoundEffect);
    program.programs = resources.programs;
    program.score = score;
    program.tables = tables;
    write(
        root.path(),
        "audio/effects/cue.json",
        &serde_json::to_vec(&resonance_audio::effect::Definition::Synthesized(
            resonance_audio::effect::Recipe::from_program(&program)?,
        ))?,
    )?;
    write(
        root.path(),
        "audio/mappings.json",
        br#"{"3":"effects/cue.json"}"#,
    )?;
    let mut field = crate::field_audio::named_tests::read_choices(root.path())?;
    let field_environment = [[0.5, 0.5, 1., 0.5, 0.]; 2];
    field.reverbs = Some(field_environment);
    crate::field_audio::named_tests::write_choices(root.path(), &field)?;
    let mut definition = document::Definition {
        speech_rates: vec![field::Speech {
            ids: [7, 65543].into(),
            sample_rate: Some(32028),
        }],
        reverbs: Some([[0.5, 0.5, 2., 0.5, 0.], field_environment[1]]),
        effect_spatial: DEFAULT_EFFECT_SPATIAL,
        voice_spatial: DEFAULT_VOICE_SPATIAL,
    };
    write(
        root.path(),
        document::PATH,
        &serde_json::to_vec(&definition)?,
    )?;
    let library = Library::open(root.path(), &[])?;
    let mut cache = Cache::default();
    let field = cache.capture_test_field(
        "fields/iselia/field.json",
        &mut library.snapshot(),
        Diagnostics::new(true),
        || false,
    )?;
    let selected = Selection {
        music: [7].into(),
        sounds: [3].into(),
        streams: [7].into(),
        voices: [65543].into(),
    };
    let mut pinned = library.snapshot();
    let catalogue = document::Catalogue::capture(&mut pinned, &|| false)?;
    let before = Assets::capture_named(
        &catalogue,
        &selected,
        &mut pinned,
        Diagnostics::new(true),
        &mut cache,
        &|| false,
    )?;
    assert_eq!(field.sounds[&3].reverbs(), field_environment);
    assert_eq!(before.mixer.reverbs, definition.reverbs.unwrap());
    assert_eq!(before.mixer.sounds[&3].reverbs(), before.mixer.reverbs);
    assert!(!Arc::ptr_eq(&field.sounds[&3], &before.mixer.sounds[&3]));
    assert_eq!(
        field.sounds[&3].program().unwrap().resources().samples[&1].pcm,
        before.mixer.sounds[&3]
            .program()
            .unwrap()
            .resources()
            .samples[&1]
            .pcm
    );
    assert!(Arc::ptr_eq(
        &before.mixer.voices[&7],
        &before.mixer.voices[&65543]
    ));
    assert_eq!(before.mixer.voices[&7].rate, 32028);
    assert!(!before.mixer.voices.contains_key(&131079));

    definition.reverbs.as_mut().unwrap()[0][2] = 3.;
    write(
        root.path(),
        document::PATH,
        &serde_json::to_vec(&definition)?,
    )?;
    let mut fresh = library.snapshot();
    let updated = document::Catalogue::capture(&mut fresh, &|| false)?;
    let changed = Assets::capture_named(
        &updated,
        &selected,
        &mut fresh,
        Diagnostics::new(true),
        &mut cache,
        &|| false,
    )?;
    assert_eq!(
        changed.mixer.sounds[&3].reverbs(),
        definition.reverbs.unwrap()
    );
    assert_eq!(before.mixer.sounds[&3].reverbs()[0][2], 2.);
    write(
        root.path(),
        "audio/instruments/test.wav",
        &wav(&[9000, 8000], 22050)?,
    )?;
    write(
        root.path(),
        "audio/speech/archive-a/line.wav",
        &wav(&[7000, 6000], 22050)?,
    )?;
    let mut fresh = library.snapshot();
    let current = document::Catalogue::capture(&mut fresh, &|| false)?;
    let current = Assets::capture_named(
        &current,
        &selected,
        &mut fresh,
        Diagnostics::new(true),
        &mut cache,
        &|| false,
    )?;
    assert_eq!(
        current.mixer.sounds[&3]
            .program()
            .unwrap()
            .resources()
            .samples[&1]
            .pcm,
        [9000, 8000]
    );
    assert_eq!(
        before.mixer.sounds[&3]
            .program()
            .unwrap()
            .resources()
            .samples[&1]
            .pcm,
        [16384]
    );
    assert_eq!(current.mixer.voices[&7].value(0), 7000);
    assert_eq!(before.mixer.voices[&7].value(0), 12000);
    assert_eq!(
        Assets::capture_named(
            &catalogue,
            &selected,
            &mut pinned,
            Diagnostics::new(true),
            &mut cache,
            &|| false
        )?
        .mixer
        .voices[&7]
            .value(0),
        12000
    );
    assert!(
        Assets::capture_named(
            &catalogue,
            &selected,
            &mut pinned,
            Diagnostics::default(),
            &mut cache,
            &|| true
        )
        .is_err()
    );
    fs::remove_file(root.path().join("audio/instruments/test.wav"))?;
    // An unrequested effect must not read its missing instrument. A requested
    // broken effect must not discard independently valid speech in tolerant mode.
    let mut voice_snapshot = library.snapshot();
    let voice_catalogue = document::Catalogue::capture(&mut voice_snapshot, &|| false)?;
    let voice_only = Assets::capture_named(
        &voice_catalogue,
        &Selection {
            streams: [7].into(),
            ..Default::default()
        },
        &mut voice_snapshot,
        Diagnostics::new(true),
        &mut cache,
        &|| false,
    )?;
    assert_eq!(voice_only.mixer.voices[&7].value(0), 7000);
    assert!(voice_only.mixer.sounds.is_empty());
    let diagnostics = Diagnostics::default();
    let tolerant = Assets::capture_named(
        &voice_catalogue,
        &selected,
        &mut library.snapshot(),
        diagnostics.clone(),
        &mut cache,
        &|| false,
    )?;
    assert!(tolerant.mixer.sounds.is_empty());
    assert_eq!(tolerant.mixer.voices[&7].value(0), 7000);
    assert!(diagnostics.has_errors());
    let mut fresh = library.snapshot();
    let current = document::Catalogue::capture(&mut fresh, &|| false)?;
    assert!(
        Assets::capture_named(
            &current,
            &selected,
            &mut fresh,
            Diagnostics::new(true),
            &mut cache,
            &|| false
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn recorded_effects_follow_battle_entry_priority_and_return_to_field_ownership() -> Result<()> {
    let root = fixture()?;
    let entry = resonance_content::field_audio::ServiceCue::BattleEntry as i16;
    write(
        root.path(),
        "audio/mappings.json",
        &serde_json::to_vec(&BTreeMap::from([
            (3u32, "effects/test-cue.json"),
            (u32::try_from(entry)?, "effects/test-cue.json"),
        ]))?,
    )?;
    write(
        root.path(),
        "audio/effects/test-cue.json",
        br#"{
        "kind":"wave", "audio":"audio/effects/test-cue/sound.wav",
        "loop":{"start_frame":0,"end_frame":64}
    }"#,
    )?;
    write(
        root.path(),
        "audio/effects/test-cue/sound.wav",
        &wav(&[8000; 64], RATE)?,
    )?;
    let mut definition = crate::field_audio::named_tests::read_choices(root.path())?;
    definition.effects.insert(u32::try_from(entry)?);
    crate::field_audio::named_tests::write_choices(root.path(), &definition)?;
    let mixer = Cache::default().capture_test_field(
        "fields/iselia/field.json",
        &mut Library::open(root.path(), &[])?.snapshot(),
        Diagnostics::new(true),
        || false,
    )?;
    let battle = Arc::new(Assets {
        diagnostics: Diagnostics::new(true),
        mixer: mixer.clone(),
        effect_spatial: DEFAULT_EFFECT_SPATIAL,
        voice_spatial: DEFAULT_VOICE_SPATIAL,
    });
    let (source, _control) = mixer.clone().session();
    let mut frames = source.decoder();
    frames.command(AudioCommand::Sound {
        id: entry,
        pan: 64,
        volume: 127,
        slot: None,
    })?;
    for _ in 0..37 {
        assert!(frames.frame()?.unwrap()[0] > 0.);
    }
    frames.apply_battle_command(Command::Begin(
        battle,
        Settings {
            music: 127,
            effects: 127,
            battle_effects: 127,
            voice: 127,
            stereo: true,
        },
        None,
    ))?;
    assert!(frames.sounds.is_empty());
    assert_eq!(frames.battle.as_ref().unwrap().effects.len(), 1);
    assert!(
        frames.frame()?.unwrap()[0] > 0.,
        "entry cue must survive the handoff"
    );
    frames.apply_battle_command(Command::Sound {
        category: EffectCategory::Battle,
        binding: Sound::Cue(3),
        score: mixer.sounds[&3].clone(),
        x: Some(0.),
        priority: 10,
    })?;
    for _ in 0..BLOCK_FRAMES {
        frames.frame()?;
    }
    assert_eq!(frames.battle.as_ref().unwrap().effects.len(), 2);
    frames.apply_battle_command(Command::End(true))?;
    assert!(frames.battle.is_none() && frames.sounds.is_empty());
    for _ in 0..(BLOCK_FRAMES + resonance_audio::RELEASE_FRAMES as usize) {
        frames.frame()?;
    }
    assert_eq!(frames.frame()?.unwrap(), [0.; 2]);
    frames.command(AudioCommand::Sound {
        id: 3,
        pan: 0,
        volume: 127,
        slot: None,
    })?;
    let mut heard = false;
    for _ in 0..2 * BLOCK_FRAMES {
        heard |= frames.frame()?.unwrap()[0] > 0.;
    }
    assert!(heard, "returned field must still own its recorded cue");
    Ok(())
}
