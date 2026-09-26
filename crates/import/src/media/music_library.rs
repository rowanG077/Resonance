//! Prepare mixer scores directly from original arrangements and instruments.
use super::{Workspace, music::ReverbChange, sound_library};
use anyhow::{Context, Result};
use resonance_audio::package::Package;
use resonance_audio_cook::{compile, decode, instrument, song::Song};
use std::fs;

pub(crate) fn package(
    workspace: &Workspace,
    executable: &[u8],
    coefficients: &[u8],
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
    let mut roots = std::collections::BTreeSet::new();
    let score = compile::score(&song, &setup, |_, page, key, velocity| {
        let voices = page.map_or_else(
            || Ok(Vec::new()),
            |page| instrument::resolve(&bank, page, key, velocity, 64),
        )?;
        roots.extend(voices.iter().map(|voice| voice.macro_id));
        Ok(voices)
    })?;
    let resources =
        decode::programs(&bank, roots).with_context(|| format!("decode music {id}: {source}"))?;
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
        super::synthesis_tables(executable, coefficients)?,
        reverbs,
    )
    .with_context(|| format!("prepare music {id}: {source}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::ensure;
    use resonance_audio::{data::Note, music_voice::Tables, package::SampleAsset};
    use resonance_audio_cook::{
        bank::{Bank, MusicSetup},
        decode::Instruction,
    };
    use serde::Deserialize;
    use std::{collections::BTreeMap, path::Path};

    // Independent reader for the frozen pre-migration preparation records.
    #[derive(Deserialize)]
    struct Playback {
        version: u32,
        programs: BTreeMap<u16, Vec<Instruction>>,
        samples: BTreeMap<u16, SampleAsset>,
        setup: MusicSetup,
        first_event_count: usize,
        note_bindings: Vec<NoteBinding>,
        tables: Tables,
        reverb: ReverbChange,
    }

    #[derive(Deserialize)]
    struct NoteBinding {
        event: usize,
        voices: Vec<Note>,
    }

    impl Playback {
        fn package(self, song: &Song, current_reverbs: Option<[[f32; 5]; 2]>) -> Result<Package> {
            let reverbs = match self.reverb {
                ReverbChange::Set { parameters } => parameters,
                ReverbChange::Keep => {
                    current_reverbs.context("music needs the current reverb environment")?
                }
            };
            ensure!(
                self.first_event_count == song.events().len(),
                "cooked music event count differs from arrangement"
            );
            let mut notes = BTreeMap::new();
            for binding in self.note_bindings {
                ensure!(
                    notes.insert(binding.event, binding.voices).is_none(),
                    "duplicate cooked music note binding"
                );
            }
            let score = compile::score(song, &self.setup, |index, page, _, _| {
                let voices = notes.remove(&index);
                if page.is_some() {
                    voices.with_context(|| format!("missing cooked music note binding {index}"))
                } else {
                    ensure!(
                        voices.is_none(),
                        "unexpected cooked music note binding {index}"
                    );
                    Ok(Vec::new())
                }
            })?;
            ensure!(notes.is_empty(), "cooked music has unused note bindings");
            sound_library::Resources {
                version: self.version,
                programs: self.programs,
                samples: self.samples,
                score: Some(score),
            }
            .package(self.tables, reverbs)
        }
    }

    #[test]
    #[ignore = "requires both extracted discs and cook-all; compares data without audio playback"]
    fn original_music_binding_preserves_every_arrangement_and_setup() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let root = std::env::var_os("RESONANCE_AUDIO_BASELINE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| local.join("worktrees/generic-cooking/local/all-assets"));
        let coefficients = fs::read(
            std::env::var_os("RESONANCE_DSP_COEFFICIENTS")
                .context("set RESONANCE_DSP_COEFFICIENTS to Dolphin's dsp_coef.bin")?,
        )?;
        let sources = sound_library::source_index(&root)?;
        let mut count = 0;
        let mut arrangements = 0;
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let bytes = fs::read(extracted.join("files/S/inst.snd"))?;
            let bank = Bank::parse(&bytes)?;
            let output = tempfile::tempdir()?;
            let pools = crate::media::library::Pools::read(&extracted)?;
            let workspace = Workspace::open(&extracted, output.path())?;
            let mut songs = fs::read_dir(extracted.join("files/S"))?
                .map(|entry| Ok(entry?.path()))
                .collect::<Result<Vec<_>>>()?;
            songs.retain(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "song")
            });
            songs.sort();
            ensure!(!songs.is_empty(), "disc{disc} has no source arrangements");
            for path in songs {
                let source = format!(
                    "S/{}",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .context("invalid song filename")?
                );
                let song = Song::parse(&fs::read(path)?)?;
                let paths = &sources[&format!("disc{disc}/{source}")];
                let arrangement = paths
                    .iter()
                    .find(|path| path.ends_with("/arrangement.json"))
                    .context("missing cooked arrangement")?;
                assert_eq!(
                    serde_json::to_value(&song)?,
                    serde_json::from_slice::<serde_json::Value>(&fs::read(
                        root.join(arrangement)
                    )?)?
                );
                arrangements += 1;
                for id in crate::music_directory::Directory::read(&executable)?.ids(&source)? {
                    let (resources, score) =
                        compile::music(&bank, &song, &bank.music_setup(0, id)?)
                            .with_context(|| format!("disc{disc} source music {id}: {source}"))?;
                    let current = super::super::music::title_reverbs(&executable)?;
                    let reverb = super::super::song_reverb_change(&executable, id)?;
                    let package = package(
                        &workspace,
                        &executable,
                        &coefficients,
                        &pools,
                        id,
                        Some(current),
                    )?;
                    let frozen = paths
                        .iter()
                        .find(|path| path.ends_with(&format!("/setup-{id}.json")))
                        .context("missing frozen music setup")?;
                    let frozen: Playback = serde_json::from_slice(&fs::read(root.join(frozen))?)?;
                    let expected = frozen.package(&song, Some(current))?;
                    assert_eq!(
                        serde_json::to_vec(&package)?,
                        serde_json::to_vec(&expected)?,
                        "disc{disc} music {id}: complete frozen package"
                    );
                    assert!(!output.path().join("sources.json").exists());
                    assert_eq!(
                        serde_json::to_value(&package.score)?,
                        serde_json::to_value(&score)?,
                        "disc{disc} music {id} score"
                    );
                    assert_eq!(
                        serde_json::to_value(&package.programs)?,
                        serde_json::to_value(&resources.programs)?,
                        "disc{disc} music {id} programs"
                    );
                    match reverb {
                        ReverbChange::Set { parameters } => assert_eq!(package.reverbs, parameters),
                        ReverbChange::Keep => {
                            assert_eq!(package.reverbs, current);
                            assert!(
                                super::package(
                                    &workspace,
                                    &executable,
                                    &coefficients,
                                    &pools,
                                    id,
                                    None
                                )
                                .is_err()
                            );
                            let mut changed = current;
                            changed[0][1] = 0.25;
                            assert_eq!(
                                super::package(
                                    &workspace,
                                    &executable,
                                    &coefficients,
                                    &pools,
                                    id,
                                    Some(changed)
                                )?
                                .reverbs,
                                changed
                            );
                        }
                    }
                    assert!(package.samples.keys().eq(resources.samples.keys()));
                    for (sample_id, asset) in package.samples {
                        assert!(asset.path.starts_with("audio/samples/"));
                        resonance_content::validate_asset_path(&asset.path)?;
                        let bytes = fs::read(output.path().join(&asset.path))?;
                        assert_eq!(
                            bytes,
                            fs::read(root.join(&asset.path))?,
                            "shared PCM differs from frozen output"
                        );
                        assert_eq!(crate::digest(&bytes), asset.sha256);
                        let original = &resources.samples[&sample_id];
                        assert_eq!(
                            (
                                asset.key,
                                asset.rate,
                                asset.first_frames,
                                asset.loop_start,
                                asset.loop_length
                            ),
                            (
                                original.key,
                                original.rate,
                                original.pcm.len() as u32,
                                original.loop_start,
                                original.loop_length
                            )
                        );
                        let mut wave = hound::WavReader::new(std::io::Cursor::new(bytes))?;
                        assert_eq!(wave.spec().sample_rate, u32::from(original.rate));
                        assert_eq!(wave.spec().channels, 1);
                        let pcm = wave
                            .samples::<i16>()
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                        assert!(
                            pcm.iter().eq(original.pcm.iter().chain(&original.loop_pcm)),
                            "disc{disc} music {id} sample {sample_id}"
                        );
                    }
                    count += 1;
                }
            }
        }
        assert_eq!((arrangements, count), (236, 224));
        println!(
            "Preserved all {arrangements} arrangements and {count} setups: complete scores, programs, reverbs, sample metadata and PCM."
        );
        Ok(())
    }
}
