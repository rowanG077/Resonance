//! Bind the complete configured catalogue from the shared, already cooked library.
use super::*;
use crate::media::{library::Cooker, sound_library::Resources};
use resonance_content::field_audio::archive::MemberKind;
use std::{path::PathBuf, sync::Arc};

#[derive(serde::Deserialize)]
struct Song {
    version: u32,
    programs: BTreeMap<u16, Vec<resonance_audio::data::Command>>,
    samples: BTreeMap<u16, resonance_audio::package::SampleAsset>,
    score: resonance_audio::data::Score,
    tables: resonance_audio::music_voice::Tables,
    reverb: media::music::ReverbChange,
}

/// Stage catalogue/package JSON against an existing physical library. Returned
/// paths include retained PCM dependencies; those files are not copied here.
pub fn publish_all(discs: &[PathBuf], library: &Path, output: &Path) -> Result<Vec<String>> {
    let session = media::OutputSession::open(output)?;
    let _publications = crate::publication::Session::start_if_needed(output)?;
    publish_all_from(discs, library, &session, &mut BTreeMap::new())
}

/// Finishing pass only: physical banks, song setups and PCM already exist.
/// Reuses their source/version keys instead of compiling or decoding them again.
pub(crate) fn publish_all_in(
    discs: &[PathBuf],
    session: &Arc<media::OutputSession>,
    sources: &mut BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    let primary = discs.first().context("no audio source disc")?;
    let workspace = session.workspace(primary)?;
    publish_all_from(discs, &workspace.output, session, sources)
}

