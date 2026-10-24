use super::super::named_tests::{fixture, wav, write};
use super::*;
use resonance_content::{
    battle_audio::{Selection, document},
    field_audio::document as field,
    library::Library,
};

#[test]
fn recorded_battle_handoff_preserves_queued_pcm_position_and_inherited_environment() -> Result<()> {
    let root = fixture()?;
    let samples: Vec<_> = (0..10000)
        .map(|i| ((i % 1000) * 20 - 9000) as i16)
        .collect();
    write(
        root.path(),
        "audio/music/test-song/music.wav",
        &wav(&samples, RATE)?,
    )?;
    write(
        root.path(),
        "audio/music/test-song/battle.wav",
        &wav(&[-8000; 128], RATE)?,
    )?;
    write(root.path(), "audio/music/test-song/music.json", br#"{"version":1,"audio":"audio/music/test-song/music.wav","loop":{"start_frame":0,"end_frame":10000},"battle":{"audio":"audio/music/test-song/battle.wav","loop":{"start_frame":0,"end_frame":128}}}"#)?;
    write(
        root.path(),
        "audio/music/mappings.json",
        br#"{"7":"test-song","8":"test-song"}"#,
    )?;
    let mut definition: field::Definition =
        serde_json::from_slice(&fs::read(root.path().join("fields/iselia/audio.json"))?)?;
    definition.music[0].ids.insert(8);
    definition.music_reverbs.presets[1][2] = 2.;
    definition.music_reverbs.selectors.push(2);
    write(
        root.path(),
        "fields/iselia/audio.json",
        &serde_json::to_vec(&definition)?,
    )?;
    let mixer = Cache::default().capture_named(
        "fields/iselia/audio.json",
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
    assert_eq!(
        frames.studio.music_reverb(),
        definition.music_reverbs.presets[0]
    );
    assert!(frames.battle.is_none());

    // Context comes from the actual inherited preset, even outside battle.
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
    assert_eq!(frames.frame()?.unwrap(), value(-8000));
    Ok(())
}

#[test]
fn named_battle_audio_shares_programs_and_samples_but_keeps_current_context_and_aliases()
-> Result<()> {
    let root = fixture()?;
    let program_path = "audio/effects/test-cue/sound.json";
    let mut program = serde_json::from_slice::<resonance_audio::effect::Definition>(&fs::read(
        root.path().join(program_path),
    )?)?
    .into_program()?;
    program.reverbs = None;
    write(
        root.path(),
        program_path,
        &serde_json::to_vec(&resonance_audio::effect::Definition::Synthesized(
            program.clone(),
        ))?,
    )?;
    let (resources, score, tables) =
        test_score_data(resonance_audio::data::ScoreOrigin::SoundEffect);
    program.programs = resources.programs;
    program.score = score;
    program.tables = tables;
    write(
        root.path(),
        "audio/effects/cue/sound.json",
        &serde_json::to_vec(&resonance_audio::effect::Definition::Synthesized(
            program.clone(),
        ))?,
    )?;
    write(
        root.path(),
        "audio/effects/mappings.json",
        br#"{"3":"cue"}"#,
    )?;
    let mut field: field::Definition =
        serde_json::from_slice(&fs::read(root.path().join("fields/iselia/audio.json"))?)?;
    let field_environment = [[0.5, 0.5, 1., 0.5, 0.]; 2];
    field.reverbs = Some(field_environment);
    write(
        root.path(),
        "fields/iselia/audio.json",
        &serde_json::to_vec(&field)?,
    )?;
    let mut definition = document::Definition {
        version: 1,
        music: "audio/music/mappings.json".into(),
        effects: "audio/effects/mappings.json".into(),
        streams: Some(65536),
        archive_playback: [(
            65536,
            field::Playback {
                sample_rate: Some(32028),
            },
        )]
        .into(),
        voice_playback: BTreeMap::new(),
        music_reverbs: field.music_reverbs.clone(),
        voice_gains: field.voice_gains.clone(),
        reverbs: [[0.5, 0.5, 2., 0.5, 0.], field_environment[1]],
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
    let field = cache.capture_named(
        "fields/iselia/audio.json",
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
    assert_eq!(before.mixer.sounds[&3].reverbs(), definition.reverbs);
    assert_eq!(before.mixer.sounds[&3].reverbs(), definition.reverbs);
    assert!(!Arc::ptr_eq(&field.sounds[&3], &before.mixer.sounds[&3]));
    assert!(Arc::ptr_eq(
        &field.sounds[&3].program().unwrap().resources().samples[&1],
        &before.mixer.sounds[&3]
            .program()
            .unwrap()
            .resources()
            .samples[&1]
    ));
    assert!(Arc::ptr_eq(
        &before.mixer.voices[&7],
        &before.mixer.voices[&65543]
    ));
    assert_eq!(before.mixer.voices[&7].rate, 32028);
    assert!(!before.mixer.voices.contains_key(&131079));

    definition.reverbs[0][2] = 3.;
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
    assert_eq!(changed.mixer.sounds[&3].reverbs(), definition.reverbs);
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
        "audio/effects/mappings.json",
        &serde_json::to_vec(&BTreeMap::from([
            (3u32, "test-cue"),
            (u32::try_from(entry)?, "test-cue"),
        ]))?,
    )?;
    write(
        root.path(),
        "audio/effects/test-cue/sound.json",
        br#"{
        "kind":"wave", "version":1, "audio":"audio/effects/test-cue/sound.wav",
        "loop":{"start_frame":0,"end_frame":64}
    }"#,
    )?;
    write(
        root.path(),
        "audio/effects/test-cue/sound.wav",
        &wav(&[8000; 64], RATE)?,
    )?;
    let mut definition: field::Definition =
        serde_json::from_slice(&fs::read(root.path().join("fields/iselia/audio.json"))?)?;
    definition.effects[0].ids.insert(u32::try_from(entry)?);
    write(
        root.path(),
        "fields/iselia/audio.json",
        &serde_json::to_vec(&definition)?,
    )?;
    let mixer = Cache::default().capture_named(
        "fields/iselia/audio.json",
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
