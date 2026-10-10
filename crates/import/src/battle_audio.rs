//! Prepare selected battle sounds through the same converters as field audio.
use crate::{
    all_assets::roles,
    media::{self, Workspace, library::Pools},
};
use anyhow::{Context, Result, ensure};
use resonance_audio::package::Package;
use resonance_audio_cook::bank::Bank;
use resonance_content::{
    battle_audio::{Audio, PATH},
    field_audio::{Asset, FieldAudio},
    field_preload::{File, Role},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub sounds: BTreeSet<u16>,
    pub streams: BTreeSet<u16>,
    pub music: BTreeSet<u16>,
}

/// Maintained IDs from verified encounter binding inventories. The normal
/// cooker never depends on a developer checkpoint or reads authored source paths.
pub fn maintained_selection() -> Result<Selection> {
    let source = resonance_script_content::FILES
        .iter()
        .find_map(|&(path, source)| (path == "battle/audio-requirements.json").then_some(source))
        .context(
            "battle audio requirements are not published; finish the real binding inventory",
        )?;
    Ok(serde_json::from_str(source)?)
}

pub struct Publication {
    pub files: Vec<String>,
    /// Original paths relative to the extracted disc (including files/ or sys/).
    pub inputs: BTreeMap<String, String>,
}

/// Refresh only selected original ADX members in a private shared-library stage.
/// The ordinary full cook already decodes its stream jobs and does not use this.
/// Publish these PCM files and metadata together with the consuming descriptors.
pub fn refresh_streams(
    extracted: &Path,
    library: &Path,
    output: &Path,
    streams: &BTreeSet<u16>,
) -> Result<Publication> {
    let workspace = Workspace::open(extracted, output)?;
    ensure!(
        library.canonicalize()? != workspace.output,
        "stream refresh requires a separate staging directory"
    );
    ensure!(
        !streams.is_empty() && streams.iter().all(|&id| id < 0x8000),
        "stream refresh requires selected original IDs"
    );
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let directory = crate::voice_directory::Directory::read(&executable)?;
    let source = directory
        .active()
        .find(|entry| entry.resource_id == 0x78)
        .context("missing original battle stream declaration")?
        .source_path()?;
    let source = crate::field_resources::resolve_path(&extracted.join("files"), &source)?;
    let mut original = fs::File::open(extracted.join("files").join(&source))?;
    let entries = crate::afs::index(&mut original)?;
    let mut archive = media::voice_library::Archive::open(library, extracted, &source)?;
    let mut files = BTreeSet::new();
    for &id in streams {
        // Validate the current source/metadata/PCM identity before replacement.
        let previous = archive.voice(usize::from(id))?;
        let entry = entries
            .get(usize::from(id))
            .context("missing selected stream")?;
        ensure!(
            entry.size <= 128 * 1024 * 1024,
            "selected stream exceeds read budget"
        );
        original.seek(SeekFrom::Start(entry.offset))?;
        let mut bytes = vec![0; entry.size];
        original.read_exact(&mut bytes)?;
        ensure!(
            crate::digest(&bytes) == previous.source_sha256
                && bytes
                    .get(4)
                    .is_some_and(|encoding| (2..=4).contains(encoding)),
            "selected stream is not the verified original ADX member"
        );
        let voice = media::decode_voice_to(
            &workspace,
            &previous.voice.asset.path,
            &crate::afs::Member {
                name: &entry.name,
                data: &bytes,
            },
            media::VoiceFormat::Adx,
        )?;
        ensure!(
            voice.source_sha256 == previous.source_sha256
                && voice.voice.frames == previous.voice.frames
                && voice.voice.channels == previous.voice.channels
                && voice.voice.source_sample_rate == previous.voice.source_sample_rate,
            "selected stream dimensions changed"
        );
        let metadata = &archive.index.members[usize::from(id)].metadata;
        crate::write_atomic(
            &workspace.output.join(metadata),
            &serde_json::to_vec(&voice)?,
        )?;
        files.insert(voice.voice.asset.path);
        files.insert(metadata.clone());
    }
    Ok(Publication {
        files: files.into_iter().collect(),
        inputs: [
            ("sys/main.dol".into(), crate::digest(&executable)),
            (format!("files/{source}"), archive.source.sha256),
        ]
        .into(),
    })
}

