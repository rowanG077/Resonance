//! Field audio recipes, using original resource tables and Rust synthesis.
use super::{Workspace, hash_file, write_json, write_sample_assets};
use anyhow::{Context, Result, ensure};
use resonance_audio::package::Package;
use resonance_audio_cook::{bank::Bank, compile, song::Song};
use resonance_content::field_audio::{Asset, FieldAudio, Voice};
use serde_json::json;
use std::{collections::BTreeMap, fs, io::ErrorKind, path::Path};
mod resources;

/// Resolve the saved setting through CRI attenuation and the stream mixer.
/// The runtime consumes amplitudes, without executable addresses or codec tables.
fn voice_gains(executable: &[u8]) -> Result<Vec<f32>> {
    let attenuation = crate::dol::slice(executable, 0x801f_9bc4, 128)?;
    let stream_volume = crate::dol::slice(executable, 0x802b_4680, 1000)?;
    let mix = super::sound_buses::tables(executable)?;
    attenuation
        .iter()
        .map(|&db| {
            let volume = *stream_volume
                .get(usize::from(db) * 10)
                .context("invalid dialogue attenuation")?;
            ensure!(volume <= 127, "invalid dialogue stream volume");
            Ok(mix.volume[usize::from(volume)])
        })
        .collect()
}

/// Cook every declared scenario branch, message voice and native service cue.
pub fn cook_field_audio(
    extracted: &Path,
    output: &Path,
    map_id: u32,
    coefficients: &Path,
    additional_disc: Option<&Path>,
) -> Result<()> {
    let workspace = Workspace::open(extracted, output)?;
    if let Some(disc) = additional_disc {
        validate_additional_disc(disc)?;
    }
    let map = crate::field::MapArchive::open(&crate::field::source_for_id(extracted, map_id)?)
        .with_context(|| format!("read field {map_id} source archive"))?;
    let executable = read_file(&extracted.join("sys/main.dol"))?;
    let resources = resources::Resources::read(extracted, &executable, map.section(6)?)
        .with_context(|| format!("inventory field {map_id} audio"))?;
    let coefficients = read_file(coefficients)?;
    let mut archives = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut voice_sources = BTreeMap::new();
    for group in resources
        .voices
        .iter()
        .map(|id| id & 0xffff_0000)
        .collect::<std::collections::BTreeSet<_>>()
    {
        let source = voice_path(&executable, group)?;
        let (bytes, disc) = voice_archive(extracted, additional_disc, &source)
            .with_context(|| format!("read field {map_id} voice group {group:#x}"))?;
        voice_sources.insert(
            group,
            json!({"game":"GQSEAF", "revision":0, "disc":disc,
            "path":source,"sha256":crate::digest(&bytes)}),
        );
        archives.insert(group, bytes);
    }
    for source in ["S/inst.snd".to_owned()]
        .into_iter()
        .chain(resources.banks.iter().map(|(source, _)| source.clone()))
        .chain(
            resources
                .music
                .iter()
                .map(|id| music_path(&executable, *id))
                .collect::<Result<Vec<_>>>()?,
        )
    {
        sources.insert(
            source.clone(),
            hash_file(&extracted.join("files").join(&source))
                .with_context(|| format!("hash field {map_id} audio source {source}"))?,
        );
    }
    let recipe = json!({"version":6,"map_id":map_id,"map_sha256":map.source_sha256,"sound_banks":resources.banks,
        "executable_sha256":crate::digest(&executable),"coefficients_sha256":crate::digest(&coefficients),
        "voice_decoder":super::AHX_DECODER,
        "compiler_sha256":hash_file(&std::env::current_exe()?)?,"sources":sources,"voice_sources":voice_sources,"audio_device":false});
    let metadata = output.join(if map_id == 340 {
        "fields/iselia-classroom-audio.json".into()
    } else {
        format!("fields/map-{map_id}-audio.json")
    });
    if let Some(previous) = super::json_file(&metadata)
        && let Ok(previous) = serde_json::from_value::<FieldAudio>(previous)
        && previous.recipe == recipe
        && previous
            .music
            .keys()
            .copied()
            .eq(resources.music.iter().map(|id| *id as i16))
        && previous
            .sounds
            .keys()
            .copied()
            .eq(resources.sounds.iter().map(|id| *id as i16))
        && previous
            .voices
            .keys()
            .copied()
            .eq(resources.voices.iter().copied())
        && current(output, &previous)?
    {
        println!("Field {map_id} audio is current");
        return crate::field::refresh_preloads(output);
    }
    let reverbs = super::music::title_reverbs(&executable)?;
    let manifest = FieldAudio {
        version: FieldAudio::VERSION,
        voice_gains: voice_gains(&executable)?,
        music: cook_music(
            &workspace,
            &executable,
            &coefficients,
            resources.music,
            reverbs,
        )?,
        sounds: cook_sounds(
            &workspace,
            &executable,
            &coefficients,
            reverbs,
            "field-sound",
            &resources.banks,
        )?,
        voices: cook_voices(&workspace, &archives, &resources.voices)?,
        recipe,
    };
    manifest.validate()?;
    write_json(&metadata, &serde_json::to_value(&manifest)?)?;
    println!(
        "Cooked field {map_id}: {} scores, {} cues, {} voices without playback",
        manifest.music.len(),
        manifest.sounds.len(),
        manifest.voices.len()
    );
    crate::field::refresh_preloads(output)
}

