use super::*;
use crate::sequence::{SOUND_EFFECT_VOICE_BUDGET, shared::Synthesizer};
use serde_json::json;

#[test]
fn synthesized_effects_read_numeric_program_and_sample_ids_through_the_kind_tag() -> Result<()> {
    let (resources, mut score, tables) = crate::package::tests::playback_data();
    score.origin = crate::data::ScoreOrigin::SoundEffect;
    score.initial_bpm_1024 = 120 * 1024;
    score.loop_start_tick = 0;
    score.tempos.clear();
    score.loop_events.clear();
    score.controls = [crate::music_voice::Controls::default(); 16];
    score.first_events = vec![crate::data::Event {
        tick: 0,
        channel: 0,
        kind: crate::data::EventKind::Notes {
            source: crate::data::VoiceSource::SoundEffect { id: 3 },
            length: 100, // Arrangement lengths are ignored for sound effects.
            voices: vec![crate::data::Note {
                macro_id: 1,
                key: 60,
                velocity: 127,
                pan: 64,
                priority: 10,
                max_voices: 255,
            }],
        },
    }];
    let mut program = Program {
        programs: resources.programs,
        samples: [(
            2,
            crate::package::editable::Sample {
                silence: None,
                path: "audio/instruments/bell/sample.wav".into(),
                key: 60,
                loop_start: 2,
                loop_length: 2,
            },
        )]
        .into(),
        score,
        tables,
        reverbs: Some([[0., 0., 1., 0., 0.]; 2]),
    };
    // Runtime output controls do not change what can be represented by a recipe.
    program.score.controls[0].mono = true;
    program.score.controls[0].source_volume = 0.25;
    let definition = Definition::Synthesized(Recipe::from_program(&program)?);
    program.score.controls[0].group_volume = 0.5;
    assert!(Recipe::from_program(&program).is_err());
    let encoded = serde_json::to_string(&definition)?;
    let value: serde_json::Value = serde_json::from_str(&encoded)?;
    assert!(value.get("version").is_none());
    assert_eq!(
        value["voices"],
        json!([{"group":3,"notes":[{"program":1}]}])
    );
    let decoded: Definition = serde_json::from_str(&encoded)?;
    assert_eq!(
        serde_json::to_value(decoded)?,
        serde_json::to_value(&definition)?
    );
    let duplicate = encoded.replacen("\"programs\":{", "\"programs\":{\"1\":[],", 1);
    assert!(serde_json::from_str::<Definition>(&duplicate).is_err());
    for key in ["01", "65536", "-1"] {
        let mut invalid = serde_json::to_value(&definition)?;
        invalid["programs"] = json!({key: []});
        assert!(serde_json::from_value::<Definition>(invalid).is_err());
    }
    let mut edited = value;
    edited["voices"][0]["notes"][0]["pan"] = 0.into();
    edited["voices"][0]["notes"][0]["velocity"] = 96.into();
    let edited: Definition = serde_json::from_value(edited)?;
    let Definition::Synthesized(edited) = edited else {
        unreachable!()
    };
    assert_eq!(edited.voices[0].notes[0].pan, 0);
    assert_eq!(edited.voices[0].notes[0].velocity, 96);
    assert_eq!(edited.voices[0].notes[0].key, 60);
    let command: crate::data::Command =
        serde_json::from_value(json!({"operation":"wait","sample_end":true}))?;
    let crate::data::Command::Wait {
        milliseconds,
        from_start,
        key_off,
        sample_end,
    } = command
    else {
        unreachable!()
    };
    assert_eq!(
        (milliseconds, from_start, key_off, sample_end),
        (None, false, false, true)
    );
    Ok(())
}

