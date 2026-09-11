//! Field audio recipes, using original resource tables and Rust synthesis.
use super::{Tool, Workspace, hash_file, write_json, write_pcm16, write_sample_assets};
use anyhow::{Context, Result, ensure};
use resonance_audio::package::Package;
use resonance_audio_cook::{bank::Bank, compile, song::Song};
use resonance_content::field_audio::{Asset, FieldAudio, Voice};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, time::Duration};

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

pub fn cook_classroom_audio(
    extracted: &Path,
    output: &Path,
    coefficients: &Path,
    decoder: &Path,
) -> Result<()> {
    let workspace = Workspace::open(extracted, output)?;
    let executable = fs::read(workspace.extracted.join("sys/main.dol"))?;
    let decoder = Tool::resolve(decoder)?;
    let coefficient_bytes = fs::read(coefficients)?;
    let bank_bytes = fs::read(workspace.extracted.join("files/S/inst.snd"))?;
    let reverbs = super::music::title_reverbs(&executable)?;
    let voice_path = voice_path(&executable, 0xa0000)?;
    let archive = fs::read(workspace.extracted.join("files").join(&voice_path))?;
    let mut sources = BTreeMap::new();
    for source in [
        music_path(&executable, 7)?,
        music_path(&executable, 82)?,
        "S/se.snd".into(),
        "S/se_ev02.snd".into(),
    ] {
        sources.insert(
            source.clone(),
            hash_file(&workspace.extracted.join("files").join(source))?,
        );
    }
    let recipe = json!({"version":3,"executable_sha256":crate::digest(&executable),"instrument_bank_sha256":crate::digest(&bank_bytes),
        "coefficients_sha256":crate::digest(&coefficient_bytes),"voice_archive":voice_path,"voice_archive_sha256":crate::digest(&archive),
        "voice_decoder_sha256":decoder.hash,"compiler_sha256":hash_file(&std::env::current_exe()?)?,"sources":sources,"audio_device":false});
    let metadata = workspace.output.join("fields/iselia-classroom-audio.json");
    if let Some(previous) = super::json_file(&metadata)
        && let Ok(previous) = serde_json::from_value::<FieldAudio>(previous)
        && previous.recipe == recipe
        && previous.music.keys().copied().eq([7, 82])
        && previous
            .sounds
            .keys()
            .copied()
            .eq([1, 2, 3, 4, 33, 38, 80, 104, 132, 236, 452])
        && previous
            .voices
            .keys()
            .copied()
            .eq((0..32).chain([0x173, 0x174]).map(|index| 0xa0000 + index))
        && current(&workspace.output, &previous)?
    {
        println!("Classroom audio is current");
        crate::field::refresh_preloads(&workspace.output)?;
        return Ok(());
    }
    let music = cook_music(
        &workspace,
        &executable,
        &coefficient_bytes,
        [7, 82],
        reverbs,
    )?;
    let sounds = cook_sounds(
        &workspace,
        &executable,
        &coefficient_bytes,
        reverbs,
        "field-sound",
        &[
            ("S/se.snd", vec![1, 2, 3, 4, 33, 38, 80, 104, 132, 236]),
            ("S/se_ev02.snd", vec![452]),
        ],
    )?;
    let members = crate::afs::parse(&archive)?;
    let intermediate = workspace.output.join("intermediate/field-voices");
    fs::create_dir_all(&intermediate)?;
    fs::create_dir_all(workspace.output.join("audio/voices"))?;
    let mut voices = BTreeMap::new();
    // Control-9 IDs reached in the opening classroom. The doorway conversation
    // is unvoiced; its party-join fanfare is a separate score-backed cue.
    for index in (0..32u32).chain([0x173, 0x174]) {
        let id = 0xa0000 + index;
        let member = members
            .get(index as usize)
            .context("spoken line is missing from its AFS archive")?;
        let raw = intermediate.join(format!("{id:08x}.ahx"));
        fs::write(&raw, member.data)?;
        let path = format!("audio/voices/{id:08x}.wav");
        let target = workspace.output.join(&path);
        let temporary = target.with_extension("partial.wav");
        let mut command = decoder.command(&intermediate);
        command.arg("-i").arg("-o").arg(&temporary).arg(&raw);
        super::process::pipe(
            &mut command,
            &intermediate.join(format!("{id:08x}.log")),
            Duration::from_secs(30),
            |_| Ok(()),
        )?;
        let mut wave = hound::WavReader::open(&temporary)?;
        let spec = wave.spec();
        ensure!(
            spec.bits_per_sample == 16
                && spec.sample_format == hound::SampleFormat::Int
                && spec.sample_rate == 32000
                && (1..=2).contains(&spec.channels)
                && (1..=32_000_000).contains(&wave.duration()),
            "voice decoder did not produce bounded 32 kHz PCM16"
        );
        let frames = wave.duration();
        let pcm = wave
            .samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(wave);
        // The AHX header describes the nominal synthesis rate. As with the
        // music bank, Dolphin consumes these samples at the actual DAC clock.
        // Relabel the WAV in Rust; preserve every decoded sample unchanged.
        write_pcm16(&temporary, spec.channels, super::PLAYBACK_RATE, pcm)?;
        fs::rename(&temporary, &target)?;
        voices.insert(
            id,
            Voice {
                asset: Asset {
                    path,
                    sha256: hash_file(&target)?,
                },
                frames,
                sample_rate: super::PLAYBACK_RATE,
                source_sample_rate: spec.sample_rate,
                channels: spec.channels,
                source_name: member.name.into(),
                source_sha256: crate::digest(member.data),
            },
        );
    }
    let manifest = FieldAudio {
        version: FieldAudio::VERSION,
        voice_gains: voice_gains(&executable)?,
        music,
        sounds,
        voices,
        recipe,
    };
    manifest.validate()?;
    write_json(
        &workspace.output.join("fields/iselia-classroom-audio.json"),
        &serde_json::to_value(&manifest)?,
    )?;
    println!(
        "Cooked {} music scores, {} cues, and {} spoken lines without playback",
        manifest.music.len(),
        manifest.sounds.len(),
        manifest.voices.len()
    );
    crate::field::refresh_preloads(&workspace.output)?;
    Ok(())
}

