use super::*;
use resonance_content::{
    battle_audio::{self, document as battle},
    diagnostics::Diagnostics,
    library::Library,
};

fn write(root: &Path, path: &str, value: &impl Serialize) -> Result<()> {
    fs::create_dir_all(root.join(path).parent().unwrap())?;
    Ok(fs::write(
        root.join(path),
        serde_json::to_vec_pretty(value)?,
    )?)
}

#[test]
fn battle_audio_catalogue_discovers_mappings_and_preserves_stream_scope() -> Result<()> {
    let root = tempfile::tempdir()?;
    let definition = battle::Definition {
        version: battle::VERSION,
        music: "audio/music/mappings.json".into(),
        effects: "audio/effects/mappings.json".into(),
        streams: Some(65536),
        archive_playback: [(
            65536,
            Playback {
                sample_rate: Some(32028),
            },
        )]
        .into(),
        voice_playback: BTreeMap::new(),
        music_reverbs: resonance_content::field_audio::MusicReverbs {
            presets: [[0.5, 0.5, 1., 0.5, 0.]; 2],
            selectors: vec![0; 10],
        },
        voice_gains: (0..128).map(|v| v as f32 / 127.).collect(),
        reverbs: [[0.5, 0.5, 1., 0.5, 0.]; 2],
        effect_spatial: [320., 5., 64., 0., 127.],
        voice_spatial: [320., 5., 64., 24., 104.],
    };
    write(
        root.path(),
        "content.json",
        &serde_json::json!({"version":1,"game":"GQSEAF","revision":0}),
    )?;
    write(root.path(), battle::PATH, &definition)?;
    for (path, mapping) in [
        (&*definition.music, serde_json::json!({"7":"song"})),
        (&*definition.effects, serde_json::json!({"3":"cue"})),
        (
            document::SPEECH,
            serde_json::json!({"65536":"speech-a","131072":"speech-b"}),
        ),
        (
            "audio/speech/speech-a/mappings.json",
            serde_json::json!({"2":"first.wav"}),
        ),
        (
            "audio/speech/speech-b/mappings.json",
            serde_json::json!({"2":"second.wav"}),
        ),
    ] {
        write(root.path(), path, &mapping)?;
    }
    let library = Library::open(root.path(), &[])?;
    let mut snapshot = library.snapshot();
    let original = battle::Catalogue::capture(&mut snapshot, &|| false)?;
    assert!(snapshot.digest("audio/music/song/music.json").is_err());
    assert_eq!(
        original.resolved.voices[&2].path,
        original.resolved.voices[&65538].path
    );
    assert_ne!(
        original.resolved.voices[&2].path,
        original.resolved.voices[&131074].path
    );
    assert_eq!(original.resolved.voices[&2].sample_rate, Some(32028));
    let mut selected = battle_audio::Selection::default();
    use resonance_content::battle_voice::Sound;
    let diagnostics = Diagnostics::new(true);
    original.collect(&mut selected, &diagnostics, Sound::Cue(3))?;
    original.collect(&mut selected, &diagnostics, Sound::Stream(2))?;
    selected.voices.insert(65538);
    let choice = original.select(&selected, &diagnostics)?;
    assert_eq!(choice.effects.keys().copied().collect::<Vec<_>>(), [3]);
    assert_eq!(
        choice.voices.keys().copied().collect::<Vec<_>>(),
        [2, 65538]
    );
    assert!(
        original
            .collect(&mut selected, &diagnostics, Sound::Cue(4))
            .is_err()
    );
    assert!(
        original
            .collect(&mut selected, &Diagnostics::default(), Sound::Cue(4))?
            .is_none()
    );
    write(
        root.path(),
        &definition.effects,
        &serde_json::json!({"3":"cue","4":"added"}),
    )?;
    write(
        root.path(),
        "audio/speech/speech-a/mappings.json",
        &serde_json::json!({"2":"replacement.wav"}),
    )?;
    let fresh = battle::Catalogue::capture(&mut library.snapshot(), &|| false)?;
    fresh.collect(&mut selected, &diagnostics, Sound::Cue(4))?;
    assert_eq!(
        fresh.resolved.voices[&2].path,
        "audio/speech/speech-a/replacement.wav"
    );
    assert_eq!(
        battle::Catalogue::capture(&mut snapshot, &|| false)?
            .resolved
            .voices[&2]
            .path,
        original.resolved.voices[&2].path
    );
    write(
        root.path(),
        &definition.music,
        &serde_json::json!({"7":"song", "8":"shared"}),
    )?;
    let mut scoped = library.snapshot();
    let current = battle::Catalogue::capture(&mut scoped, &|| false)?;
    assert_eq!(current.resolved.music[&7], "audio/music/song/music.json");
    assert_eq!(current.resolved.music[&8], "audio/music/shared/music.json");
    assert_eq!(original.resolved.music[&7], "audio/music/song/music.json");
    write(
        root.path(),
        &definition.music,
        &serde_json::json!({"7":"changed"}),
    )?;
    assert_eq!(
        battle::Catalogue::capture(&mut scoped, &|| false)?
            .resolved
            .music[&7],
        current.resolved.music[&7]
    );
    assert_eq!(
        battle::Catalogue::capture(&mut library.snapshot(), &|| false)?
            .resolved
            .music[&7],
        "audio/music/changed/music.json"
    );
    let mut invalid = serde_json::to_value(&definition)?;
    invalid["files"] = serde_json::json!({});
    write(root.path(), battle::PATH, &invalid)?;
    assert!(battle::Catalogue::capture(&mut library.snapshot(), &|| false).is_err());
    assert!(battle::Catalogue::capture(&mut snapshot, &|| true).is_err());
    Ok(())
}
