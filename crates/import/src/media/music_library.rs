//! Prepare mixer scores directly from original arrangements and instruments.
use super::{Workspace, music::ReverbChange, sound_library};
use anyhow::{Context, Result};
use resonance_audio::package::Package;
use resonance_audio_cook::{compile, song::Song};
use std::fs;

pub(crate) fn package(
    workspace: &Workspace,
    executable: &[u8],
    pools: &crate::media::library::Pools,
    id: u16,
    current_reverbs: Option<[[f32; 5]; 2]>,
) -> Result<Package> {
    let source = super::music_path(executable, id)?;
    let files = workspace.extracted.join("files");
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
    sound_library::package(
        &workspace.output,
        &resources,
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
    fn music_packages_close_instrument_dependencies_and_preserve_sample_data() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let mut count = 0;
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let output = tempfile::tempdir()?;
            let pools = crate::media::library::Pools::read(&extracted)?;
            let bank = pools.instruments()?;
            let workspace = Workspace::open(&extracted, output.path())?;
            let directory = crate::music_directory::Directory::read(&executable)?;
            let ids: BTreeSet<_> = directory.active().map(|entry| entry.id as u16).collect();
            ensure!(!ids.is_empty(), "disc{disc} has no declared music setups");
            let files = extracted.join("files");
            for id in ids {
                let source = crate::field_resources::resolve_path(&files, &directory.path(id)?)?;
                let song = Song::parse(&fs::read(files.join(source))?)?;
                let current = super::super::music::title_reverbs(&executable)?;
                let reverb = super::super::song_reverb_change(&executable, id)?;
                let package = package(&workspace, &executable, &pools, id, Some(current))?;
                let loaded = Package::load_with(
                    "music.json",
                    &mut |path, _| {
                        if path == "music.json" {
                            Ok(serde_json::to_vec(&package)?)
                        } else {
                            Ok(fs::read(output.path().join(path))?)
                        }
                    },
                    &mut Default::default(),
                )?;
                assert_eq!(package.score.initial_bpm_1024, song.initial_bpm_1024);
                assert_eq!(
                    (package.score.loop_start_tick, package.score.end_tick),
                    song.playback_interval()?
                );
                match reverb {
                    ReverbChange::Set { parameters } => assert_eq!(package.reverbs, parameters),
                    ReverbChange::Keep => {
                        assert_eq!(package.reverbs, current);
                        assert!(super::package(&workspace, &executable, &pools, id, None).is_err());
                        let mut changed = current;
                        changed[0][1] = 0.25;
                        assert_eq!(
                            super::package(&workspace, &executable, &pools, id, Some(changed))?
                                .reverbs,
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
