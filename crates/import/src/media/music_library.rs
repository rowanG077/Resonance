//! Assemble mixer scores from cooked arrangements, instruments and shared PCM.
use super::{Workspace, field_audio::write_package, music::ReverbChange, sound_library};
use anyhow::{Context, Result, ensure};
use resonance_audio::{
    data::Note,
    music_voice::Tables,
    package::{Package, SampleAsset},
};
use resonance_audio_cook::{bank::MusicSetup, compile, decode::Instruction, song::Song};
use resonance_content::field_audio::Asset;
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

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

pub(super) struct Library<'a> {
    root: &'a Path,
    disc: u8,
    sources: BTreeMap<String, Vec<String>>,
}

impl<'a> Library<'a> {
    pub(super) fn open(root: &'a Path, disc: u8) -> Result<Self> {
        Ok(Self {
            root,
            disc,
            sources: sound_library::source_index(root)?,
        })
    }

    pub(super) fn package(
        &self,
        source: &str,
        id: u16,
        current_reverbs: Option<[[f32; 5]; 2]>,
    ) -> Result<Package> {
        let key = format!("disc{}/{source}", self.disc);
        let outputs = self
            .sources
            .get(&key)
            .with_context(|| format!("missing cooked {key}; rerun cook-all"))?;
        let read = |name: &str| -> Result<Vec<u8>> {
            let suffix = format!("/{name}.json");
            let mut paths = outputs
                .iter()
                .filter(|path| path.starts_with("audio/songs/") && path.ends_with(&suffix));
            let path = paths
                .next()
                .with_context(|| format!("missing cooked {key} {name}"))?;
            ensure!(paths.next().is_none(), "ambiguous cooked {key} {name}");
            resonance_content::validate_asset_path(path)?;
            fs::read(self.root.join(path)).with_context(|| format!("read cooked {path}"))
        };
        let song: Song = serde_json::from_slice(&read("arrangement")?)?;
        let playback: Playback = serde_json::from_slice(&read(&format!("setup-{id}"))?)?;
        playback
            .package(&song, current_reverbs)
            .with_context(|| format!("bind music {id}: {key}"))
    }
}

pub(crate) fn bind_music(
    workspace: &Workspace,
    executable: &[u8],
    prefix: &str,
    ids: impl IntoIterator<Item = u16>,
    reverbs: [[f32; 5]; 2],
) -> Result<BTreeMap<i16, Asset>> {
    let library = Library::open(&workspace.output, workspace.disc)?;
    ids.into_iter()
        .map(|id| {
            let key = i16::try_from(id).context("music ID exceeds runtime range")?;
            let package =
                library.package(&super::music_path(executable, id)?, id, Some(reverbs))?;
            ensure!(
                package.reverbs == reverbs,
                "music {id} requires another reverb preset"
            );
            let asset = write_package(workspace, &format!("audio/{prefix}-{id}.json"), &package)?;
            Ok((key, asset))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_audio_cook::bank::Bank;

    #[test]
    #[ignore = "requires both extracted discs and cook-all; compares data without audio playback"]
    fn original_music_binding_preserves_every_arrangement_and_setup() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let root = local.join("all-assets");
        let mut count = 0;
        let mut arrangements = 0;
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let bytes = fs::read(extracted.join("files/S/inst.snd"))?;
            let bank = Bank::parse(&bytes)?;
            let library = Library::open(&root, disc)?;
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
                let paths = &library.sources[&format!("disc{disc}/{source}")];
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
                    let package = library.package(&source, id, Some(current))?;
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
                            assert!(library.package(&source, id, None).is_err());
                            let mut changed = current;
                            changed[0][1] = 0.25;
                            assert_eq!(
                                library.package(&source, id, Some(changed))?.reverbs,
                                changed
                            );
                        }
                    }
                    assert!(package.samples.keys().eq(resources.samples.keys()));
                    for (sample_id, asset) in package.samples {
                        assert!(asset.path.starts_with("audio/samples/"));
                        resonance_content::validate_asset_path(&asset.path)?;
                        let bytes = fs::read(root.join(&asset.path))?;
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

    #[test]
    #[ignore = "requires the cooked title arrangement; no original sources or playback"]
    fn cooked_music_rejects_missing_duplicate_and_unused_note_bindings() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
        let sources = sound_library::source_index(&root)?;
        let paths = &sources["disc1/S/bgm_etc000.song"];
        let read = |suffix: &str| -> Result<Vec<u8>> {
            fs::read(
                root.join(
                    paths
                        .iter()
                        .find(|path| path.ends_with(suffix))
                        .context("missing title fixture")?,
                ),
            )
            .map_err(Into::into)
        };
        let song: Song = serde_json::from_slice(&read("/arrangement.json")?)?;
        let setup = read("/setup-1.json")?;
        for mode in 0..3 {
            let mut playback: Playback = serde_json::from_slice(&setup)?;
            let first = playback
                .note_bindings
                .first()
                .context("title has no notes")?;
            match mode {
                0 => {
                    playback.note_bindings.remove(0);
                }
                1 => playback.note_bindings.push(NoteBinding {
                    event: first.event,
                    voices: Vec::new(),
                }),
                _ => playback.note_bindings.push(NoteBinding {
                    event: usize::MAX,
                    voices: Vec::new(),
                }),
            }
            assert!(playback.package(&song, None).is_err());
        }
        Ok(())
    }
}
