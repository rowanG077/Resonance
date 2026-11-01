use super::*;
use resonance_content::{
    field_audio::document,
    library::{Library, Mod},
};
use serde_json::json;

pub(super) fn wav(values: &[i16], rate: u32) -> Result<Vec<u8>> {
    let mut bytes = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(
        &mut bytes,
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for &value in values {
        writer.write_sample(value)?;
    }
    writer.finalize()?;
    Ok(bytes.into_inner())
}

pub(super) fn looped_wav(values: &[i16], rate: u32) -> Result<Vec<u8>> {
    let mut bytes = Cursor::new(wav(values, rate)?);
    resonance_asset_writer::wav::append_loop(&mut bytes, rate, 0..values.len() as u32)?;
    Ok(bytes.into_inner())
}

pub(super) fn write(root: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, bytes)?;
    Ok(())
}

pub(super) fn fixture() -> Result<tempfile::TempDir> {
    let root = tempfile::tempdir()?;
    let (resources, score, tables) =
        test_score_data(resonance_audio::data::ScoreOrigin::SoundEffect);
    let program = resonance_audio::package::editable::Program {
        programs: resources.programs,
        samples: BTreeMap::from([(
            1,
            resonance_audio::package::editable::Sample {
                silence: None,
                path: "audio/instruments/test.wav".into(),
                key: 60,
                loop_start: 0,
                loop_length: 0,
            },
        )]),
        score,
        tables,
        reverbs: None,
    };
    for (path, value) in [
        (
            resonance_audio::effect::recipe::TABLES,
            serde_json::to_value(&program.tables)?,
        ),
        (
            "content.json",
            serde_json::to_value(resonance_content::library::Descriptor {
                version: resonance_content::library::VERSION,
                game: resonance_content::library::GAME.into(),
            })?,
        ),
        ("fields/mappings.json", json!({"330":"iselia"})),
        (
            "fields/iselia/field.json",
            json!({
                "script":"fields/iselia/scenario.ssb", "messages":"fields/iselia/messages.json",
                "scene":"fields/iselia/scene.json", "collision":"fields/iselia/collision.json",
                "actors":"fields/iselia/actors.json", "audio":{
                    "music":[7], "effects":[3],
                    "speech":[{"ids":[65543],"sample_rate":32028},{"ids":[131079]}]
                }, "movies":[]
            }),
        ),
        ("audio/music/mappings.json", json!({"7":"test-song.wav"})),
        ("audio/mappings.json", json!({"3":"effects/test-cue.json"})),
        (
            "audio/effects/test-cue.json",
            serde_json::to_value(resonance_audio::effect::Definition::Synthesized(
                resonance_audio::effect::Recipe::from_program(&program)?,
            ))?,
        ),
        (
            document::SPEECH,
            json!({"7":"archive-a/line.wav", "65543":"archive-a/line.wav", "131079":"archive-b/line.wav"}),
        ),
        (
            resonance_content::field_audio::mix::PATH,
            json!({
                "reverbs": {
                    "field":{"coloration":0.5,"mix":0.5,"decay_seconds":1.,"damping":0.5,"predelay_seconds":0.},
                    "battle":{"coloration":0.5,"mix":0.5,"decay_seconds":2.,"damping":0.5,"predelay_seconds":0.}
                },
                "music_reverbs":{"keep_current":[0,1,2,3,4,5,6,7]},
                "field_reverbs":{"auxiliary_a":"field","auxiliary_b":"field"},
                "battle_reverbs":{"auxiliary_a":"battle","auxiliary_b":"field"},
                "voice_gains":(0..128).map(|value| value as f32 / 127.).collect::<Vec<_>>()
            }),
        ),
    ] {
        write(root.path(), path, &serde_json::to_vec_pretty(&value)?)?;
    }
    write(
        root.path(),
        "audio/music/test-song.wav",
        &looped_wav(&[16384; 128], RATE)?,
    )?;
    write(
        root.path(),
        "audio/instruments/test.wav",
        &wav(&[16384], RATE)?,
    )?;
    write(
        root.path(),
        "audio/speech/archive-a/line.wav",
        &wav(&[12000], RATE)?,
    )?;
    write(
        root.path(),
        "audio/speech/archive-b/line.wav",
        &wav(&[6000], RATE)?,
    )?;
    Ok(root)
}

