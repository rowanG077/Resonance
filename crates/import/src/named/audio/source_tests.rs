use super::*;
use resonance_audio::package::SampleCache;
use resonance_content::library::Library;

#[test]
#[ignore = "requires original discs; publishes and checks all native effects without a staged cook"]
fn native_effects_publish_directly_and_inherit_the_selected_studio() -> Result<()> {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let empty = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(empty.path(), output.path())?.with_audio_source(
        AudioSource::read(&extracted.join("disc1"), Some(&extracted.join("disc2")))?,
    );
    let ids = publisher
        .audio_source()?
        .effects
        .keys()
        .copied()
        .map(u16::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    assert!(!ids.is_empty());
    publisher.sound_catalogue()?;
    let publications = publisher.published.len();
    publisher.sound_catalogue()?;
    assert_eq!(publisher.published.len(), publications);
    publisher.finish_audio()?;
    publisher.descriptor("GQSEAF", 0)?;
    let mut snapshot = Library::open(output.path(), &[])?.snapshot();
    let playable: Vec<_> = snapshot
        .mappings("audio/effects/mappings.json")?
        .iter()
        .map(|(id, _)| u16::try_from(id))
        .collect::<Result<_, _>>()?;
    let coverage: serde_json::Value = snapshot.json("reports/audio-effects.json", 1024 * 1024)?;
    let unavailable = coverage["unavailable_source_dependencies"]
        .as_object()
        .unwrap();
    assert_eq!(ids.len(), playable.len() + unavailable.len());
    assert_eq!(coverage["declared"], serde_json::json!(ids.len()));
    assert_eq!(coverage["published"], serde_json::json!(playable.len()));
    for id in unavailable.keys() {
        let id = id.parse::<u16>()?;
        let source = publisher.audio_source()?;
        let error = source
            .prepare_sound(id, source.field_reverbs)
            .err()
            .context("unavailable sound compiled")?;
        assert!(source.original.unavailable_dependency(&error)?.is_some());
        assert!(publisher.sound_programs([id]).is_err());
    }
    let source = publisher.audio_source()?;
    let mut cache = SampleCache::default();
    let mut sample_count = 0;
    for &id in &playable {
        let path = snapshot.resolve(
            "audio/effects/mappings.json",
            u32::from(id),
            Some("sound.json"),
        )?;
        for environment in [source.field_reverbs, source.battle_reverbs] {
            let program = snapshot
                .json::<resonance_audio::effect::Definition>(&path, 16 * 1024 * 1024)?
                .into_program()?;
            assert!(program.reverbs.is_none());
            let loaded = program.prepare_in(
                Some(environment),
                &mut |path, limit| snapshot.read(path, limit),
                &mut cache,
            )?;
            let original = source.prepare_sound(id, environment)?;
            assert_eq!(loaded.reverbs(), environment);
            assert_eq!(
                serde_json::to_value(loaded.score())?,
                serde_json::to_value(original.score())?
            );
            assert_eq!(
                serde_json::to_value(loaded.tables())?,
                serde_json::to_value(original.tables())?
            );
            assert_eq!(
                serde_json::to_value(&loaded.resources().programs)?,
                serde_json::to_value(&original.resources().programs)?
            );
            assert_eq!(
                loaded.resources().samples.len(),
                original.resources().samples.len()
            );
            for (slot, native) in &original.resources().samples {
                let current = &loaded.resources().samples[slot];
                assert_eq!(
                    (
                        current.key,
                        current.rate,
                        current.loop_start,
                        current.loop_length
                    ),
                    (
                        native.key,
                        native.rate,
                        native.loop_start,
                        native.loop_length
                    )
                );
                assert_eq!(current.pcm, native.pcm, "effect {id} sample {slot}");
                assert_eq!(
                    current.loop_pcm, native.loop_pcm,
                    "effect {id} sample {slot}"
                );
                sample_count += 1;
            }
        }
        cache.prune();
    }
    assert_eq!(fs::read_dir(empty.path())?.count(), 0);
    assert!(!output.path().join("audio/samples").exists());
    assert!(!output.path().join("assets").exists());
    eprintln!(
        "Verified {} native effects and {sample_count} sample bindings across field/battle environments, with no staged input",
        playable.len()
    );
    Ok(())
}