fn publish_all_from(
    discs: &[PathBuf],
    library: &Path,
    session: &Arc<media::OutputSession>,
    sources: &mut BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    let discs = crate::all_assets::source_discs(discs)?;
    let (&disc, &primary) = discs.first_key_value().context("no audio source disc")?;
    let workspace = session.workspace(primary)?;
    let output = &workspace.output;
    let executable = fs::read(primary.join("sys/main.dol"))?;
    let music_directory = crate::music_directory::Directory::read(&executable)?;
    let mut hashes = BTreeMap::new();
    for entry in music_directory.active() {
        let path =
            crate::field_resources::resolve_path(&primary.join("files"), &entry.source_path()?)?;
        hashes.insert(
            path.clone(),
            media::hash_file(&primary.join("files").join(path))?,
        );
    }
    let cooker = Cooker::new(primary, session, &hashes)?;
    let reverbs = media::music::battle_reverbs(&executable)?;
    let tables = media::synthesis_tables(&executable)?;
    let mut files = BTreeMap::new();
    let mut sounds = BTreeMap::new();
    let mut music = BTreeMap::new();
    let mut voices = BTreeMap::new();
    let mut unavailable = BTreeMap::new();
    let mut missing_archives = Vec::new();
    let mut input_sources = BTreeSet::from([format!("disc{disc}/sys/main.dol")]);
    let source_assets = crate::source_assets::Sources::read_with(primary, &executable)?;
    for source in [
        source_assets.usual,
        source_assets.enemy,
        roles::voice_bank_path(primary)?,
    ] {
        input_sources.insert(format!("disc{disc}/{source}"));
    }

    let mut publish = |package: Package| -> Result<Asset> {
        compatible_reverbs(package.reverbs, reverbs)?;
        let bytes = serde_json::to_vec(&package)?;
        let path = format!("audio/prepared/{}.json", crate::digest(&bytes));
        let asset = media::field_audio::write_package_from(library, output, &path, &package)?;
        inventory(output, &mut files, &path, Role::AudioPackage)?;
        for sample in package.samples.values() {
            if !files.contains_key(&sample.path) {
                inventory(library, &mut files, &sample.path, Role::InstrumentSample)?;
            }
        }
        Ok(asset)
    };
    let mut groups = BTreeMap::new();
    let mut authored_sounds = BTreeSet::new();
    for source in media::library::bank_sources(primary, &executable)? {
        let bytes = fs::read(primary.join("files").join(&source))?;
        let bank = Bank::parse(&bytes)?;
        let group = bank.group()?;
        let hash = crate::digest(&bytes);
        input_sources.insert(format!("disc{disc}/{source}"));
        if let Some(previous) = groups.insert(group, hash.clone()) {
            ensure!(previous == hash, "conflicting sound group {group}");
            continue;
        }
        let directory = format!("audio/banks/{}", cooker.resource_key(&hash));
        for id in bank.sound_ids() {
            ensure!(authored_sounds.insert(id), "ambiguous sound {id}");
            let path = format!("{directory}/sound-{id}.json");
            let value: serde_json::Value = serde_json::from_slice(
                &fs::read(library.join(&path))
                    .with_context(|| format!("missing compiled sound {path}"))?,
            )
            .with_context(|| format!("decode compiled sound {path}"))?;
            if value.get("state").and_then(serde_json::Value::as_str)
                == Some("unavailable_source_dependency")
            {
                // Preserve the physical cooker's explicit source defect. This
                // is catalogue accounting, not a playable silent replacement.
                unavailable.insert(
                    id,
                    serde_json::json!({"source":source,"record":path,"declaration":value}),
                );
                continue;
            }
            let resources: Resources = serde_json::from_value(value)
                .with_context(|| format!("decode compiled sound resources {path}"))?;
            ensure!(
                resources.version == media::sound_library::VERSION,
                "stale sound {path}"
            );
            let package = Package {
                version: resonance_audio::package::VERSION,
                programs: resources.programs,
                samples: resources.samples,
                score: resources.score.context("compiled sound has no score")?,
                tables: tables.clone(),
                reverbs,
            };
            sounds.insert(
                i16::try_from(id)?,
                publish(package).with_context(|| format!("publish sound {id} from {path}"))?,
            );
        }
    }
    for entry in music_directory.active() {
        let id = u16::try_from(entry.id)?;
        let source =
            crate::field_resources::resolve_path(&primary.join("files"), &entry.source_path()?)?;
        input_sources.insert(format!("disc{disc}/{source}"));
        let path = format!(
            "audio/songs/{}/setup-{id}.json",
            cooker.resource_key(&hashes[&source])
        );
        let song: Song = serde_json::from_slice(
            &fs::read(library.join(&path))
                .with_context(|| format!("missing compiled song {path}"))?,
        )
        .with_context(|| format!("decode compiled song {path}"))?;
        ensure!(
            song.version == media::sound_library::VERSION,
            "stale song {path}"
        );
        let package = Package {
            version: resonance_audio::package::VERSION,
            programs: song.programs,
            samples: song.samples,
            score: song.score,
            tables: song.tables,
            reverbs: match song.reverb {
                media::music::ReverbChange::Keep => reverbs,
                media::music::ReverbChange::Set { parameters } => parameters,
            },
        };
        music.insert(
            entry.id,
            publish(package).with_context(|| format!("publish song {id} from {path}"))?,
        );
    }
    drop(publish);

    let directory = crate::voice_directory::Directory::read(&executable)?;
    for entry in directory.active() {
        let declared = entry.source_path()?;
        let mut owner = None;
        for (&disc, &extracted) in &discs {
            if let Some(source) =
                crate::field_resources::find_path(&extracted.join("files"), &declared)?
            {
                owner = Some((disc, extracted, source));
                break;
            }
        }
        let Some((disc, extracted, source)) = owner else {
            missing_archives.push(declared);
            continue;
        };
        input_sources.insert(format!("disc{disc}/{source}"));
        let mut archive = media::voice_library::Archive::open(library, extracted, &source)?;
        for id in 0..archive.index.members.len() {
            if archive.index.members[id].kind == MemberKind::Empty {
                continue;
            }
            let metadata = archive.voice(id)?;
            let mut voice = metadata.voice;
            // AHX field/event lines use the field playback clock; the battle
            // ADX alias retains its native rate, exactly as the existing binders.
            if metadata.source_name.ends_with(".ahx")
                && voice.source_sample_rate == media::SAMPLE_RATE
            {
                voice.sample_rate = media::PLAYBACK_RATE;
            }
            if !files.contains_key(&voice.asset.path) {
                inventory(library, &mut files, &voice.asset.path, Role::Voice)?;
            }
            let id = u16::try_from(id)?;
            if entry.resource_id == 0x78 {
                ensure!(id < 0x8000, "battle stream ID exceeds its domain");
                ensure!(
                    voices.insert(u32::from(id), voice.clone()).is_none(),
                    "duplicate battle stream {id}"
                );
            }
            let logical = entry.logical_base | u32::from(id);
            ensure!(
                voices.insert(logical, voice).is_none(),
                "duplicate voice {logical:#x}"
            );
        }
    }
    let module_path =
        crate::field_resources::resolve_path(&primary.join("files"), "US_r_Top2Btl.rel")?;
    input_sources.insert(format!("disc{disc}/{module_path}"));
    let module = crate::rel::Rel::read(&primary.join("files").join(module_path))?;
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
    let coverage = "battle/audio-coverage.json";
    crate::write_atomic(
        &output.join(coverage),
        &serde_json::to_vec_pretty(&serde_json::json!({
            "authored_sounds": authored_sounds.len(),
            "playable_sounds": descriptor.assets.sounds.len(),
            "music": descriptor.assets.music.len(),
            "voice_bindings": descriptor.assets.voices.len(),
            "unavailable_sounds": unavailable,
            "unavailable_archives": missing_archives,
            "sources": input_sources,
        }))?,
    )?;
    crate::write_atomic(&output.join(PATH), &serde_json::to_vec(&descriptor)?)?;
    for source in input_sources {
        let aliases = sources.entry(source).or_default();
        aliases.extend([PATH.to_owned(), coverage.to_owned()]);
        aliases.sort();
        aliases.dedup();
    }
    Ok([PATH.to_owned(), coverage.to_owned()]
        .into_iter()
        .chain(descriptor.files.into_keys())
        .collect())
}
