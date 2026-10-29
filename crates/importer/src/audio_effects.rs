//! Field sound banks directly into named WAVs or editable effect recipes.
//! The shared audio mapping is assembled by the caller alongside battle speech.
use crate::{audio_music, dol};
use anyhow::{Context, Result, ensure};
use resonance_audio::{
    data::{Event, EventKind, Score, ScoreOrigin, VoiceSource},
    effect::{
        Definition, Recipe, Silence,
        recipe::{ProgramSource, SharedProgram},
    },
    music_voice::Controls,
    package::{
        Loaded,
        editable::{Program, Sample as EditableSample, Silence as SampleSilence},
    },
};
use resonance_audio_cook::{
    bank::{Bank, Page},
    decode, instrument,
};
use resonance_content::library::Mappings;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Write},
    path::Path,
    sync::Arc,
};
use symphonia_constants::EffectId;

const BANKS: [&str; 9] = [
    "se.snd",
    "se_ev00.snd",
    "se_ev01.snd",
    "se_ev02.snd",
    "se_ev03.snd",
    "se_ev04.snd",
    "se_ev05.snd",
    "se_ev06.snd",
    "se_ev07.snd",
];

/// Convert the field/common effect catalogue, or one ID for a focused check.
/// The returned targets are relative to `audio/mappings.json`.
pub fn cook_effects(root: &Path, library: &Path, selected: Option<u16>) -> Result<Mappings> {
    let disc = root.join("disc1");
    let exe = fs::read(disc.join("sys/main.dol"))?;
    let directory = disc.join("files/S");
    let native = BANKS
        .iter()
        .map(|name| fs::read(directory.join(name)))
        .collect::<Result<Vec<_>, _>>()?;
    let banks = native
        .iter()
        .map(|bytes| Bank::parse(bytes))
        .collect::<Result<Vec<_>>>()?;
    let instrument_bytes = fs::read(directory.join("inst.snd"))?;
    let instruments = Bank::parse(&instrument_bytes)?;
    let sustains = audio_music::sustains(&exe)?;
    let tables = audio_music::tables(&exe)?;
    let reverbs = audio_music::reverbs(&exe, 1)?;
    let samples: BTreeMap<u16, String> =
        serde_json::from_str(include_str!("effect-sample-names.json"))?;
    let programs: BTreeMap<u16, String> =
        serde_json::from_str(include_str!("effect-program-names.json"))?;
    let mut bindings = BTreeMap::new();
    let mut sample_paths = BTreeSet::new();

    for (bank_index, source) in banks.iter().enumerate() {
        for id in source.sound_ids() {
            if selected.is_some_and(|wanted| wanted != id) {
                continue;
            }
            ensure!(
                !bindings.contains_key(&u32::from(id)),
                "duplicate field effect ID {id}"
            );
            let mut bank = Bank::parse(&native[bank_index])?;
            bank.inherit_objects(&banks[0]);
            bank.inherit_objects(&instruments);
            bank.inherit_unique_objects(&banks);
            bank.inherit_samples(&instruments);
            bank.inherit_samples(&banks[0]);
            let loaded = prepare_sound(&bank, id, &sustains, &tables, reverbs)?;
            let name = EffectId::try_from(id)
                .ok()
                .and_then(EffectId::resource_name)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("cue-{id:04}"));
            let relative = if let Some(wave) =
                resonance_audio::effect::bake::simple_wave(Arc::clone(&loaded))?
            {
                let mut reader = hound::WavReader::new(Cursor::new(&wave))?;
                let silent = reader.samples::<i16>().try_fold(true, |silent, sample| {
                    sample.map(|value| silent && value == 0)
                })?;
                if silent {
                    let relative = format!("effects/silence-{}-frames.json", reader.duration());
                    write_json(
                        &library.join("audio").join(&relative),
                        &Definition::Silence(Silence {
                            frames: reader.duration(),
                            sample_rate: reader.spec().sample_rate,
                        }),
                    )?;
                    relative
                } else {
                    let relative = format!("effects/{name}.wav");
                    write_once(&library.join("audio").join(&relative), &wave)?;
                    relative
                }
            } else {
                let mut editable_samples = BTreeMap::new();
                for (&sample_id, sample) in &loaded.resources().samples {
                    let name = samples
                        .get(&sample_id)
                        .cloned()
                        .unwrap_or_else(|| format!("sample-{sample_id:05}"));
                    let name = if bank_index == 0 {
                        name
                    } else {
                        format!("event-{bank_index:02}-{name}")
                    };
                    let path = format!("audio/effects/samples/{name}.wav");
                    editable_samples.insert(
                        sample_id,
                        write_sample(library, &path, sample, &mut sample_paths)?,
                    );
                }
                let program = Program {
                    programs: loaded.resources().programs.clone(),
                    samples: editable_samples,
                    score: loaded.score().clone(),
                    tables: tables.clone(),
                    reverbs: None,
                };
                let mut recipe = Recipe::from_program(&program)?;
                for (&program_id, instructions) in &program.programs {
                    let Some(old_path) = programs.get(&program_id) else {
                        continue;
                    };
                    let stem = old_path
                        .rsplit_once('/')
                        .map_or(old_path.as_str(), |(_, stem)| stem);
                    // Event banks may reuse a program ID with different local
                    // samples. Keep that bank's editable program distinct.
                    let path = if bank_index == 0 {
                        format!("audio/effects/programs/{stem}")
                    } else {
                        format!("audio/effects/programs/event-{bank_index:02}-{stem}")
                    };
                    let shared_samples = instructions.iter().filter_map(|command| {
                        if let resonance_audio::data::Command::StartSample { sample } = command {
                            Some(*sample)
                        } else { None }
                    }).map(|sample| {
                        Ok((sample, program.samples.get(&sample)
                            .with_context(|| format!("effect {id} program {program_id} sample {sample} is missing"))?.clone()))
                    }).collect::<Result<BTreeMap<_, EditableSample>>>()?;
                    write_json(
                        &library.join(&path),
                        &SharedProgram {
                            instructions: instructions.clone(),
                            samples: shared_samples.clone(),
                        },
                    )?;
                    for sample in shared_samples.keys() {
                        recipe.samples.remove(sample);
                    }
                    recipe
                        .programs
                        .insert(program_id, ProgramSource::Shared(path));
                }
                let relative = format!("effects/{name}.json");
                write_json(
                    &library.join("audio").join(&relative),
                    &Definition::Synthesized(recipe),
                )?;
                relative
            };
            bindings.insert(u32::from(id), relative);
        }
    }
    if let Some(id) = selected {
        ensure!(
            bindings.contains_key(&u32::from(id)),
            "native field effect {id} is missing"
        );
    } else {
        ensure!(
            bindings.len() == 501,
            "incomplete field effect catalogue: {}",
            bindings.len()
        );
    }
    Mappings::new(bindings)
}