#[test]
fn shared_programs_expand_current_code_and_reject_conflicting_sample_bindings() -> Result<()> {
    let (_, _, tables) = crate::package::tests::playback_data();
    let tables: Arc<[u8]> = serde_json::to_vec(&tables)?.into();
    let shared = json!({"instructions":[{"operation":"start_sample","sample":2},{"operation":"end"}],
        "samples":{"2":{"path":"shared.wav"}}});
    let definition = json!({"kind":"synthesized","programs":{"1":"audio/programs/bell.json"},
        "voices":[{"group":3,"notes":[{"program":1}]}]});
    let expand = |definition: serde_json::Value, shared: serde_json::Value| -> Result<Program> {
        let definition: Definition = serde_json::from_value(definition)?;
        let shared: Arc<[u8]> = serde_json::to_vec(&shared)?.into();
        definition.into_program(&mut |path, _| match path {
            "audio/programs/bell.json" => Ok(Arc::clone(&shared)),
            recipe::TABLES => Ok(Arc::clone(&tables)),
            _ => anyhow::bail!("missing shared program {path}"),
        })
    };
    let first = expand(definition.clone(), shared.clone())?;
    assert_eq!(first.samples[&2].path, "shared.wav");
    let mut edit = shared.clone();
    edit["instructions"][0] = json!({"operation":"wait","milliseconds":20});
    let second = expand(definition.clone(), edit)?;
    assert!(matches!(
        first.programs[&1][0],
        crate::data::Command::StartSample { sample: 2 }
    ));
    assert!(matches!(
        second.programs[&1][0],
        crate::data::Command::Wait {
            milliseconds: Some(20),
            ..
        }
    ));
    let mut duplicate = definition.clone();
    duplicate["samples"] = shared["samples"].clone();
    assert!(expand(duplicate.clone(), shared.clone()).is_ok());
    duplicate["samples"]["2"]["key"] = 48.into();
    assert!(
        format!("{:#}", expand(duplicate, shared.clone()).err().unwrap())
            .contains("conflicting sample 2")
    );
    let mut missing = definition.clone();
    missing["programs"]["1"] = "missing.json".into();
    assert!(expand(missing, shared.clone()).is_err());
    let mut recursive = shared.clone();
    recursive["instructions"] = "another-program.json".into();
    assert!(expand(definition.clone(), recursive).is_err());
    let mut malformed = shared;
    malformed["instructions"][0] = json!({"operation":"not-an-instruction"});
    assert!(expand(definition, malformed).is_err());
    Ok(())
}