// The tests prepare the same field owner used by the real loading pipeline.
impl Cache {
    pub(super) fn capture_test_field(
        &mut self,
        path: &str,
        snapshot: &mut resonance_content::library::Snapshot,
        diagnostics: Diagnostics,
        cancelled: impl Fn() -> bool,
    ) -> Result<Arc<Assets>> {
        ensure!(!cancelled(), "audio preparation cancelled");
        let field: resonance_content::field::document::Definition =
            snapshot.json(path, resonance_content::preparation::JSON_LIMIT)?;
        self.capture_named(&field.audio, snapshot, diagnostics, cancelled)
    }
}

pub(super) fn read_choices(root: &Path) -> Result<document::Definition> {
    let field: resonance_content::field::document::Definition =
        serde_json::from_slice(&fs::read(root.join("fields/iselia/field.json"))?)?;
    Ok(field.audio)
}

pub(super) fn write_choices(root: &Path, audio: &document::Definition) -> Result<()> {
    let path = root.join("fields/iselia/field.json");
    let mut field: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    field["audio"] = serde_json::to_value(audio)?;
    fs::write(path, serde_json::to_vec(&field)?)?;
    Ok(())
}

#[test]
fn inline_speech_edits_change_only_selected_clocks_and_reject_ambiguity() -> Result<()> {
    let root = fixture()?;
    let mut choices = document::Definition {
        speech: vec![document::Speech {
            ids: [65543, 131079].into(),
            sample_rate: Some(24000),
        }],
        ..Default::default()
    };
    write_choices(root.path(), &choices)?;
    let library = Library::open(root.path(), &[])?;
    let mut pinned = library.snapshot();
    let mut cache = Cache::default();
    let before = cache.capture_test_field(
        "fields/iselia/field.json",
        &mut pinned,
        Diagnostics::new(true),
        || false,
    )?;
    assert!(before.recordings.is_empty() && before.sounds.is_empty());
    assert_eq!(
        before.voices.keys().copied().collect::<Vec<_>>(),
        [65543, 131079]
    );
    assert!(before.voices.values().all(|voice| voice.rate == 24000));
    choices.speech = vec![document::Speech {
        ids: [131079].into(),
        sample_rate: None,
    }];
    write_choices(root.path(), &choices)?;
    let after = cache.capture_test_field(
        "fields/iselia/field.json",
        &mut library.snapshot(),
        Diagnostics::new(true),
        || false,
    )?;
    assert_eq!(after.voices.keys().copied().collect::<Vec<_>>(), [131079]);
    assert_eq!(after.voices[&131079].rate, RATE);
    assert_eq!(
        cache
            .capture_test_field(
                "fields/iselia/field.json",
                &mut pinned,
                Diagnostics::new(true),
                || false
            )?
            .voices[&131079]
            .rate,
        24000
    );
    choices.speech.push(document::Speech {
        ids: [131079].into(),
        sample_rate: Some(32028),
    });
    assert!(
        choices
            .resolve(&mut library.snapshot())
            .err()
            .context("duplicate voice was accepted")?
            .to_string()
            .contains("duplicate speech binding")
    );
    choices.speech.pop();
    choices.speech[0].sample_rate = Some(0);
    assert!(choices.resolve(&mut library.snapshot()).is_err());
    Ok(())
}