/// Convert the SND battle voice cues. These are distinct from streamed AFS
/// speech, although both live under the same human-readable speech directory.
/// Unused native cue slots all select one shared silence definition.
pub fn cook_battle_cues(root: &Path, library: &Path, selected: Option<u16>) -> Result<Mappings> {
    let disc = root.join("disc1");
    let exe = fs::read(disc.join("sys/main.dol"))?;
    let directory = disc.join("files/S");
    let mut sources = fs::read_dir(&directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    sources.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("v_") && name.ends_with(".snd"))
    });
    sources.sort();
    ensure!(sources.len() == 70, "expected 70 native battle voice banks");
    let native = sources
        .iter()
        .map(fs::read)
        .collect::<Result<Vec<_>, _>>()?;
    let banks = native
        .iter()
        .map(|bytes| Bank::parse(bytes))
        .collect::<Result<Vec<_>>>()?;
    let common_bytes = fs::read(directory.join("se.snd"))?;
    let common = Bank::parse(&common_bytes)?;
    let instrument_bytes = fs::read(directory.join("inst.snd"))?;
    let instruments = Bank::parse(&instrument_bytes)?;
    let payloads = battle_payloads(&disc)?;
    let sustains = audio_music::sustains(&exe)?;
    let tables = audio_music::tables(&exe)?;
    let reverbs = audio_music::reverbs(&exe, 1)?;
    let names: BTreeMap<u16, String> = serde_json::from_str(include_str!("battle-cue-names.json"))?;
    let samples: BTreeMap<String, String> =
        serde_json::from_str(include_str!("battle-sample-names.json"))?;
    ensure!(
        names.len() == 741 && samples.len() == 640,
        "incomplete battle cue names"
    );
    let mut bindings = BTreeMap::new();
    let mut missing = Vec::new();
    let mut sample_paths = BTreeSet::new();
    for (index, source) in banks.iter().enumerate() {
        let group = source.group()?;
        ensure!(
            group == 10 + index as u16,
            "unexpected battle voice group {group}"
        );
        let ids = source
            .sound_ids()
            .filter(|id| selected.is_none_or(|wanted| wanted == *id))
            .collect::<Vec<_>>();
        if ids.is_empty() {
            continue;
        }
        let mut bank = Bank::parse(&native[index])?;
        bank.inherit_objects(&common);
        bank.inherit_objects(&instruments);
        bank.inherit_unique_objects(&banks[9..]);
        bank.inherit_samples(&instruments);
        bank.inherit_samples(&common);
        if group >= 19 {
            bank = bank.with_sample_payload(&payloads[usize::from(group - 19)]);
        }
        for id in ids {
            let relative = if let Some(name) = names.get(&id) {
                let loaded = prepare_sound(&bank, id, &sustains, &tables, reverbs)
                    .with_context(|| format!("battle voice cue {id} ({name})"))?;
                if let Some(wave) = resonance_audio::effect::bake::simple_wave(Arc::clone(&loaded))?
                {
                    let relative = format!("speech/battle/{name}.wav");
                    write_once(&library.join("audio").join(&relative), &wave)?;
                    relative
                } else {
                    let mut editable_samples = BTreeMap::new();
                    for (&sample_id, sample) in &loaded.resources().samples {
                        let key = format!("{group}:{sample_id}");
                        let path = if sample
                            .pcm
                            .iter()
                            .chain(&sample.loop_pcm)
                            .all(|&value| value == 0)
                        {
                            ""
                        } else {
                            samples.get(&key).with_context(|| {
                                format!("battle cue {id} sample {sample_id} lacks a name")
                            })?
                        };
                        let binding = write_sample(library, path, sample, &mut sample_paths)?;
                        editable_samples.insert(sample_id, binding);
                    }
                    let program = Program {
                        programs: loaded.resources().programs.clone(),
                        samples: editable_samples,
                        score: loaded.score().clone(),
                        tables: tables.clone(),
                        reverbs: None,
                    };
                    let relative = format!("speech/battle/{name}.json");
                    write_json(
                        &library.join("audio").join(&relative),
                        &Definition::Synthesized(Recipe::from_program(&program)?),
                    )?;
                    relative
                }
            } else {
                let sound = bank.sound(id)?;
                if sound.object != 1602 {
                    if let Err(error) =
                        bank.object(resonance_audio_cook::bank::ObjectKind::Macro, sound.object)
                    {
                        if selected.is_some() {
                            return Err(error)
                                .with_context(|| format!("battle cue {id} has no source"));
                        }
                        missing.push(id);
                        continue;
                    }
                    let loaded = prepare_sound(&bank, id, &sustains, &tables, reverbs)?;
                    let wave = resonance_audio::effect::bake::simple_wave(loaded)
                        .with_context(|| format!("verify empty battle cue {id}"))?
                        .with_context(|| {
                            format!("unnamed battle cue {id} is not simple silence")
                        })?;
                    let mut reader = hound::WavReader::new(Cursor::new(&wave))?;
                    ensure!(
                        reader.duration() == 320
                            && reader.spec().sample_rate == 32028
                            && reader
                                .samples::<i16>()
                                .try_fold(true, |silent, sample| sample
                                    .map(|value| silent && value == 0))?,
                        "unnamed battle cue {id} is not silent"
                    );
                }
                let relative = "effects/silence-320-frames.json".to_owned();
                write_json(
                    &library.join("audio").join(&relative),
                    &Definition::Silence(Silence {
                        frames: 320,
                        sample_rate: 32028,
                    }),
                )?;
                relative
            };
            ensure!(
                bindings.insert(u32::from(id), relative).is_none(),
                "duplicate battle cue {id}"
            );
        }
    }
    if let Some(id) = selected {
        ensure!(
            bindings.contains_key(&u32::from(id)),
            "native battle cue {id} is missing"
        );
    } else {
        ensure!(
            bindings.len() == 1933 && missing.len() == 7,
            "battle cue coverage differs: {} mapped, {} missing",
            bindings.len(),
            missing.len()
        );
        for id in missing {
            eprintln!("warning: native battle cue {id} has no source program");
        }
    }
    Mappings::new(bindings)
}