fn wav(channels: u16, rate: u32, samples: &[i16]) -> Result<Arc<[u8]>> {
    let mut bytes = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(
        &mut bytes,
        hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(bytes.into_inner().into())
}
fn prepared(bytes: Arc<[u8]>, extra: serde_json::Value) -> Result<Prepared> {
    let mut definition = json!({"kind":"wave", "audio":"audio/effects/door/close.wav"});
    definition
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let definition: Definition = serde_json::from_value(definition)?;
    definition.prepare_in(
        Some([[0., 0., 1., 0., 0.]; 2]),
        &mut |path, limit| {
            assert_eq!(path, "audio/effects/door/close.wav");
            assert_eq!(limit, MAX_WAVE_BYTES);
            Ok(bytes.clone())
        },
        &mut SampleCache::default(),
    )
}
fn tick(synth: &Synthesizer, stream: &Stream, controls: LiveControls) -> Result<Option<BusFrame>> {
    stream.set_shared_controls([controls; CONTROLS_PER_BLOCK])?;
    synth.advance()?;
    Ok(stream.shared_frame())
}

#[test]
fn silent_effect_has_the_same_voice_lifetime_as_its_wav_without_a_file() -> Result<()> {
    let recorded = prepared(wav(2, SOURCE_RATE, &[0; 640])?, json!({}))?;
    let definition: Definition = serde_json::from_value(json!({
        "kind":"silence", "frames":320, "sample_rate":SOURCE_RATE
    }))?;
    let silence = definition.prepare_in(
        Some([[0., 0., 1., 0., 0.]; 2]),
        &mut |_, _| anyhow::bail!("silence must not read a file"),
        &mut SampleCache::default(),
    )?;
    let Prepared::Wave(clock) = &silence else {
        panic!("missing silence clock")
    };
    assert!(clock.pcm.is_empty());
    let original_synth = Synthesizer::default();
    let silent_synth = Synthesizer::default();
    let original = Stream::in_synthesizer(recorded, &original_synth)?;
    let silent = Stream::in_synthesizer(silence, &silent_synth)?;
    for _ in 0..640 {
        assert_eq!(
            tick(&original_synth, &original, LiveControls::default())?,
            tick(&silent_synth, &silent, LiveControls::default())?
        );
    }
    Ok(())
}

#[test]
fn stereo_recording_keeps_header_clock_and_exact_unblended_loop() -> Result<()> {
    let sound = prepared(
        wav(2, SOURCE_RATE, &[1000, -2000, 3000, -4000, 5000, -6000])?,
        json!({"loop":{"start_frame":1,"end_frame":3}}),
    )?;
    let synth = Synthesizer::default();
    let stream = Stream::in_synthesizer(sound, &synth)?;
    for expected in [
        [1000, -2000],
        [3000, -4000],
        [5000, -6000],
        [3000, -4000],
        [5000, -6000],
    ] {
        let frame = tick(&synth, &stream, LiveControls::default())?.unwrap();
        assert_eq!(frame, [expected, [0; 2], [0; 2]]);
        assert_eq!(
            stream.shared_frame(),
            Some(frame),
            "reading a queued frame twice must not advance PCM"
        );
    }
    let sound = prepared(
        wav(2, SOURCE_RATE / 2, &[1000, -1000, 3000, -3000])?,
        json!({}),
    )?;
    let synth = Synthesizer::default();
    let stream = Stream::in_synthesizer(sound, &synth)?;
    for expected in [[1000, -1000], [2000, -2000], [3000, -3000], [3000, -3000]] {
        assert_eq!(
            tick(&synth, &stream, LiveControls::default())?.unwrap()[0],
            expected
        );
    }
    assert!(tick(&synth, &stream, LiveControls::default())?.is_none());
    Ok(())
}

#[test]
fn cue_controls_pan_source_gain_group_gain_sends_and_release_remain_live() -> Result<()> {
    let sound = prepared(
        wav(1, SOURCE_RATE, &[16000; 32])?,
        json!({"loop":{"start_frame":0,"end_frame":32}, "sends":[0.5,0.25], "release_ms":1}),
    )?;
    let synth = Synthesizer::default();
    let stream = Stream::in_synthesizer(sound, &synth)?;
    let mut controls = LiveControls {
        pan: Some(0),
        source_volume: 0.5,
        volume: 0.25,
        ..Default::default()
    };
    assert_eq!(
        tick(&synth, &stream, controls)?,
        Some([[2000, 0], [4000, 0], [2000, 0]])
    );
    controls.pan = Some(127);
    assert_eq!(
        tick(&synth, &stream, controls)?,
        Some([[0, 2000], [0, 4000], [0, 2000]])
    );
    controls.mono = true;
    assert_eq!(
        tick(&synth, &stream, controls)?,
        Some([[1000; 2], [2000; 2], [1000; 2]])
    );
    controls.release = true;
    let release = (u64::from(SOURCE_RATE)).div_ceil(1000);
    let mut previous = i32::MAX;
    for _ in 0..release {
        let frame = tick(&synth, &stream, controls)?.unwrap();
        assert!(frame[0][0] <= previous);
        previous = frame[0][0];
    }
    assert!(tick(&synth, &stream, controls)?.is_none());
    Ok(())
}

#[test]
fn recordings_enter_at_queue_boundaries_and_share_effect_priority_limits() -> Result<()> {
    let sound = prepared(
        wav(2, SOURCE_RATE, &[1000; 64])?,
        json!({"loop":{"start_frame":0,"end_frame":32}}),
    )?;
    let synth = Synthesizer::default();
    synth.advance()?;
    let delayed = Stream::in_synthesizer(sound.clone(), &synth)?;
    for _ in 1..BLOCK_FRAMES {
        assert_eq!(
            tick(&synth, &delayed, LiveControls::default())?,
            Some([[0; 2]; 3])
        );
    }
    assert_eq!(
        tick(&synth, &delayed, LiveControls::default())?.unwrap()[0],
        [1000; 2]
    );
    drop(delayed);
    let synth = Synthesizer::default();
    let mut streams = Vec::new();
    for _ in 0..SOUND_EFFECT_VOICE_BUDGET {
        let stream = Stream::in_synthesizer(sound.clone(), &synth)?;
        stream.set_shared_controls([LiveControls::default(); CONTROLS_PER_BLOCK])?;
        streams.push(stream);
    }
    synth.advance()?;
    for stream in &streams {
        assert!(stream.shared_frame().is_some());
    }
    for _ in 1..BLOCK_FRAMES {
        synth.advance()?;
    }
    let important = Stream::in_synthesizer(sound, &synth)?;
    important.set_shared_controls(
        [LiveControls {
            priority: Some(256),
            ..Default::default()
        }; CONTROLS_PER_BLOCK],
    )?;
    synth.advance()?;
    assert!(important.shared_frame().is_some());
    assert_eq!(
        streams
            .iter()
            .filter(|stream| stream.shared_frame().is_some())
            .count(),
        SOUND_EFFECT_VOICE_BUDGET - 1
    );
    Ok(())
}

#[test]
fn malformed_replacements_do_not_fall_back_to_synthesis() -> Result<()> {
    let bytes = wav(1, 44100, &[1000, -1000])?;
    for extra in [
        json!({"version":0}),
        json!({"gain":-1}),
        json!({"sends":[2,0]}),
        json!({"release_ms":5001}),
        json!({"loop":{"start_frame":1,"end_frame":3}}),
        json!({"loop":{"start_frame":1,"end_frame":1}}),
        json!({"audio":"../escape.wav"}),
        json!({"unexpected":true}),
    ] {
        assert!(prepared(bytes.clone(), extra).is_err());
    }
    assert!(prepared(wav(3, 44100, &[1, 2, 3])?, json!({})).is_err());
    assert!(prepared(wav(1, 1000, &[1, 2, 3])?, json!({})).is_err());
    assert!(prepared(wav(1, 44100, &[])?, json!({})).is_err());
    let mut changed = bytes.to_vec();
    changed.truncate(changed.len() - 1);
    assert!(prepared(changed.into(), json!({})).is_err());
    assert!(
        serde_json::from_value::<Definition>(json!({"version":1,"audio":"sample.wav"})).is_err()
    );
    Ok(())
}

#[test]
fn native_programs_and_recordings_compete_for_the_same_effect_slots() -> Result<()> {
    use crate::data::{Command, Event, EventKind, Interpolation, Note, ScoreOrigin, VoiceSource};
    let sound = prepared(
        wav(2, SOURCE_RATE, &[1000; 64])?,
        json!({"loop":{"start_frame":0,"end_frame":32}}),
    )?;
    let synth = Synthesizer::default();
    let mut recordings = Vec::new();
    for _ in 0..SOUND_EFFECT_VOICE_BUDGET {
        let stream = Stream::in_synthesizer(sound.clone(), &synth)?;
        stream.set_shared_controls([LiveControls::default(); CONTROLS_PER_BLOCK])?;
        recordings.push(stream);
    }
    synth.advance()?;
    for recording in &recordings {
        assert!(recording.shared_frame().is_some());
    }
    for _ in 1..BLOCK_FRAMES {
        synth.advance()?;
    }
    let (mut resources, mut score, tables) = crate::package::tests::playback_data();
    resources.programs.insert(
        1,
        vec![
            Command::Interpolation {
                mode: Interpolation::Direct,
                coefficients: 0,
            },
            Command::StartSample { sample: 2 },
            Command::Wait {
                milliseconds: None,
                from_start: false,
                key_off: false,
                sample_end: false,
            },
        ],
    );
    score.origin = ScoreOrigin::SoundEffect;
    score.first_events = vec![Event {
        tick: 0,
        channel: 0,
        kind: EventKind::Notes {
            source: VoiceSource::SoundEffect { id: 1 },
            length: u16::MAX,
            voices: vec![Note {
                macro_id: 1,
                key: 60,
                velocity: 127,
                pan: 64,
                priority: 0,
                max_voices: SOUND_EFFECT_VOICE_BUDGET as u8,
            }],
        },
    }];
    let native = Stream::in_synthesizer(
        Prepared::from(Loaded::new(
            resources,
            score,
            tables,
            [[0., 0., 1., 0., 0.]; 2],
        )?),
        &synth,
    )?;
    native.set_shared_controls(
        [LiveControls {
            priority: Some(256),
            ..Default::default()
        }; CONTROLS_PER_BLOCK],
    )?;
    synth.advance()?;
    assert!(native.shared_frame().is_some());
    assert_eq!(
        recordings
            .iter()
            .filter(|stream| stream.shared_frame().is_some())
            .count(),
        SOUND_EFFECT_VOICE_BUDGET - 1
    );
    Ok(())
}

#[test]
fn plain_wav_mapping_needs_no_descriptor() -> Result<()> {
    let bytes = wav(2, SOURCE_RATE, &[123, -456, 789, -987])?;
    let mut reads = vec![];
    let definition = Definition::read("audio/effects/click.wav", &mut |_, _| {
        anyhow::bail!("a WAV mapping must not read a JSON descriptor")
    })?;
    let prepared = definition.prepare_in(
        Some([[0., 0., 1., 0., 0.]; 2]),
        &mut |path, _| {
            reads.push(path.to_owned());
            Ok(bytes.clone())
        },
        &mut SampleCache::default(),
    )?;
    assert!(matches!(prepared, Prepared::Wave(_)));
    assert_eq!(reads, ["audio/effects/click.wav"]);
    Ok(())
}