#[test]
fn silent_speech_keeps_timing_and_a_mod_can_replace_it_with_a_wav() -> Result<()> {
    let root = fixture()?;
    write(
        root.path(),
        document::SPEECH,
        &serde_json::to_vec(&json!({
            "65543":{"name":"archive-a/pause", "silence":{"frames":64000,"sample_rate":32000,"channels":1}},
            "131079":"archive-b/line.wav"
        }))?,
    )?;
    fs::remove_file(root.path().join("audio/speech/archive-a/line.wav"))?;
    let diagnostics = Diagnostics::default();
    let base = Library::open(root.path(), &[])?;
    let assets = Cache::default().capture_test_field(
        "fields/iselia/field.json",
        &mut base.snapshot(),
        diagnostics.clone(),
        || false,
    )?;
    let pause = &assets.voices[&65543];
    assert!(pause.bytes.is_empty());
    assert_eq!((pause.sample_count(), pause.rate), (64000, 32028));
    assert_eq!(pause.sample(63999, 1, [1.; 2]), Some([0.; 2]));
    assert_eq!(pause.sample(64000, 1, [1.; 2]), None);
    assert!(diagnostics.entries().is_empty());

    let dub = tempfile::tempdir()?;
    write(dub.path(), document::SPEECH, br#"{"65543":"dub.wav"}"#)?;
    write(
        dub.path(),
        "audio/speech/dub.wav",
        &wav(&[12000; 8], 32000)?,
    )?;
    let modified = Library::open(
        root.path(),
        &[Mod {
            name: "dub".into(),
            root: dub.path().into(),
        }],
    )?;
    let assets = Cache::default().capture_test_field(
        "fields/iselia/field.json",
        &mut modified.snapshot(),
        diagnostics.clone(),
        || false,
    )?;
    assert_eq!(assets.voices[&65543].sample_count(), 8);
    assert_eq!(assets.voices[&65543].value(0), 12000);
    assert_eq!(assets.voices[&65543].rate, 32028);
    assert_eq!(assets.voices[&131079].value(0), 6000);
    assert!(diagnostics.entries().is_empty());
    Ok(())
}

#[test]
fn missing_named_media_remains_isolated_and_capture_cancellation_is_fatal() -> Result<()> {
    let root = fixture()?;
    let library = Library::open(root.path(), &[])?;
    fs::remove_file(root.path().join("audio/speech/archive-a/line.wav"))?;
    let diagnostics = Diagnostics::default();
    let assets = Cache::default().capture_test_field(
        "fields/iselia/field.json",
        &mut library.snapshot(),
        diagnostics.clone(),
        || false,
    )?;
    assert!(assets.recordings.contains_key(&7));
    assert!(!assets.voices.contains_key(&65543));
    assert!(assets.voices.contains_key(&131079));
    assert_eq!(diagnostics.entries().len(), 1);
    let checks = std::cell::Cell::new(0);
    let diagnostics = Diagnostics::default();
    let result = Cache::default().capture_test_field(
        "fields/iselia/field.json",
        &mut library.snapshot(),
        diagnostics.clone(),
        || {
            checks.set(checks.get() + 1);
            checks.get() >= 3
        },
    );
    let error = result.err().context("cancellation was swallowed")?;
    assert!(format!("{error:#}").contains("cancelled"));
    assert!(diagnostics.entries().is_empty());
    Ok(())
}

#[test]
fn named_skit_audio_joins_the_field_bank_and_uses_current_pcm() -> Result<()> {
    use resonance_content::skit::document as skits;
    let root = fixture()?;
    for (path, value) in [
        (
            skits::PATH,
            json!({
                "skits":[], "preview_order":[],
                "media":{"ids":[42,43],"speed":1.000875}
            }),
        ),
        (
            skits::portraits::PATH,
            json!({"mappings":{},"images":{},"animations":{}}),
        ),
        (skits::MAPPINGS, json!({})),
        (
            document::SPEECH,
            json!({
                "42":"skits/arrival.wav",
                "43":{"name":"skits/arrival/silence", "silence":{"frames":2400,"sample_rate":24000,"channels":1}},
                "65543":"archive-a/line.wav", "131079":"archive-b/line.wav"
            }),
        ),
    ] {
        write(root.path(), path, &serde_json::to_vec(&value)?)?;
    }
    let path = "audio/speech/skits/arrival.wav";
    write(root.path(), path, &wav(&[15000, 7500], 24000)?)?;
    let library = Library::open(root.path(), &[])?;
    let mut cache = Cache::default();
    let prepare = |cache: &mut Cache| -> Result<Arc<Assets>> {
        let mut snapshot = library.snapshot();
        let mut assets = cache.capture_test_field(
            "fields/iselia/field.json",
            &mut snapshot,
            Diagnostics::new(true),
            || false,
        )?;
        let skits = skits::capture(&mut snapshot, &|| false)?;
        Arc::make_mut(&mut assets).capture_skits(&skits, &mut snapshot, &|| false)?;
        Ok(assets)
    };
    let first = prepare(&mut cache)?;
    assert_eq!(first.voices[&42].value(0), 15000);
    assert_eq!(first.voices[&42].rate, 24021);
    assert!(!first.voices.contains_key(&43));
    assert!(first.voices.contains_key(&65543));
    write(root.path(), path, &wav(&[4000, 2000, 1000], 48000)?)?;
    let edited = prepare(&mut cache)?;
    assert_eq!(edited.voices[&42].value(0), 4000);
    assert_eq!(edited.voices[&42].sample_count(), 3);
    assert_eq!(edited.voices[&42].rate, 48042);
    assert_eq!(first.voices[&42].value(0), 15000);
    assert_eq!(first.voices[&65543].bytes, edited.voices[&65543].bytes);
    let diagnostics = Diagnostics::default();
    let mut snapshot = library.snapshot();
    let skits = skits::capture(&mut snapshot, &|| false)?;
    let mut assets = Arc::new(Assets::silent(diagnostics.clone()));
    assert!(
        Arc::make_mut(&mut assets)
            .capture_skits(&skits, &mut snapshot, &|| true)
            .is_err()
    );
    assert!(diagnostics.entries().is_empty());
    Ok(())
}

#[test]
fn named_audio_edits_rates_lengths_and_scoped_mapping_mods_reach_playback() -> Result<()> {
    let root = fixture()?;
    let library = Library::open(root.path(), &[])?;
    let mut cache = Cache::default();
    let prepare = |cache: &mut Cache, library: &Library| {
        cache.capture_test_field(
            "fields/iselia/field.json",
            &mut library.snapshot(),
            Diagnostics::new(true),
            || false,
        )
    };
    let before = prepare(&mut cache, &library)?;
    let same = prepare(&mut cache, &library)?;
    assert_eq!(
        before.sounds[&3].program().unwrap().resources().samples[&1].pcm,
        same.sounds[&3].program().unwrap().resources().samples[&1].pcm
    );
    assert_eq!(before.voices[&65543].value(0), 12000);
    assert_eq!(before.voices[&131079].value(0), 6000);
    assert_eq!(before.voices[&65543].rate, 32028);
    write(
        root.path(),
        "audio/instruments/test.wav",
        &wav(&[8192, 4096], 22050)?,
    )?;
    write(
        root.path(),
        "audio/speech/archive-a/line.wav",
        &wav(&[5000, 4000, 3000], 22050)?,
    )?;
    let edited = prepare(&mut cache, &library)?;
    assert_eq!(
        edited.sounds[&3].program().unwrap().resources().samples[&1].pcm,
        [8192, 4096]
    );
    assert_eq!(
        edited.sounds[&3].program().unwrap().resources().samples[&1].rate,
        22050
    );
    assert_eq!(
        before.sounds[&3].program().unwrap().resources().samples[&1].pcm,
        [16384]
    );
    assert!(!Arc::ptr_eq(&before.sounds[&3], &edited.sounds[&3]));
    assert_eq!(edited.voices[&65543].sample_count(), 3);
    assert_eq!(edited.voices[&65543].rate, 32028);
    assert_eq!(before.voices[&131079].bytes, edited.voices[&131079].bytes);

    let overlay = tempfile::tempdir()?;
    write(
        overlay.path(),
        "audio/music/mappings.json",
        br#"{"7":"replacement.wav"}"#,
    )?;
    write(
        overlay.path(),
        "audio/music/replacement.wav",
        &wav(&[4000; 64], 48000)?,
    )?;
    let modded = Library::open(
        root.path(),
        &[Mod {
            name: "remix".into(),
            root: overlay.path().into(),
        }],
    )?;
    let mut snapshot = modded.snapshot();
    let changed = cache.capture_test_field(
        "fields/iselia/field.json",
        &mut snapshot,
        Diagnostics::new(true),
        || false,
    )?;
    let first = changed.recordings[&7].start()?.next_frame()?.unwrap();
    assert_eq!(first, [4000. * std::f32::consts::FRAC_1_SQRT_2; 2]);
    assert!(
        snapshot
            .overrides()
            .iter()
            .any(|entry| entry.path == "audio/music/mappings.json" && entry.id == Some(7))
    );
    assert_eq!(
        prepare(&mut cache, &library)?.recordings[&7]
            .start()?
            .next_frame()?
            .unwrap(),
        [16384. * std::f32::consts::FRAC_1_SQRT_2; 2]
    );
    assert!(
        cache
            .capture_test_field(
                "fields/iselia/field.json",
                &mut library.snapshot(),
                Diagnostics::default(),
                || true
            )
            .is_err()
    );

    // Exercise the public recorder's named field lookup and the actual mixer.
    let output = root.path().join("recording.wav");
    record_field_audio(
        root.path(),
        330,
        &output,
        320,
        &[(
            0,
            AudioCommand::Voice {
                resource: 65543,
                completion: None,
            },
        )],
        true,
        [127; 3],
    )?;
    let mut recording = hound::WavReader::open(output)?;
    assert!(
        recording
            .samples::<i16>()
            .any(|value| value.is_ok_and(|value| value != 0))
    );
    fs::remove_file(root.path().join("audio/instruments/test.wav"))?;
    let failure = prepare(&mut cache, &library)
        .err()
        .context("missing sample was hidden by a warm cache")?;
    assert!(format!("{failure:#}").contains("audio/instruments/test.wav"));
    Ok(())
}

#[test]
fn ordinary_wav_effect_mods_reach_field_cues_and_keep_prepared_snapshots() -> Result<()> {
    let root = fixture()?;
    let overlay = tempfile::tempdir()?;
    write(
        overlay.path(),
        "audio/mappings.json",
        br#"{"3":"effects/door-close.wav"}"#,
    )?;
    write(
        overlay.path(),
        "audio/effects/door-close.wav",
        &wav(&[10000; 64], RATE)?,
    )?;
    let library = Library::open(
        root.path(),
        &[Mod {
            name: "door-recording".into(),
            root: overlay.path().into(),
        }],
    )?;
    let mut cache = Cache::default();
    let mut snapshot = library.snapshot();
    let before = cache.capture_test_field(
        "fields/iselia/field.json",
        &mut snapshot,
        Diagnostics::new(true),
        || false,
    )?;
    assert!(before.sounds[&3].program().is_none());
    let render = |assets: &Arc<Assets>| -> Result<[f32; 2]> {
        let (audio, _control) = assets.clone().session();
        let mut frames = audio.decoder();
        frames.command(AudioCommand::Sound {
            id: 3,
            pan: 0,
            volume: 127,
            slot: Some(1),
        })?;
        frames
            .frame()?
            .context("recording effect did not enter field playback")
    };
    assert!(render(&before)?[0] > 0.);
    write(
        overlay.path(),
        "audio/effects/door-close.wav",
        &wav(&[-8000; 96], 48000)?,
    )?;
    let pinned = cache.capture_test_field(
        "fields/iselia/field.json",
        &mut snapshot,
        Diagnostics::new(true),
        || false,
    )?;
    assert_eq!(render(&before)?, render(&pinned)?);
    let edited = cache.capture_test_field(
        "fields/iselia/field.json",
        &mut library.snapshot(),
        Diagnostics::new(true),
        || false,
    )?;
    assert!(!Arc::ptr_eq(&before.sounds[&3], &edited.sounds[&3]));
    assert!(render(&edited)?[0] < 0.);
    assert!(
        render(&before)?[0] > 0.,
        "already prepared playback must retain its captured WAV"
    );
    fs::remove_file(overlay.path().join("audio/effects/door-close.wav"))?;
    let error = cache
        .capture_test_field(
            "fields/iselia/field.json",
            &mut library.snapshot(),
            Diagnostics::new(true),
            || false,
        )
        .err()
        .context("warm recorded effect cache hid a missing file")?;
    assert!(format!("{error:#}").contains("audio/effects/door-close.wav"));
    Ok(())
}
