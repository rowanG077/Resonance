use super::*;
use resonance_content::startup;

impl Publisher<'_> {
    /// Publish title audio directly from native music and the resident sound bank.
    pub fn startup_audio(&mut self) -> Result<()> {
        self.title_recording(&|| false)?;
        self.menu_cues()
    }

    fn menu_cues(&mut self) -> Result<()> {
        let source = self.audio_source()?;
        let reverbs = source.field_reverbs;
        let cues = source.original.menu()?;
        let mut bindings = BTreeMap::new();
        for (name, cue) in cues {
            ensure!(cue.reverbs() == reverbs, "menu cue studio differs");
            let owner = format!("audio/effects/menu/{name}");
            self.native_program(&cue, &owner, "audio/instruments/menu", false)?;
            bindings.insert(name.to_owned(), format!("{owner}/sound.json"));
        }
        self.json(
            "audio/effects/menu/cues.json",
            &startup::Cues {
                version: startup::VERSION,
                reverbs,
                cues: bindings,
            },
        )
    }
}

#[test]
#[ignore = "requires original disc 1; no cooked baseline or audio device"]
fn native_menu_cues_publish_directly_and_preserve_samples_and_playback() -> Result<()> {
    use resonance_audio::{package::SampleCache, sequence};
    use resonance_content::library::Library;
    use std::sync::Arc;

    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let empty = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(empty.path(), output.path())?
        .with_audio_source(AudioSource::read(&extracted, None)?);
    publisher.menu_cues()?;
    publisher.descriptor("GQSEAF", 0)?;
    let mut snapshot = Library::open(output.path(), &[])?.snapshot();
    let cues: startup::Cues = snapshot.json("audio/effects/menu/cues.json", 1024 * 1024)?;
    assert_eq!(cues.cues.len(), 4);
    let mut cache = SampleCache::default();
    for (name, original) in publisher.audio_source()?.original.menu()? {
        let program = snapshot
            .json::<resonance_audio::effect::Definition>(&cues.cues[name], 16 * 1024 * 1024)?
            .into_program()?;
        let current = Arc::new(
            program.prepare_editable(&mut |path, limit| snapshot.read(path, limit), &mut cache)?,
        );
        assert_eq!(current.reverbs(), cues.reverbs);
        assert_eq!(
            serde_json::to_value(current.score())?,
            serde_json::to_value(original.score())?
        );
        assert_eq!(
            serde_json::to_value(current.tables())?,
            serde_json::to_value(original.tables())?
        );
        assert_eq!(
            serde_json::to_value(&current.resources().programs)?,
            serde_json::to_value(&original.resources().programs)?
        );
        assert_eq!(
            current.resources().samples.len(),
            original.resources().samples.len()
        );
        for (id, native) in &original.resources().samples {
            let sample = &current.resources().samples[id];
            assert_eq!(
                (
                    sample.key,
                    sample.rate,
                    sample.loop_start,
                    sample.loop_length
                ),
                (
                    native.key,
                    native.rate,
                    native.loop_start,
                    native.loop_length
                )
            );
            assert_eq!(sample.pcm, native.pcm);
            assert_eq!(sample.loop_pcm, native.loop_pcm);
        }
        let frames = resonance_audio::SOURCE_RATE;
        let actual = sequence::render_preview(current, cues.reverbs, frames)?;
        let expected = sequence::render_preview(Arc::new(original), cues.reverbs, frames)?;
        assert_eq!(actual.pcm, expected.pcm, "menu cue {name}");
        assert!(actual.pcm.iter().any(|&sample| sample != 0));
    }
    assert!(publisher.published.keys().all(|path| path == "content.json"
        || path.starts_with("audio/effects/menu/")
        || path.starts_with("audio/instruments/menu/")));
    assert_eq!(fs::read_dir(empty.path())?.count(), 0);
    Ok(())
}