/// Cook unvoiced field scripts with statically declared music and common cues.
/// Voiced or dynamically selected banks still require their own complete recipe.
pub fn cook_field_audio(
    extracted: &Path,
    output: &Path,
    map_id: u32,
    coefficients: &Path,
) -> Result<()> {
    use crate::field_resources::{declarations, literal_calls};
    use symphonia_script::{NativeCall, message, scenario};
    let workspace = Workspace::open(extracted, output)?;
    let source = crate::field::source_for_id(extracted, map_id)?;
    let map = crate::field::MapArchive::open(&source)?;
    let script = map.section(6)?;
    let messages = message::parse(&script[scenario::parse_header(script)?.auxiliary_offset()..])?;
    ensure!(
        !messages
            .iter()
            .flat_map(|m| &m.tokens)
            .any(|t| matches!(t, message::Token::Control { opcode: 9, .. })),
        "field {map_id} needs a spoken-line recipe"
    );
    let music_ids = literal_calls(script, NativeCall::AudioCommand, 1)?
        .into_iter()
        .filter_map(|id| u16::try_from(id).ok())
        .collect::<Vec<_>>();
    let mut sounds = literal_calls(script, NativeCall::PlaySoundSimple, 2)?;
    ensure!(
        literal_calls(script, NativeCall::SelectAudioBank, 1)?
            .iter()
            .all(|id| id & 7 == 0),
        "field {map_id} needs an additional sound-bank recipe"
    );
    sounds.extend(literal_calls(script, NativeCall::PlaySound, 4)?);
    sounds.extend([1, 2, 3, 4, 30, 33, 38, 0x68, 0x84]); // Menus, item recovery and scenery doors.
    if declarations(script)?.save_point {
        sounds.extend([0x21, 0x68]); // Activation and proximity cues.
    }
    let sound_ids: Vec<_> = sounds
        .into_iter()
        .map(u16::try_from)
        .collect::<Result<_, _>>()?;
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let coefficient_bytes = fs::read(coefficients)?;
    let mut sources = BTreeMap::new();
    for name in ["S/inst.snd".to_owned(), "S/se.snd".into()]
        .into_iter()
        .chain(
            music_ids
                .iter()
                .map(|id| music_path(&executable, *id))
                .collect::<Result<Vec<_>>>()?,
        )
    {
        sources.insert(
            name.clone(),
            hash_file(&extracted.join("files").join(name))?,
        );
    }
    let recipe = json!({"version":1,"map_id":map_id,"map_sha256":map.source_sha256,
        "executable_sha256":crate::digest(&executable),"coefficients_sha256":crate::digest(&coefficient_bytes),
        "compiler_sha256":hash_file(&std::env::current_exe()?)?,"sources":sources,"audio_device":false});
    let metadata = output.join(format!("fields/map-{map_id}-audio.json"));
    if let Some(previous) = super::json_file(&metadata)
        && let Ok(previous) = serde_json::from_value::<FieldAudio>(previous)
        && previous.recipe == recipe
        && previous
            .music
            .keys()
            .copied()
            .eq(music_ids.iter().map(|id| *id as i16))
        && previous
            .sounds
            .keys()
            .copied()
            .eq(sound_ids.iter().map(|id| *id as i16))
        && previous.voices.is_empty()
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
            &coefficient_bytes,
            music_ids,
            reverbs,
        )?,
        sounds: cook_sounds(
            &workspace,
            &executable,
            &coefficient_bytes,
            reverbs,
            "field-sound",
            &[("S/se.snd", sound_ids)],
        )?,
        voices: BTreeMap::new(),
        recipe,
    };
    manifest.validate()?;
    write_json(&metadata, &serde_json::to_value(&manifest)?)?;
    println!(
        "Cooked field {map_id}: {} scores and {} cues without playback",
        manifest.music.len(),
        manifest.sounds.len()
    );
    crate::field::refresh_preloads(output)
}