fn battle_payloads(disc: &Path) -> Result<Vec<Vec<u8>>> {
    let usual = fs::read(disc.join("files/BTL/BTLusual.dat"))?;
    let archive = fs::read(disc.join("files/BTL/BTLvbank.dat"))?;
    let count = dol::u32(&usual, 0)?;
    ensure!(count >= 14, "missing battle sample directory");
    let start = usize::try_from(dol::u32(&usual, 56)?)?;
    let end = if count == 14 {
        usual.len()
    } else {
        usize::try_from(dol::u32(&usual, 60)?)?
    };
    ensure!(
        end > start && end - start <= 65536 && (end - start).is_multiple_of(4),
        "invalid battle sample directory"
    );
    let mut offsets = usual
        .get(start..end)
        .context("battle sample directory exceeds source")?
        .chunks_exact(4)
        .map(|row| u32::from_be_bytes(row.try_into().unwrap()) as usize)
        .collect::<Vec<_>>();
    let last = offsets
        .iter()
        .position(|&offset| offset == archive.len())
        .context("battle sample directory has no EOF")?;
    ensure!(
        offsets[last + 1..]
            .iter()
            .all(|&offset| offset == 0 || offset == 0xfefe_fefe),
        "invalid battle sample directory padding"
    );
    offsets.truncate(last + 1);
    ensure!(
        offsets.len() == 62
            && offsets[0] == 0
            && offsets[61] == archive.len()
            && offsets.windows(2).all(|pair| pair[0] <= pair[1]),
        "invalid battle sample offsets"
    );
    Ok(offsets
        .windows(2)
        .map(|pair| archive[pair[0]..pair[1]].to_vec())
        .collect())
}

