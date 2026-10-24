//! Prepare mixer scores directly from original arrangements and instruments.
use super::music::ReverbChange;
use anyhow::{Context, Result};
use resonance_audio::package::Loaded;
use resonance_audio_cook::{compile, song::Song};
use std::{fs, path::Path};

/// Compile the original arrangement and its instrument closure in memory.
/// No cooked package, sample WAV or output workspace is involved.
pub(crate) fn prepare(
    extracted: &Path,
    executable: &[u8],
    pools: &crate::media::library::Pools,
    id: u16,
    current_reverbs: Option<[[f32; 5]; 2]>,
) -> Result<Loaded> {
    let source = super::music_path(executable, id)?;
    let files = extracted.join("files");
    let path = crate::field_resources::resolve_path(&files, &source)?;
    let song = Song::parse(&fs::read(files.join(path))?)?;
    let bank = pools.instruments()?;
    let setup = bank.music_setup(0, id)?;
    let (resources, score) = compile::music(&bank, &song, &setup, &pools.sustains)
        .with_context(|| format!("compile music {id}: {source}"))?;
    let reverbs = match super::song_reverb_change(executable, id)? {
        ReverbChange::Set { parameters } => parameters,
        ReverbChange::Keep => {
            current_reverbs.context("music needs the current reverb environment")?
        }
    };
    Loaded::new(
        resources,
        score,
        super::synthesis_tables(executable)?,
        reverbs,
    )
    .with_context(|| format!("prepare music {id}: {source}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::ensure;
    use std::{collections::BTreeSet, path::Path};

    #[test]
    #[ignore = "requires both extracted discs; CPU package and PCM validation"]
    fn native_music_closes_instrument_dependencies_and_preserves_sample_data() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let mut count = 0;
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let pools = crate::media::library::Pools::read(&extracted)?;
            let bank = pools.instruments()?;
            let directory = crate::music_directory::Directory::read(&executable)?;
            let ids: BTreeSet<_> = directory.active().map(|entry| entry.id as u16).collect();
            ensure!(!ids.is_empty(), "disc{disc} has no declared music setups");
            let files = extracted.join("files");
            for id in ids {
                let source = crate::field_resources::resolve_path(&files, &directory.path(id)?)?;
                let song = Song::parse(&fs::read(files.join(source))?)?;
                let current = super::super::music::title_reverbs(&executable)?;
                let reverb = super::super::song_reverb_change(&executable, id)?;
                let loaded = prepare(&extracted, &executable, &pools, id, Some(current))?;
                assert_eq!(loaded.score().initial_bpm_1024, song.initial_bpm_1024);
                assert_eq!(
                    (loaded.score().loop_start_tick, loaded.score().end_tick),
                    song.playback_interval()?
                );
                match reverb {
                    ReverbChange::Set { parameters } => assert_eq!(loaded.reverbs(), parameters),
                    ReverbChange::Keep => {
                        assert_eq!(loaded.reverbs(), current);
                        assert!(super::prepare(&extracted, &executable, &pools, id, None).is_err());
                        let mut changed = current;
                        changed[0][1] = 0.25;
                        assert_eq!(
                            super::prepare(&extracted, &executable, &pools, id, Some(changed))?
                                .reverbs(),
                            changed
                        );
                    }
                }
                for (&sample_id, sample) in &loaded.resources().samples {
                    let original = bank.sample(sample_id)?;
                    assert_eq!(
                        (
                            sample.key,
                            sample.rate,
                            sample.loop_start,
                            sample.loop_length
                        ),
                        (
                            original.key,
                            original.rate,
                            original.loop_start,
                            original.loop_length
                        )
                    );
                    assert_eq!(
                        sample.pcm, original.pcm,
                        "disc{disc} music {id} sample {sample_id}"
                    );
                    assert_eq!(sample.loop_pcm, original.loop_pcm);
                }
                count += 1;
            }
        }
        println!(
            "Validated {count} declared music setups: instrument closure, reverb, sample metadata and PCM."
        );
        Ok(())
    }
}