/// Decode only the requested cue closure. Stream PCM must already be in the
/// verified shared library; it is rebound and copied without re-decoding ADX.
/// Stream publication uses a private staging directory, separate from `library`.
pub fn publish(
    extracted: &Path,
    library: &Path,
    output: &Path,
    selection: &Selection,
) -> Result<Publication> {
    let workspace = Workspace::open(extracted, output)?;
    ensure!(
        selection.streams.is_empty() || library.canonicalize()? != workspace.output,
        "battle audio publication requires a separate staging directory"
    );
    publish_in(&workspace, library, selection)
}

/// Reuse the full cook's output owner. The physical voice jobs have completed
/// before this finishing operation; no second media lock or PCM decode is needed.
pub(crate) fn publish_in(
    workspace: &Workspace,
    library: &Path,
    selection: &Selection,
) -> Result<Publication> {
    let extracted = workspace.extracted.as_path();
    let output = workspace.output.as_path();
    let shared_library =
        !selection.streams.is_empty() && library.canonicalize()? == workspace.output;
    ensure!(
        selection
            .sounds
            .iter()
            .chain(&selection.streams)
            .chain(&selection.music)
            .all(|&id| id < 0x8000),
        "battle audio ID exceeds its original domain"
    );
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let module_path =
        crate::field_resources::resolve_path(&extracted.join("files"), "US_r_Top2Btl.rel")?;
    let module = crate::rel::Rel::read(&extracted.join("files").join(&module_path))?;
    let mut input_paths: BTreeSet<String> =
        ["sys/main.dol".into(), format!("files/{module_path}")].into();
    let mut files = BTreeMap::new();
    let mut music = BTreeMap::new();
    let mut sounds = BTreeMap::new();
    if !selection.music.is_empty() || !selection.sounds.is_empty() {
        let pools = Pools::read(extracted)?;
        let reverbs = media::music::battle_reverbs(&executable)?;
        let mut publish = |package: Package| -> Result<Asset> {
            compatible_reverbs(package.reverbs, reverbs)?;
            let bytes = serde_json::to_vec(&package)?;
            let path = format!("audio/prepared/{}.json", crate::digest(&bytes));
            let asset = media::field_audio::write_package(output, &path, &package)?;
            inventory(output, &mut files, &path, Role::AudioPackage)?;
            for sample in package.samples.values() {
                inventory(output, &mut files, &sample.path, Role::InstrumentSample)?;
            }
            Ok(asset)
        };
        let [instruments, common] = roles::resident_banks(extracted, &executable)?;
        input_paths.insert(format!("files/{instruments}"));
        input_paths.insert(format!("files/{common}"));
        for &id in &selection.music {
            input_paths.insert(format!("files/{}", media::music_path(&executable, id)?));
            music.insert(
                i16::try_from(id)?,
                publish(media::music_library::package(
                    workspace,
                    &executable,
                    &pools,
                    id,
                    Some(reverbs),
                )?)?,
            );
        }
        if !selection.sounds.is_empty() {
            let sources = crate::source_assets::Sources::read_with(extracted, &executable)?;
            input_paths.insert(format!("files/{}", sources.usual));
            let party = roles::party_banks(extracted, &executable)?;
            let mut groups = BTreeMap::new();
            let voice_bank = roles::voice_bank_path(extracted)?;
            input_paths.insert(format!("files/{voice_bank}"));
            let (mut archive, offsets) =
                media::library::bank_archive(extracted, &voice_bank, &sources.usual)?;
            for source in std::iter::once(common.clone())
                .chain(party.iter().cloned())
                .chain(media::library::bank_sources(extracted, &executable)?)
            {
                input_paths.insert(format!("files/{source}"));
                if source == instruments {
                    continue;
                }
                let bytes = fs::read(extracted.join("files").join(&source))?;
                let bank = Bank::parse(&bytes)?;
                let group = bank.group()?;
                if source != common && !party.contains(&source) && group < 19 {
                    continue;
                }
                let hash = crate::digest(&bytes);
                if let Some(previous) = groups.insert(group, hash.clone()) {
                    ensure!(previous == hash, "conflicting battle sound group {group}");
                    continue;
                }
                let ids: Vec<_> = bank
                    .sound_ids()
                    .filter(|id| selection.sounds.contains(id))
                    .collect();
                if ids.is_empty() {
                    continue;
                }
                let payload = if group >= 19 {
                    let index = usize::from(group - 19);
                    let range = offsets
                        .get(index..index + 2)
                        .context("missing battle sound payload")?;
                    Some(media::library::payload(&mut archive, range[0], range[1])?)
                } else {
                    None
                };
                let mut bank = pools.bank(&bytes)?;
                if let Some(payload) = &payload {
                    bank = bank.with_sample_payload(payload);
                }
                for id in ids {
                    ensure!(
                        !sounds.contains_key(&i16::try_from(id)?),
                        "ambiguous battle sound {id}"
                    );
                    let (resources, score) =
                        media::sound_library::sound(&bank, id, &pools.sustains)
                            .with_context(|| format!("battle sound {id} from {source}"))?;
                    let package = media::sound_library::package(
                        output,
                        &resources,
                        score,
                        media::synthesis_tables(&executable)?,
                        reverbs,
                    )?;
                    sounds.insert(i16::try_from(id)?, publish(package)?);
                }
            }
            ensure!(
                sounds.len() == selection.sounds.len(),
                "missing requested battle sounds: {:?}",
                selection
                    .sounds
                    .iter()
                    .filter(|&&id| !sounds.contains_key(&(id as i16)))
                    .collect::<Vec<_>>()
            );
        }
    }
    let mut voices = BTreeMap::new();
    if !selection.streams.is_empty() {
        let directory = crate::voice_directory::Directory::read(&executable)?;
        let source = directory
            .active()
            .find(|entry| entry.resource_id == 0x78)
            .context("missing original battle stream declaration")?
            .source_path()?;
        input_paths.insert(format!("files/{source}"));
        let mut archive = media::voice_library::Archive::open(library, extracted, &source)?;
        for &id in &selection.streams {
            ensure!(id < 0x8000, "invalid battle stream {id}");
            let voice = archive.voice(usize::from(id))?.voice;
            if !shared_library {
                let bytes = fs::read(library.join(&voice.asset.path))?;
                crate::write_atomic(&output.join(&voice.asset.path), &bytes)?;
            }
            inventory(output, &mut files, &voice.asset.path, Role::Voice)?;
            voices.insert(u32::from(id), voice);
        }
    }
    let spatial = |start| -> Result<[f32; 5]> {
        let mut values = [0.; 5];
        for (i, value) in values.iter_mut().enumerate() {
            *value = crate::read::f32(module.at((4, start))?, i * 4)?;
        }
        Ok(values)
    };
    let descriptor = Audio {
        assets: FieldAudio {
            version: FieldAudio::VERSION,
            music_reverbs: media::music::music_reverbs(&executable)?,
            music,
            sounds,
            voices,
            voice_gains: media::field_audio::voice_gains(&executable)?,
        },
        effect_spatial: spatial(0x4d8)?,
        voice_spatial: spatial(0x4668)?,
        files,
    };
    descriptor.validate()?;
    crate::write_atomic(&output.join(PATH), &serde_json::to_vec(&descriptor)?)?;
    Ok(Publication {
        files: std::iter::once(PATH.to_owned())
            .chain(descriptor.files.into_keys())
            .collect(),
        inputs: input_paths
            .into_iter()
            .map(|path| {
                // DOL declarations preserve authored casing; source inventories
                // name the actual extracted file, just as archive loading does.
                let path = if let Some(source) = path.strip_prefix("files/") {
                    format!(
                        "files/{}",
                        crate::field_resources::resolve_path(&extracted.join("files"), source)?
                    )
                } else {
                    path
                };
                Ok((path.clone(), media::hash_file(&extracted.join(path))?))
            })
            .collect::<Result<_>>()?,
    })
}