fn prepare_sound(
    bank: &Bank<'_>,
    id: u16,
    sustains: &resonance_audio_cook::parameters::Sustains,
    tables: &resonance_audio::music_voice::Tables,
    reverbs: [[f32; 5]; 2],
) -> Result<Arc<Loaded>> {
    let sound = bank.sound(id)?;
    let notes = instrument::resolve(
        bank,
        Page {
            object: sound.object,
            priority: sound.priority,
            max_voices: sound.max_voices,
        },
        sound.key,
        sound.volume,
        sound.pan,
    )
    .with_context(|| format!("resolve sound {id}"))?;
    let resources = decode::programs(bank, notes.iter().map(|note| note.macro_id), sustains)
        .with_context(|| format!("decode sound {id}"))?;
    Ok(Arc::new(Loaded::new(
        resources,
        score(id, notes),
        tables.clone(),
        reverbs,
    )?))
}

fn write_sample(
    library: &Path,
    path: &str,
    sample: &resonance_audio::sample::Sample,
    written: &mut BTreeSet<String>,
) -> Result<EditableSample> {
    if sample
        .pcm
        .iter()
        .chain(&sample.loop_pcm)
        .all(|&value| value == 0)
    {
        return Ok(EditableSample {
            path: String::new(),
            silence: Some(SampleSilence {
                frames: u32::try_from(sample.pcm.len() + sample.loop_pcm.len())?,
                sample_rate: sample.rate,
            }),
            key: sample.key,
            loop_start: sample.loop_start,
            loop_length: sample.loop_length,
        });
    }
    if written.insert(path.to_owned()) {
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut buffer,
            hound::WavSpec {
                channels: 1,
                sample_rate: u32::from(sample.rate),
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for &value in sample.pcm.iter().chain(&sample.loop_pcm) {
            writer.write_sample(value)?;
        }
        writer.finalize()?;
        write_once(&library.join(path), &buffer.into_inner())?;
    }
    Ok(EditableSample {
        path: path.to_owned(),
        silence: None,
        key: sample.key,
        loop_start: sample.loop_start,
        loop_length: sample.loop_length,
    })
}

fn score(id: u16, notes: Vec<resonance_audio::data::Note>) -> Score {
    Score {
        origin: ScoreOrigin::SoundEffect,
        initial_bpm_1024: 120 * 1024,
        loop_start_tick: 0,
        end_tick: 0,
        tempos: Vec::new(),
        controls: [Controls::default(); 16],
        first_events: vec![Event {
            tick: 0,
            channel: 0,
            kind: EventKind::Notes {
                source: VoiceSource::SoundEffect { id },
                voices: notes,
                length: 0,
            },
        }],
        loop_events: Vec::new(),
    }
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    write_once(path, &resonance_asset_writer::json::encode(value)?)
}

fn write_once(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().context("effect has no output directory")?)?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file.write_all(bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(
                fs::read(path)? == bytes,
                "conflicting shared effect resource {}",
                path.display()
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