fn cook_music(
    workspace: &Workspace,
    executable: &[u8],
    coefficients: &[u8],
    ids: impl IntoIterator<Item = u16>,
    reverbs: [[f32; 5]; 2],
) -> Result<BTreeMap<i16, Asset>> {
    let bytes = fs::read(workspace.extracted.join("files/S/inst.snd"))?;
    let bank = Bank::parse(&bytes)?;
    ids.into_iter()
        .map(|id| {
            let source = music_path(executable, id)?;
            let bytes = fs::read(workspace.extracted.join("files").join(&source))?;
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
        let wave = hound::WavReader::open(root.join(&voice.asset.path))?;
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
    banks: &[(&str, Vec<u16>)],
) -> Result<BTreeMap<i16, Asset>> {
    use resonance_audio::{
        data::{Event, EventKind, Score},
        music_voice::Controls,
    };
    let mut assets = BTreeMap::new();
    let common_bytes = fs::read(workspace.extracted.join("files/S/se.snd"))?;
    let common = Bank::parse(&common_bytes)?;
    let instrument_bytes = fs::read(workspace.extracted.join("files/S/inst.snd"))?;
    let instruments = Bank::parse(&instrument_bytes)?;
    for (source, ids) in banks {
        let bytes = fs::read(workspace.extracted.join("files").join(source))?;
        let mut bank = Bank::parse(&bytes)?;
        bank.inherit_tables(&common);
        bank.inherit_tables(&instruments);
        bank.inherit_samples(&instruments);
        for &id in ids {
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
            let resources = compile::programs(&bank, layers.iter().map(|l| l.macro_id))?;
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
        sha256: hash_file(&workspace.output.join(&path))?,
        path,
    })
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
    anyhow::bail!("voice group is not in the original resource table")
}