fn cook_voices(
    workspace: &Workspace,
    archives: &BTreeMap<u32, Vec<u8>>,
    ids: &std::collections::BTreeSet<u32>,
) -> Result<BTreeMap<u32, Voice>> {
    fs::create_dir_all(workspace.output.join("audio/voices"))?;
    let mut voices = BTreeMap::new();
    for (&group, archive) in archives {
        let members = crate::afs::parse(archive)?;
        for &id in ids.range(group..=(group | 0xffff)) {
            let member = members
                .get((id & 0xffff) as usize)
                .context("spoken line is missing from its AFS archive")?;
            let path = format!("audio/voices/{id:08x}.wav");
            let target = workspace.output.join(&path);
            let temporary = target.with_extension("partial.wav");
            let mut decoder = ahx_rs::Decoder::new(member.data)
                .with_context(|| format!("decode voice {id:#x}"))?;
            let info = decoder.metadata();
            ensure!(
                info.sample_rate() == 32000 && (1..=32_000_000).contains(&info.samples()),
                "voice decoder did not produce bounded 32 kHz PCM16"
            );
            let frames = info.samples();
            // Relabel the declared synthesis rate to the actual DAC clock,
            // preserving every decoded sample without resampling.
            let mut writer = hound::WavWriter::create(
                &temporary,
                hound::WavSpec {
                    channels: info.channels(),
                    sample_rate: super::PLAYBACK_RATE,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )?;
            while let Some(pcm) = decoder.next_block()? {
                for &sample in pcm {
                    writer.write_sample(sample)?;
                }
            }
            writer.finalize()?;
            fs::rename(&temporary, &target)?;
            voices.insert(
                id,
                Voice {
                    asset: Asset {
                        path,
                        sha256: hash_file(&target)
                            .with_context(|| format!("hash cooked voice {}", target.display()))?,
                    },
                    frames,
                    sample_rate: super::PLAYBACK_RATE,
                    source_sample_rate: info.sample_rate(),
                    channels: info.channels(),
                    source_name: member.name.into(),
                    source_sha256: crate::digest(member.data),
                },
            );
        }
    }
    Ok(voices)
}

fn cook_music(
    workspace: &Workspace,
    executable: &[u8],
    coefficients: &[u8],
    ids: impl IntoIterator<Item = u16>,
    reverbs: [[f32; 5]; 2],
) -> Result<BTreeMap<i16, Asset>> {
    let bytes = read_file(&workspace.extracted.join("files/S/inst.snd"))?;
    let bank = Bank::parse(&bytes)?;
    ids.into_iter()
        .map(|id| {
            let source = music_path(executable, id)?;
            let bytes = read_file(&workspace.extracted.join("files").join(&source))?;
            ensure!(
                crate::dol::slice(executable, 0x802108b0 + u32::from(id), 1)? == [1],
                "field music {id} requires another reverb preset"
            );
            let song =
                Song::parse(&bytes).with_context(|| format!("decode music {id}: {source}"))?;
            let (resources, score) = compile::music(&bank, &song, &bank.music_setup(0, id)?)?;
            let asset = cook_package(
                workspace,
                &format!("field-music-{id}"),
                resources,
                score,
                super::music_voice::tables(executable, coefficients)?,
                reverbs,
            )?;
            println!("Cooked field music {id}: {source}");
            Ok((id as i16, asset))
        })
        .collect()
}

fn current(root: &Path, manifest: &FieldAudio) -> Result<bool> {
    if manifest.validate().is_err() {
        return Ok(false);
    }
    for asset in manifest.music.values().chain(manifest.sounds.values()) {
        if hash_file(&root.join(&asset.path)).ok().as_ref() != Some(&asset.sha256)
            || Package::load(root, &asset.path).is_err()
        {
            return Ok(false);
        }
    }
    for voice in manifest.voices.values() {
        if hash_file(&root.join(&voice.asset.path)).ok().as_ref() != Some(&voice.asset.sha256) {
            return Ok(false);
        }
        let wave = hound::WavReader::open(root.join(&voice.asset.path))
            .with_context(|| format!("read cooked voice {}", voice.asset.path))?;
        let spec = wave.spec();
        if wave.duration() != voice.frames
            || spec.sample_rate != voice.sample_rate
            || spec.channels != voice.channels
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn cook_sounds(
    workspace: &Workspace,
    executable: &[u8],
    coefficients: &[u8],
    reverbs: [[f32; 5]; 2],
    prefix: &str,
    banks: &[(impl AsRef<str>, Vec<u16>)],
) -> Result<BTreeMap<i16, Asset>> {
    use resonance_audio::{
        data::{Event, EventKind, Score},
        music_voice::Controls,
    };
    let mut assets = BTreeMap::new();
    let common_bytes = read_file(&workspace.extracted.join("files/S/se.snd"))?;
    let common = Bank::parse(&common_bytes)?;
    let instrument_bytes = read_file(&workspace.extracted.join("files/S/inst.snd"))?;
    let instruments = Bank::parse(&instrument_bytes)?;
    for (source, ids) in banks {
        let source = source.as_ref();
        let bytes = read_file(&workspace.extracted.join("files").join(source))?;
        let mut bank = Bank::parse(&bytes)?;
        bank.inherit_tables(&common);
        bank.inherit_tables(&instruments);
        bank.inherit_samples(&instruments);
        bank.inherit_samples(&common);
        for &id in ids {
            ensure!(
                !assets.contains_key(&(id as i16)),
                "ambiguous cooked sound {id}"
            );
            let sound = bank.sound(id)?;
            let layers = resonance_audio_cook::instrument::resolve(
                &bank,
                resonance_audio_cook::bank::Page {
                    object: sound.object,
                    priority: 64,
                    max_voices: 255,
                },
                sound.key,
                sound.volume,
                sound.pan,
            )?;
            let resources = compile::programs(&bank, layers.iter().map(|l| l.macro_id))
                .with_context(|| format!("compile {source} sound {id}"))?;
            let score = Score {
                initial_bpm_1024: 120 * 1024,
                loop_start_tick: 0,
                end_tick: 65535,
                has_master_track: false,
                tempos: vec![],
                controls: [Controls::default(); 16],
                first_events: vec![Event {
                    tick: 0,
                    channel: 0,
                    kind: EventKind::Notes {
                        voices: layers,
                        length: 65535,
                    },
                }],
                loop_events: vec![],
            };
            let tables = super::music_voice::tables(executable, coefficients)?;
            assets.insert(
                id as i16,
                cook_package(
                    workspace,
                    &format!("{prefix}-{id}"),
                    resources,
                    score,
                    tables,
                    reverbs,
                )?,
            );
        }
    }
    Ok(assets)
}

fn cook_package(
    workspace: &Workspace,
    name: &str,
    resources: resonance_audio::data::Resources,
    score: resonance_audio::data::Score,
    tables: resonance_audio::music_voice::Tables,
    reverbs: [[f32; 5]; 2],
) -> Result<Asset> {
    let directory = format!("audio/field-instruments/{name}");
    fs::create_dir_all(workspace.output.join(&directory))?;
    let samples = write_sample_assets(&workspace.output, &resources, |id| {
        format!("{directory}/{id}.wav")
    })?;
    let package = Package {
        version: resonance_audio::package::VERSION,
        programs: resources.programs,
        samples,
        score,
        tables,
        reverbs,
    };
    let path = format!("audio/{name}.json");
    write_json(
        &workspace.output.join(&path),
        &serde_json::to_value(&package)?,
    )?;
    Package::load(&workspace.output, &path)?;
    Ok(Asset {
        sha256: hash_file(&workspace.output.join(&path))
            .with_context(|| format!("hash cooked audio {path}"))?,
        path,
    })
}
fn read_file(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("read {}", path.display()))
}

fn validate_additional_disc(disc: &Path) -> Result<()> {
    ensure!(
        read_file(&disc.join("sys/boot.bin"))?.get(..8) == Some(b"GQSEAF\x01\0"),
        "additional voice disc must be GQSEAF revision 0 disc 2: {}",
        disc.display()
    );
    Ok(())
}

fn voice_archive(primary: &Path, additional: Option<&Path>, source: &str) -> Result<(Vec<u8>, u8)> {
    let path = primary.join("files").join(source);
    match fs::read(&path) {
        Ok(bytes) => Ok((bytes, 1)),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let additional = additional.with_context(|| {
                format!(
                    "voice archive {} is missing; extract Disc 2 and supply --additional-disc",
                    path.display()
                )
            })?;
            Ok((read_file(&additional.join("files").join(source))?, 2))
        }
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn string(executable: &[u8], address: u32) -> Result<String> {
    let bytes = crate::dol::slice(executable, address, 64)?;
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .context("unterminated audio resource path")?;
    let path = std::str::from_utf8(&bytes[..end])?;
    // The extracted disc retains uppercase directory names; table paths are lowercase.
    let (directory, name) = path
        .split_once('/')
        .context("audio path has no directory")?;
    let path = format!("{}/{name}", directory.to_ascii_uppercase());
    resonance_content::validate_asset_path(&path)?;
    Ok(path)
}
fn music_path(executable: &[u8], id: u16) -> Result<String> {
    for entry in crate::dol::slice(executable, 0x801f982c, 0x450)?.chunks_exact(8) {
        if u16::from_be_bytes(entry[..2].try_into()?) == id {
            return string(executable, u32::from_be_bytes(entry[4..].try_into()?));
        }
    }
    anyhow::bail!("music {id} is not in the original resource table")
}
fn voice_path(executable: &[u8], group: u32) -> Result<String> {
    for entry in crate::dol::slice(executable, 0x802a30a0, 0xd8)?.chunks_exact(12) {
        if u32::from_be_bytes(entry[4..8].try_into()?) == group {
            return string(executable, u32::from_be_bytes(entry[..4].try_into()?));
        }
    }
    anyhow::bail!("voice group {group:#x} is not in the original resource table")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn additional_voice_disc_only_fills_missing_sources_and_requires_matching_disc_identity() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "resonance-voice-disc-{}-{nonce}",
            std::process::id()
        ));
        let primary = root.join("disc1");
        let additional = root.join("disc2");
        for directory in [
            primary.join("files/EV"),
            additional.join("files/EV"),
            additional.join("sys"),
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        let source = "EV/voices.afs";
        let first = primary.join("files").join(source);
        let second = additional.join("files").join(source);
        fs::write(additional.join("sys/boot.bin"), b"GQSEAF\x01\0").unwrap();
        validate_additional_disc(&additional).unwrap();
        fs::write(&first, b"primary").unwrap();
        fs::write(&second, b"additional").unwrap();
        assert_eq!(
            voice_archive(&primary, Some(&additional), source).unwrap(),
            (b"primary".to_vec(), 1)
        );
        fs::remove_file(&first).unwrap();
        assert_eq!(
            voice_archive(&primary, Some(&additional), source).unwrap(),
            (b"additional".to_vec(), 2)
        );
        assert!(
            voice_archive(&primary, None, source)
                .unwrap_err()
                .to_string()
                .contains("--additional-disc")
        );
        fs::create_dir(&first).unwrap();
        assert!(
            voice_archive(&primary, Some(&additional), source).is_err(),
            "non-missing primary errors must not fall back"
        );
        for boot in [b"GQSEAF\0\0", b"GQSEAF\x01\x01", b"GQSPAF\x01\0"] {
            fs::write(additional.join("sys/boot.bin"), boot).unwrap();
            assert!(validate_additional_disc(&additional).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
