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
        version: resonance_audio::package::editable::VERSION,
        programs: resources.programs,
        samples: BTreeMap::from([(
            1,
            resonance_audio::package::editable::Sample {
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
            "content.json",
            json!({"version":1, "game":"GQSEAF", "revision":0}),
        ),
        ("fields/mappings.json", json!({"330":"iselia"})),
        (
            "fields/iselia/field.json",
            json!({
                "version":1, "script":"fields/iselia/scenario.ssb", "messages":"fields/iselia/messages.json",
                "scene":"fields/iselia/scene.json", "collision":"fields/iselia/collision.json",
                "actors":"fields/iselia/actors.json", "audio":"fields/iselia/audio.json", "movies":[]
            }),
        ),
        ("audio/music/mappings.json", json!({"7":"test-song"})),
        ("audio/effects/mappings.json", json!({"3":"test-cue"})),
        (
            "audio/music/test-song/music.json",
            json!({"version":1, "audio":"audio/music/test-song/music.wav", "loop":{"start_frame":0,"end_frame":128}}),
        ),
        (
            "audio/effects/test-cue/sound.json",
            serde_json::to_value(resonance_audio::effect::Definition::Synthesized(
                program.clone(),
            ))?,
        ),
        (
            document::SPEECH,
            json!({"65536":"archive-a", "131072":"archive-b"}),
        ),
        (
            "audio/speech/archive-a/mappings.json",
            json!({"7":"line.wav"}),
        ),
        (
            "audio/speech/archive-b/mappings.json",
            json!({"7":"line.wav"}),
        ),
        (
            "fields/iselia/audio.json",
            json!({
                "version":1, "music":[{"mapping":"audio/music/mappings.json","ids":[7]}], "effects":[{"mapping":"audio/effects/mappings.json","ids":[3]}],
                "speech":[
                    {"archive":65536,"members":{"7":{"sample_rate":32028}}},
                    {"archive":131072,"members":{"7":{}}}
                ],
                "music_reverbs":{"presets":[[0.5,0.5,1.,0.5,0.],[0.5,0.5,1.,0.5,0.]],"selectors":vec![0;8]},
                "reverbs":[[0.5,0.5,1.,0.5,0.],[0.5,0.5,1.,0.5,0.]],
                "voice_gains":(0..128).map(|value| value as f32 / 127.).collect::<Vec<_>>()
            }),
        ),
    ] {
        write(root.path(), path, &serde_json::to_vec_pretty(&value)?)?;
    }
    write(
        root.path(),
        "audio/music/test-song/music.wav",
        &wav(&[16384; 128], RATE)?,
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

#[test]
fn missing_named_media_remains_isolated_and_capture_cancellation_is_fatal() -> Result<()> {
    let root = fixture()?;
    let library = Library::open(root.path(), &[])?;
    fs::remove_file(root.path().join("audio/speech/archive-a/line.wav"))?;
    let diagnostics = Diagnostics::default();
    let assets = Cache::default().capture_named(
        "fields/iselia/audio.json",
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
    let result = Cache::default().capture_named(
        "fields/iselia/audio.json",
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
                "version":1, "skits":[], "preview_order":[], "portraits":{}, "portrait_recipes":[],
                "media":{"42":{"kind":"speech"},"43":{"kind":"silence","frames":2400,"sample_rate":24000}}
            }),
        ),
        (skits::MAPPINGS, json!({})),
        (skits::SPEECH_MAPPINGS, json!({"42":"arrival.wav"})),
    ] {
        write(root.path(), path, &serde_json::to_vec(&value)?)?;
    }
    let path = "audio/speech/skits/arrival.wav";
    write(root.path(), path, &wav(&[15000, 7500], 24000)?)?;
    let library = Library::open(root.path(), &[])?;
    let mut cache = Cache::default();
    let prepare = |cache: &mut Cache| -> Result<Arc<Assets>> {
        let mut snapshot = library.snapshot();
        let mut assets = cache.capture_named(
            "fields/iselia/audio.json",
            &mut snapshot,
            Diagnostics::new(true),
            || false,
        )?;
        let skits = skits::capture(&mut snapshot, &|| false)?;
        cache.capture_skits(&mut assets, &skits, &mut snapshot, &|| false)?;
        Ok(assets)
    };
    let first = prepare(&mut cache)?;
    assert_eq!(first.voices[&42].value(0), 15000);
    assert_eq!(first.voices[&42].rate, 24000);
    assert!(!first.voices.contains_key(&43));
    assert!(first.voices.contains_key(&65543));
    write(root.path(), path, &wav(&[4000, 2000, 1000], 48000)?)?;
    let edited = prepare(&mut cache)?;
    assert_eq!(edited.voices[&42].value(0), 4000);
    assert_eq!(edited.voices[&42].sample_count(), 3);
    assert_eq!(edited.voices[&42].rate, 48000);
    assert_eq!(first.voices[&42].value(0), 15000);
    assert!(Arc::ptr_eq(&first.voices[&65543], &edited.voices[&65543]));
    let diagnostics = Diagnostics::default();
    let mut snapshot = library.snapshot();
    let skits = skits::capture(&mut snapshot, &|| false)?;
    let mut assets = Arc::new(Assets::silent(diagnostics.clone()));
    assert!(
        cache
            .capture_skits(&mut assets, &skits, &mut snapshot, &|| true)
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
        cache.capture_named(
            "fields/iselia/audio.json",
            &mut library.snapshot(),
            Diagnostics::new(true),
            || false,
        )
    };
    let before = prepare(&mut cache, &library)?;
    let same = prepare(&mut cache, &library)?;
    assert!(Arc::ptr_eq(&before.sounds[&3], &same.sounds[&3]));
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
    assert!(Arc::ptr_eq(
        &before.voices[&131079],
        &edited.voices[&131079]
    ));

    let overlay = tempfile::tempdir()?;
    let replacement = json!({"version":1,"audio":"audio/music/replacement/music.wav"});
    write(
        overlay.path(),
        "audio/music/mappings.json",
        br#"{"7":"replacement"}"#,
    )?;
    write(
        overlay.path(),
        "audio/music/replacement/music.json",
        &serde_json::to_vec(&replacement)?,
    )?;
    write(
        overlay.path(),
        "audio/music/replacement/music.wav",
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
    let changed = cache.capture_named(
        "fields/iselia/audio.json",
        &mut snapshot,
        Diagnostics::new(true),
        || false,
    )?;
    let first = changed.recordings[&7].start(false)?.next_frame()?.unwrap();
    assert_eq!(first, [4000. * std::f32::consts::FRAC_1_SQRT_2; 2]);
    assert!(
        snapshot
            .overrides()
            .iter()
            .any(|entry| entry.path == "audio/music/mappings.json" && entry.id == Some(7))
    );
    assert_eq!(
        prepare(&mut cache, &library)?.recordings[&7]
            .start(false)?
            .next_frame()?
            .unwrap(),
        [16384. * std::f32::consts::FRAC_1_SQRT_2; 2]
    );
    assert!(
        cache
            .capture_named(
                "fields/iselia/audio.json",
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
        "audio/effects/mappings.json",
        br#"{"3":"door"}"#,
    )?;
    write(
        overlay.path(),
        "audio/effects/door/sound.json",
        br#"{
        "kind":"wave", "version":1, "audio":"audio/effects/door/close.wav"
    }"#,
    )?;
    write(
        overlay.path(),
        "audio/effects/door/close.wav",
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
    let before = cache.capture_named(
        "fields/iselia/audio.json",
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
        "audio/effects/door/close.wav",
        &wav(&[-8000; 96], 48000)?,
    )?;
    let pinned = cache.capture_named(
        "fields/iselia/audio.json",
        &mut snapshot,
        Diagnostics::new(true),
        || false,
    )?;
    assert!(Arc::ptr_eq(&before.sounds[&3], &pinned.sounds[&3]));
    let edited = cache.capture_named(
        "fields/iselia/audio.json",
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
    fs::remove_file(overlay.path().join("audio/effects/door/close.wav"))?;
    let error = cache
        .capture_named(
            "fields/iselia/audio.json",
            &mut library.snapshot(),
            Diagnostics::new(true),
            || false,
        )
        .err()
        .context("warm recorded effect cache hid a missing file")?;
    assert!(format!("{error:#}").contains("audio/effects/door/close.wav"));
    Ok(())
}
