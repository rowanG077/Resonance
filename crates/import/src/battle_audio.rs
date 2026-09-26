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
    path::Path,
};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub sounds: BTreeSet<u16>,
    pub streams: BTreeSet<u16>,
    pub music: BTreeSet<u16>,
}

/// Maintained IDs from the real opening encounter binding inventory. The normal
/// cooker never depends on a developer checkpoint or reads authored source paths.
pub fn opening_selection() -> Result<Selection> {
    let source = resonance_script_content::FILES
        .iter()
        .find_map(|&(path, source)| (path == "battle/audio-requirements.json").then_some(source))
        .context(
            "opening audio requirements are not published; finish the real binding inventory",
        )?;
    let selection: Selection = serde_json::from_str(source)?;
    ensure!(
        !selection.sounds.is_empty()
            && !selection.streams.is_empty()
            && !selection.music.is_empty(),
        "opening audio requirements are incomplete"
    );
    Ok(selection)
}

pub struct Publication {
    pub files: Vec<String>,
    /// Original paths relative to the extracted disc (including files/ or sys/).
    pub inputs: BTreeMap<String, String>,
}

/// Decode only the requested cue closure. Stream PCM must already be in the
/// verified shared library; it is rebound and copied without re-decoding ADX.
/// The output must be a private staging directory, separate from `library`.
pub fn publish(
    extracted: &Path,
    library: &Path,
    output: &Path,
    coefficients: &[u8],
    selection: &Selection,
) -> Result<Publication> {
    let workspace = Workspace::open(extracted, output)?;
    ensure!(
        library.canonicalize()? != workspace.output,
        "battle audio publication requires a separate staging directory"
    );
    publish_in(&workspace, library, coefficients, selection)
}

/// Reuse the full cook's output owner. The physical voice jobs have completed
/// before this finishing operation; no second media lock or PCM decode is needed.
pub(crate) fn publish_in(
    workspace: &Workspace,
    library: &Path,
    coefficients: &[u8],
    selection: &Selection,
) -> Result<Publication> {
    let extracted = workspace.extracted.as_path();
    let output = workspace.output.as_path();
    let shared_library = library.canonicalize()? == workspace.output;
    ensure!(
        !selection.music.is_empty() || !selection.sounds.is_empty(),
        "battle audio needs a prepared mixer package"
    );
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
    let sources = crate::source_assets::Sources::read(extracted)?;
    let module = crate::rel::Rel::read(&extracted.join("files").join(&sources.module))?;
    let pools = Pools::read(extracted)?;
    let mut input_paths: BTreeSet<String> = [
        "sys/main.dol".into(),
        format!("files/{}", sources.module),
        format!("files/{}", sources.usual),
    ]
    .into();
    // Battle songs, results and cues share preset 2. The original reverb lookup
    // remains authoritative; a request with another environment fails below.
    let reverbs = media::music::song_reverbs(&executable, 85)?;
    let mut files = BTreeMap::new();
    let mut publish = |package: Package| -> Result<Asset> {
        ensure!(
            package.reverbs == reverbs,
            "selected battle song changes studio environment"
        );
        let bytes = serde_json::to_vec(&package)?;
        let path = format!("audio/prepared/{}.json", crate::digest(&bytes));
        crate::write_atomic(&output.join(&path), &bytes)?;
        inventory(output, &mut files, &path, Role::AudioPackage)?;
        for sample in package.samples.values() {
            inventory(output, &mut files, &sample.path, Role::InstrumentSample)?;
        }
        Ok(Asset {
            path,
            sha256: crate::digest(&bytes),
        })
    };
    let mut music = BTreeMap::new();
    for &id in &selection.music {
        input_paths.insert(format!("files/{}", media::music_path(&executable, id)?));
        music.insert(
            i16::try_from(id)?,
            publish(media::music_library::package(
                workspace,
                &executable,
                coefficients,
                &pools,
                id,
                Some(reverbs),
            )?)?,
        );
    }
    let [instruments, common] = roles::resident_banks(extracted, &executable)?;
    let party = roles::party_banks(extracted, &executable)?;
    let mut groups = BTreeMap::new();
    let mut sounds = BTreeMap::new();
    let voice_bank = roles::voice_bank_path(extracted)?;
    input_paths.insert(format!("files/{voice_bank}"));
    input_paths.insert(format!("files/{instruments}"));
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
            let (resources, score) = media::sound_library::sound(&bank, id)
                .with_context(|| format!("battle sound {id} from {source}"))?;
            let package = media::sound_library::package(
                output,
                &resources,
                score,
                media::synthesis_tables(&executable, coefficients)?,
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
    let directory = crate::voice_directory::Directory::read(&executable)?;
    let source = directory
        .active()
        .find(|entry| entry.resource_id == 0x78)
        .context("missing original battle stream declaration")?
        .source_path()?;
    input_paths.insert(format!("files/{source}"));
    let mut archive = media::voice_library::Archive::open(library, extracted, &source)?;
    let mut voices = BTreeMap::new();
    for &id in &selection.streams {
        ensure!(id < 0x8000, "invalid battle stream {id}");
        let voice = archive.voice(usize::from(id))?;
        if !shared_library {
            let bytes = fs::read(library.join(&voice.asset.path))?;
            crate::write_atomic(&output.join(&voice.asset.path), &bytes)?;
        }
        inventory(output, &mut files, &voice.asset.path, Role::Voice)?;
        voices.insert(u32::from(id), voice);
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
            music,
            sounds,
            voices,
            voice_gains: media::field_audio::voice_gains(&executable)?,
        },
        voice_pan: media::field_audio::voice_pan(&executable)?,
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
    #[ignore = "requires both original discs, existing shared audio library and DSP coefficients; no device"]
    fn selected_original_battle_packages_preserve_both_disc_identity() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let library = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| local.join("all-assets"));
        let coefficients = fs::read(
            std::env::var_os("RESONANCE_DSP_COEFFICIENTS")
                .context("set RESONANCE_DSP_COEFFICIENTS")?,
        )?;
        let selection = Selection {
            sounds: [60].into(),
            streams: BTreeSet::new(),
            music: [85, 95].into(),
        };
        let mut expected = None;
        for disc in [1, 2] {
            let output = tempfile::tempdir()?;
            let published = publish(
                &local.join(format!("extracted/disc{disc}")),
                &library,
                output.path(),
                &coefficients,
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
            let direct = publish_in(&workspace, in_place.path(), &coefficients, &selection)?;
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
                [85, 95]
            );
            assert_eq!(published.files.len(), audio.files.len() + 1);
            assert_eq!(audio.effect_spatial, [320., 5., 64., 0., 127.]);
            assert_eq!(audio.voice_spatial, [320., 5., 64., 24., 104.]);
            for asset in audio
                .assets
                .music
                .values()
                .chain(audio.assets.sounds.values())
            {
                let loaded = Package::load(output.path(), &asset.path)?;
                assert_eq!(
                    loaded.reverbs,
                    [[0.7, 0.7, 2.5, 0.6, 0.05], [1., 0.5, 1., 0.8, 0.01]]
                );
            }
            if let Some(expected) = &expected {
                assert_eq!(&bytes, expected);
            } else {
                expected = Some(bytes);
            }
        }
        Ok(())
    }
}