fn compatible_reverbs(package: [[f32; 5]; 2], shared: [[f32; 5]; 2]) -> Result<()> {
    ensure!(
        package[1] == shared[1],
        "selected audio changes the shared auxiliary B effect"
    );
    Ok(())
}

fn inventory(
    root: &Path,
    files: &mut BTreeMap<String, File>,
    path: &str,
    role: Role,
) -> Result<()> {
    let file = File {
        sha256: media::hash_file(&root.join(path))?,
        bytes: fs::metadata(root.join(path))?.len(),
        roles: [role].into(),
    };
    if let Some(previous) = files.get_mut(path) {
        ensure!(
            previous.sha256 == file.sha256 && previous.bytes == file.bytes,
            "battle audio publication changed: {path}"
        );
        previous.roles.insert(role);
    } else {
        files.insert(path.into(), file);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn songs_can_select_auxiliary_a_while_auxiliary_b_stays_shared() {
        let shared = [[0.7, 0.7, 2.5, 0.6, 0.05], [1., 0.5, 1., 0.8, 0.01]];
        let mut song = shared;
        song[0] = [0.8, 0.7, 3.6, 0.6, 0.08];
        compatible_reverbs(song, shared).unwrap();
        song[1][1] = 0.25;
        assert!(compatible_reverbs(song, shared).is_err());
    }

    #[test]
    #[cfg(unix)]
    #[ignore = "requires extracted disc 1 and existing PCM library; no synthesis or device"]
    fn streams_publish_without_score_banks_or_coefficients() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let original = local.join("extracted/disc1");
        let library = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| local.join("all-assets"));
        let extracted = tempfile::tempdir()?;
        for source in [
            "sys/boot.bin",
            "sys/main.dol",
            "files/US_r_Top2Btl.rel",
            "files/BTL/btlvoice.afs",
        ] {
            let path = extracted.path().join(source);
            fs::create_dir_all(path.parent().unwrap())?;
            std::os::unix::fs::symlink(original.join(source).canonicalize()?, path)?;
        }
        let output = tempfile::tempdir()?;
        let publication = publish(
            extracted.path(),
            &library,
            output.path(),
            &Selection {
                streams: [257].into(),
                ..Selection::default()
            },
        )?;
        let audio: Audio = serde_json::from_slice(&fs::read(output.path().join(PATH))?)?;
        audio.validate()?;
        assert!(audio.assets.music.is_empty() && audio.assets.sounds.is_empty());
        assert_eq!(
            audio.assets.voices.keys().copied().collect::<Vec<_>>(),
            [257]
        );
        assert_eq!(audio.files.len(), 1);
        assert_eq!(publication.files.len(), 2);
        assert_eq!(publication.inputs.len(), 3);
        Ok(())
    }

    #[test]
    #[ignore = "requires original disc, existing audio library and captured CRI prefill; no device"]
    fn selected_stream_refresh_preserves_library_and_uses_current_decoder() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let extracted = local.join("extracted/disc1");
        let library = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .context("set RESONANCE_TEST_ASSETS to the existing shared library")?;
        let expected = fs::read(
            std::env::var_os("RESONANCE_CRI_ADX_REFERENCE")
                .context("set RESONANCE_CRI_ADX_REFERENCE to captured BE PCM")?,
        )?;
        ensure!(
            crate::digest(&expected)
                == "a0a169f74cec7b22fc359a342488c119747aa037c972924ac1c5d6d1704f2a42",
            "unexpected source prefill reference"
        );
        let mut archive =
            media::voice_library::Archive::open(&library, &extracted, "BTL/btlvoice.afs")?;
        let pcm = archive.voice(257)?.voice.asset.path;
        let metadata = archive.index.members[257].metadata.clone();
        let original: Vec<_> = [pcm, metadata]
            .into_iter()
            .map(|path| Ok((path.clone(), fs::read(library.join(path))?)))
            .collect::<Result<_>>()?;
        let stage = tempfile::tempdir()?;
        let publication = refresh_streams(&extracted, &library, stage.path(), &[257].into())?;
        assert_eq!(publication.files.len(), 2);
        let path = publication
            .files
            .iter()
            .find(|path| path.ends_with(".wav"))
            .context("refreshed PCM missing")?;
        let mut wave = hound::WavReader::open(stage.path().join(path))?;
        let decoded: Vec<_> = wave
            .samples::<i16>()
            .take(4096)
            .collect::<std::result::Result<_, _>>()?;
        let observed: Vec<_> = decoded
            .iter()
            .flat_map(|sample| sample.to_be_bytes())
            .collect();
        assert_eq!(observed, expected);
        let failed = tempfile::tempdir()?;
        assert!(
            refresh_streams(&extracted, &library, failed.path(), &[257, 32766].into()).is_err()
        );
        assert!(!failed.path().join(".cook-media.lock").exists());
        for (path, bytes) in original {
            assert_eq!(fs::read(library.join(path))?, bytes);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original discs; no device"]
    fn selected_original_battle_packages_preserve_both_disc_identity() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let selection = Selection {
            sounds: [60].into(),
            streams: BTreeSet::new(),
            music: [1, 85, 95].into(),
        };
        let mut expected = None;
        for disc in [1, 2] {
            let output = tempfile::tempdir()?;
            let published = publish(
                &local.join(format!("extracted/disc{disc}")),
                &output.path().join("unused-library"),
                output.path(),
                &selection,
            )?;
            let bytes = fs::read(output.path().join(PATH))?;
            // Production already owns its output session and shared library.
            // Binding in place must use that owner instead of acquiring its lock.
            let in_place = tempfile::tempdir()?;
            let workspace = Workspace::open(
                &local.join(format!("extracted/disc{disc}")),
                in_place.path(),
            )?;
            let direct = publish_in(&workspace, in_place.path(), &selection)?;
            assert_eq!(direct.files, published.files);
            assert_eq!(fs::read(in_place.path().join(PATH))?, bytes);
            let audio: Audio = serde_json::from_slice(&bytes)?;
            audio.validate()?;
            assert_eq!(
                audio.assets.sounds.keys().copied().collect::<Vec<_>>(),
                [60]
            );
            assert_eq!(
                audio.assets.music.keys().copied().collect::<Vec<_>>(),
                [1, 85, 95]
            );
            assert_eq!(published.files.len(), audio.files.len() + 1);
            assert_eq!(audio.effect_spatial, [320., 5., 64., 0., 127.]);
            assert_eq!(audio.voice_spatial, [320., 5., 64., 24., 104.]);
            for (&id, asset) in &audio.assets.music {
                let loaded = Package::load(output.path(), &asset.path)?;
                assert_eq!(
                    loaded.reverbs(),
                    media::music::song_reverbs(
                        &fs::read(local.join(format!("extracted/disc{disc}/sys/main.dol")))?,
                        id as u16,
                    )?
                );
            }
            assert!(
                published
                    .inputs
                    .keys()
                    .all(|path| !path.ends_with("btlvoice.afs"))
            );
            if let Some(expected) = &expected {
                assert_eq!(&bytes, expected);
            } else {
                expected = Some(bytes);
            }
        }
        Ok(())
    }
}
