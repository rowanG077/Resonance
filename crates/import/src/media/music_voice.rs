//! Diagnostic instrument rendering from the original bank and executable tables.
use super::{Workspace, hash_file, write_json};
use anyhow::{Result, ensure};
use resonance_asset_writer::wav::write_pcm16;
use resonance_audio::{
    BLOCK_FRAMES, CONTROL_FRAMES, SOURCE_RATE,
    package::Loaded,
    sequence::{LiveControls, stream::Stream},
    volume::frames_from_millis,
};
use resonance_audio_cook::{
    bank::Bank, dls, instrument, modulation, music_voice, resample, reverb,
};
use serde_json::json;
use std::{fs, path::Path, sync::Arc};

pub(crate) fn tables(executable: &[u8]) -> Result<music_voice::Tables> {
    let bytes = |address, size| crate::dol::slice(executable, address, size);
    let float = |address| -> Result<f32> { Ok(f32::from_be_bytes(bytes(address, 4)?.try_into()?)) };
    let mut attenuation = [0; 194];
    for (i, value) in attenuation.iter_mut().enumerate() {
        *value = u16::from_be_bytes(bytes(0x802a_88a0 + i as u32 * 2, 2)?.try_into()?);
    }
    let mut sine = [0; 1024];
    for (i, value) in sine.iter_mut().enumerate() {
        *value = i16::from_be_bytes(bytes(0x802a_ad50 + i as u32 * 2, 2)?.try_into()?);
    }
    let mut tremolo = [0.; 5];
    for (i, value) in tremolo.iter_mut().enumerate() {
        *value = float(0x8035_e1d8 + i as u32 * 4)?;
    }
    let tables = music_voice::Tables {
        mix: super::sound_buses::tables(executable)?,
        dls: dls::Tables { attenuation },
        modulation: modulation::Tables { sine, tremolo },
        coefficients: resample::Coefficients::from_be_bytes(
            &resonance_audio_cook::interpolation::coefficients(),
        )?,
    };
    tables.validate()?;
    Ok(tables)
}

pub(crate) fn sustains(executable: &[u8]) -> Result<resonance_audio_cook::parameters::Sustains> {
    let inverse = crate::dol::slice(executable, 0x802a_8a24, 1024)?.try_into()?;
    let mut curve = [0.; 129];
    for (i, value) in curve.iter_mut().enumerate() {
        *value = f32::from_be_bytes(
            crate::dol::slice(executable, 0x802a_8e24 + i as u32 * 4, 4)?.try_into()?,
        );
    }
    resonance_audio_cook::parameters::Sustains::new(inverse, curve)
}

pub struct MusicVoiceOptions<'a> {
    pub extracted: &'a Path,
    pub output: &'a Path,
    pub macro_id: u16,
    pub key: u8,
    pub velocity: u8,
    pub hold_ms: u32,
    pub max_ms: u32,
}

pub fn render_music_voice(options: MusicVoiceOptions<'_>) -> Result<()> {
    ensure!(
        options.hold_ms < options.max_ms && options.max_ms <= 10000,
        "voice hold must be shorter than its render limit (maximum 10 seconds)"
    );
    let workspace = Workspace::open(options.extracted, options.output)?;
    let _publications = crate::publication::Session::start_if_needed(options.output)?;
    let executable_bytes = fs::read(workspace.extracted.join("sys/main.dol"))?;
    let bank_bytes = fs::read(workspace.extracted.join("files/S/inst.snd"))?;
    let coefficient_bytes = resonance_audio_cook::interpolation::coefficients();
    let tables = tables(&executable_bytes)?;
    let bank = Bank::parse(&bank_bytes)?;
    let resources = resonance_audio_cook::compile::programs(
        &bank,
        [options.macro_id],
        &sustains(&executable_bytes)?,
    )?;
    let note = instrument::Voice {
        macro_id: options.macro_id,
        key: options.key,
        velocity: options.velocity,
        pan: 64,
        priority: 64,
        max_voices: 255,
    };
    let parameters = super::music::title_reverbs(&executable_bytes)?;
    let loaded = Arc::new(Loaded::new(
        resources,
        super::sound_score(note.macro_id, Some(vec![note])),
        tables,
        parameters,
    )?);
    let buses = render_instrument(
        loaded,
        frames_from_millis(u64::from(options.hold_ms))?,
        frames_from_millis(u64::from(options.max_ms))?,
    )?;
    let frame = buses[0].len() / 2;
    let studio = reverb::mix_studio(&buses, parameters)?;
    let mut assets = serde_json::Map::new();
    for (name, samples) in ["direct", "aux-a", "aux-b"]
        .into_iter()
        .zip(buses.iter().map(|bus| {
            bus.iter()
                .map(|sample| (*sample).clamp(i16::MIN as i32, i16::MAX as i32) as i16)
                .collect::<Vec<_>>()
        }))
        .chain(std::iter::once(("studio", studio)))
    {
        let name = format!("macro-{}-key-{}-{name}.wav", options.macro_id, options.key);
        let path = workspace.output.join(&name);
        let temporary = crate::temporary_path(&path);
        write_pcm16(&temporary, 2, SOURCE_RATE, samples.iter().copied())?;
        crate::publication::install(&temporary, &path, &hash_file(&temporary)?)?;
        assets.insert(
            name,
            json!({"sha256":hash_file(&path)?,"frames":samples.len()/2}),
        );
    }
    write_json(
        &workspace.output.join(format!(
            "macro-{}-key-{}.json",
            options.macro_id, options.key
        )),
        &json!({"version":1,"renderer":"resonance-audio-cook","audio_device":false,
            "renderer_sha256":hash_file(&std::env::current_exe()?)?,"oracle_accepted":false,
            "bank_sha256":crate::digest(&bank_bytes),"executable_sha256":crate::digest(&executable_bytes),
            "coefficients_sha256":crate::digest(&coefficient_bytes),
            "macro_id":options.macro_id,"key":options.key,"velocity":options.velocity,
            "hold_ms":options.hold_ms,"macro_frames":frame,"synthesis_rate":SOURCE_RATE,
            "sample_rate":SOURCE_RATE,"channels":2,"assets":assets,
        }),
    )?;
    println!(
        "Rendered music macro {} through note-off and release ({frame} frames), without playback",
        options.macro_id
    );
    Ok(())
}

/// Use live note-off scheduling and completion, including the final queued block.
fn render_instrument(
    loaded: Arc<Loaded>,
    hold_frames: u64,
    max_frames: u64,
) -> Result<[Vec<i32>; 3]> {
    ensure!(
        hold_frames < max_frames,
        "voice hold must be shorter than its render limit"
    );
    let mut stream = Stream::new(loaded, false)?;
    let mut buses: [Vec<i32>; 3] = Default::default();
    // Commands run on the native control grid; output is submitted in whole blocks.
    let note_off = hold_frames.div_ceil(CONTROL_FRAMES as u64) * CONTROL_FRAMES as u64;
    let limit = max_frames.div_ceil(BLOCK_FRAMES as u64) * BLOCK_FRAMES as u64;
    let mut start = 0;
    loop {
        let controls = std::array::from_fn(|quantum| LiveControls {
            release: start + quantum as u64 * CONTROL_FRAMES as u64 == note_off,
            ..Default::default()
        });
        let Some(block) = stream.block_envelope(controls)? else {
            return Ok(buses);
        };
        ensure!(start < limit, "music voice exceeded its render limit");
        for output in block {
            for (bus, samples) in buses.iter_mut().zip(output) {
                bus.extend(samples);
            }
        }
        start += BLOCK_FRAMES as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_audio_cook::{bank::ObjectKind, mix};

    #[test]
    fn instrument_diagnostic_keeps_final_pcm_and_uses_native_note_off() -> Result<()> {
        use resonance_audio::{
            data::{Command, Interpolation, Note, Resources},
            sample::Sample,
        };
        use std::collections::BTreeMap;
        for looping in [false, true] {
            let mut pcm = vec![0; 160];
            if looping {
                pcm.fill(8192);
            } else {
                pcm[159] = 8192;
            }
            let mut commands = vec![
                Command::Interpolation {
                    mode: Interpolation::Direct,
                    coefficients: 0,
                },
                Command::VolumeControl { value: 16383 },
                // This operation requires the same shared scheduler as live playback.
                Command::RandomWait {
                    upper_ms: 1,
                    key_off: false,
                    sample_end: false,
                },
                Command::StartSample { sample: 1 },
            ];
            if looping {
                commands.extend([
                    Command::Wait {
                        milliseconds: None,
                        from_start: false,
                        key_off: true,
                        sample_end: false,
                    },
                    Command::StopSample,
                ]);
            }
            commands.push(Command::End);
            let loaded = Arc::new(Loaded::new(
                Resources {
                    programs: BTreeMap::from([(1, commands)]),
                    samples: BTreeMap::from([(
                        1,
                        Arc::new(Sample {
                            key: 60,
                            rate: 32000,
                            loop_start: 0,
                            loop_length: if looping { 160 } else { 0 },
                            loop_pcm: if looping { pcm.clone() } else { vec![] },
                            pcm,
                        }),
                    )]),
                },
                super::super::sound_score(
                    1,
                    Some(vec![Note {
                        macro_id: 1,
                        key: 60,
                        velocity: 127,
                        pan: 64,
                        priority: 1,
                        max_voices: 1,
                    }]),
                ),
                super::super::tests::audio_tables(),
                [[0., 0., 1., 0., 0.]; 2],
            )?);
            // A request just after frame32 takes effect at the next control boundary64.
            let buses = render_instrument(loaded.clone(), 33, 3200)?;
            let dry = buses[0]
                .chunks_exact(2)
                .map(|frame| frame[0])
                .collect::<Vec<_>>();
            let end = if looping { 64 } else { 160 };
            assert!(dry[end - 1] > 0, "last source sample was discarded");
            assert!(dry[end] > 0, "native release tail was discarded");
            assert!(dry[end + 1] < dry[end]);
            assert!(
                dry[end + resonance_audio::RELEASE_FRAMES as usize..]
                    .iter()
                    .all(|&sample| sample == 0)
            );
            if looping {
                let immediate = render_instrument(loaded.clone(), 0, 160)?;
                assert!(immediate.iter().flatten().all(|&sample| sample == 0));
                assert!(immediate[0].len() <= 160 * 2);
                assert!(dry[..end].iter().all(|&sample| sample == dry[0]));
                assert!(
                    render_instrument(loaded, 33, 64)
                        .unwrap_err()
                        .to_string()
                        .contains("render limit")
                );
            } else {
                assert!(dry[..end - 1].iter().all(|&sample| sample == 0));
            }
        }
        Ok(())
    }

    fn extracted() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1")
    }

    #[test]
    #[ignore = "requires the local GQSEAF executable; expected gains are pinned Dolphin observations"]
    fn music_gain_curves_match_independent_dolphin_voice_state() {
        let bytes = fs::read(extracted().join("sys/main.dol")).unwrap();
        // Full channel reset copies these MSB/LSB bytes before setup overrides.
        let defaults = crate::dol::slice(&bytes, 0x801e_04d8, 0x86).unwrap();
        let controls = music_voice::Controls::default();
        assert_eq!(
            controls.paired,
            std::array::from_fn(|index| {
                (u16::from(defaults[index]) << 7) | u16::from(defaults[index + 32])
            })
        );
        assert_eq!(
            controls.pitch_bend,
            (u16::from(defaults[0x80]) << 7) | u16::from(defaults[0x81])
        );
        assert_eq!(
            controls.surround,
            (u16::from(defaults[0x84]) << 7) | u16::from(defaults[0x85])
        );
        let tables = super::super::sound_buses::tables(&bytes).unwrap();
        // title-entry GQSEAF.s01: active sample 384 at pan 50, then sample 155 at pan 40.
        // These are oracle expectations, never cooker inputs.
        for (volume, pan, expected) in [
            (6815744, 50, [[5000, 3562], [5001, 3563], [0, 0]]),
            (6566400, 40, [[4919, 2631], [4920, 2632], [0, 0]]),
        ] {
            assert_eq!(
                tables.gains_for(mix::Parameters {
                    volume,
                    controller: 8890,
                    pan: pan << 16,
                    pre: [0; 2],
                    post: [127 << 7, 0],
                    scale: 1.0,
                    group_volume: 1.0,
                    aux_a: 128,
                    alternate: true,
                    interaural_delay: false,
                }),
                expected
            );
        }
    }

    #[test]
    #[ignore = "requires local extracted instruments and RESONANCE_DSP_COEFFICIENTS"]
    fn title_macros_release_before_and_after_their_delayed_modulation() {
        let executable = fs::read(extracted().join("sys/main.dol")).unwrap();
        let bytes = fs::read(extracted().join("files/S/inst.snd")).unwrap();
        let bank = Bank::parse(&bytes).unwrap();
        let parameters = resonance_audio_cook::parameters::dls(
            bank.object(ObjectKind::Table, 168).unwrap(),
            &sustains(&executable).unwrap(),
        )
        .unwrap()
        .resolve(104, 78)
        .unwrap();
        assert_eq!(
            (
                parameters.attack_frames,
                parameters.sustain,
                parameters.release_frames
            ),
            (
                0,
                193,
                resonance_audio::volume::frames_from_millis(110).unwrap()
            )
        );
        assert!(
            (resonance_audio::volume::frames_from_millis(2745).unwrap()
                ..=resonance_audio::volume::frames_from_millis(2746).unwrap())
                .contains(&parameters.decay_frames)
        );
        for (macro_id, key) in [
            (379, 55),
            (380, 67),
            (394, 76),
            (416, 67),
            (503, 36),
            (590, 69),
            (591, 60),
            (744, 64),
            (746, 81),
            (785, 72),
            (791, 69),
            (792, 48),
        ] {
            for hold_ms in [100, 1500] {
                let resources = resonance_audio_cook::compile::programs(
                    &bank,
                    [macro_id],
                    &sustains(&executable).unwrap(),
                )
                .unwrap();
                let note = instrument::Voice {
                    macro_id,
                    key,
                    velocity: 104,
                    pan: 64,
                    priority: 64,
                    max_voices: 255,
                };
                let loaded = Arc::new(
                    Loaded::new(
                        resources,
                        super::super::sound_score(macro_id, Some(vec![note])),
                        super::tables(&executable).unwrap(),
                        [[0., 0., 1., 0., 0.]; 2],
                    )
                    .unwrap(),
                );
                let buses = render_instrument(
                    loaded,
                    frames_from_millis(hold_ms).unwrap(),
                    u64::from(SOURCE_RATE) * 5,
                )
                .unwrap_or_else(|error| panic!("macro {macro_id} at {hold_ms}ms: {error:#}"));
                let nonzero = buses[0]
                    .chunks_exact(2)
                    .filter(|frame| *frame != [0, 0])
                    .count();
                assert!(
                    nonzero > 100,
                    "macro {macro_id} did not render instrument PCM"
                );
                if macro_id == 744 && hold_ms == 100 {
                    // Table 157 starts a 250-ms release at 100 ms. The second
                    // KeyOff after its absolute 333-ms wait must not extend it.
                    let release_end = frames_from_millis(350)
                        .unwrap()
                        .div_ceil(CONTROL_FRAMES as u64)
                        * CONTROL_FRAMES as u64;
                    let complete = (release_end + u64::from(resonance_audio::RELEASE_FRAMES))
                        .div_ceil(BLOCK_FRAMES as u64)
                        * BLOCK_FRAMES as u64;
                    assert!(
                        buses[0].len() as u64 / 2 <= complete,
                        "repeated release extended the native completion boundary"
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "requires original sound banks and RESONANCE_DSP_COEFFICIENTS; silent in-memory synthesis"]
    fn original_layered_random_cues_share_music_clock_and_finish_after_release() -> Result<()> {
        use anyhow::Context;
        use resonance_audio::{
            data::{Command, EventKind, Note, VoiceSource},
            package::{Loaded, Package, SampleAsset, SampleCache},
            sequence::{LiveControls, shared::Synthesizer, stream::Stream},
        };
        use resonance_audio_cook::{bank::Page, decode};
        use sha2::{Digest, Sha256};
        use std::{collections::BTreeMap, io::Cursor, sync::Arc};

        // Exercise the physical decoder, scene binder and runtime loader without
        // writing a package, opening a device or depending on a previous cook.
        fn load(
            bank: &Bank<'_>,
            source: VoiceSource,
            notes: Vec<Note>,
            tables: music_voice::Tables,
            sustains: &resonance_audio_cook::parameters::Sustains,
        ) -> Result<Arc<Loaded>> {
            let resources =
                decode::programs(bank, notes.iter().map(|note| note.macro_id), sustains)?;
            let mut files = BTreeMap::new();
            let mut samples = BTreeMap::new();
            for (id, sample) in resources.samples {
                let mut bytes = Cursor::new(Vec::new());
                let mut wave = hound::WavWriter::new(
                    &mut bytes,
                    hound::WavSpec {
                        channels: 1,
                        sample_rate: u32::from(sample.rate),
                        bits_per_sample: 16,
                        sample_format: hound::SampleFormat::Int,
                    },
                )?;
                for &pcm in sample.pcm.iter().chain(&sample.loop_pcm) {
                    wave.write_sample(pcm)?;
                }
                wave.finalize()?;
                let bytes = bytes.into_inner();
                let path = format!("sample-{id}.wav");
                samples.insert(
                    id,
                    SampleAsset {
                        path: path.clone(),
                        sha256: crate::digest(&bytes),
                        key: sample.key,
                        rate: sample.rate,
                        first_frames: sample.pcm.len() as u32,
                        loop_start: sample.loop_start,
                        loop_length: sample.loop_length,
                    },
                );
                files.insert(path, bytes);
            }
            let mut score = super::super::sound_score(0, Some(notes));
            score.origin = source.origin();
            let EventKind::Notes {
                source: allocation, ..
            } = &mut score.first_events[0].kind
            else {
                unreachable!()
            };
            *allocation = source;
            let package = Package {
                version: resonance_audio::package::VERSION,
                programs: resources.programs,
                samples,
                score,
                tables,
                reverbs: [[0., 0., 1., 0., 0.]; 2],
            };
            files.insert("cue.json".into(), serde_json::to_vec(&package)?);
            Ok(Arc::new(Package::load_with(
                "cue.json",
                &mut |path, limit| {
                    let bytes = files.get(path).context("missing in-memory cue asset")?;
                    ensure!(bytes.len() <= limit, "in-memory cue exceeds loader budget");
                    Ok(bytes.clone())
                },
                &mut SampleCache::default(),
            )?))
        }

        let executable = fs::read(extracted().join("sys/main.dol"))?;
        let common_bytes = fs::read(extracted().join("files/S/se.snd"))?;
        let instrument_bytes = fs::read(extracted().join("files/S/inst.snd"))?;
        let event_bytes = fs::read(extracted().join("files/S/se_ev06.snd"))?;
        let instruments = Bank::parse(&instrument_bytes)?;
        let mut common = Bank::parse(&common_bytes)?;
        common.inherit_objects(&instruments);
        common.inherit_samples(&instruments);
        let mut event = Bank::parse(&event_bytes)?;
        event.inherit_objects(&common);
        event.inherit_objects(&instruments);
        event.inherit_samples(&instruments);
        event.inherit_samples(&common);
        let mut loaded = Vec::new();
        for (bank, id, expected) in [
            (&common, 416, [485, 487, 488, 486]),
            (&event, 490, [66, 65, 64, 69]),
        ] {
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
            )?;
            assert_eq!(
                notes.iter().map(|note| note.macro_id).collect::<Vec<_>>(),
                expected
            );
            loaded.push(load(
                bank,
                VoiceSource::SoundEffect { id },
                notes,
                tables(&executable)?,
                &sustains(&executable)?,
            )?);
        }
        // These are the actual layered routes that previously failed admission:
        // two random roots and callback loops, plus a timer/key-off/sample wait.
        for (id, upper_ms) in [(487, 3000), (488, 2000)] {
            let program = &loaded[0].resources().programs[&id];
            assert!(
                matches!(program[6], Command::RandomWait { upper_ms: bound, key_off: true, sample_end: false } if bound == upper_ms)
            );
            assert!(matches!(program[7], Command::RandomNote { .. }));
            assert!(matches!(
                program[9],
                Command::Wait {
                    milliseconds: None,
                    key_off: true,
                    sample_end: true,
                    ..
                }
            ));
            assert!(matches!(
                program[11],
                Command::Loop {
                    instruction: 4,
                    key_off: true,
                    ..
                }
            ));
        }
        assert!(matches!(
            loaded[1].resources().programs[&69][7],
            Command::Wait {
                milliseconds: Some(1000),
                key_off: true,
                sample_end: true,
                ..
            }
        ));
        // An ordinary title instrument shares allocation and completion ordering.
        loaded.push(load(
            &instruments,
            VoiceSource::Sequence {
                group: 0,
                program: 0,
                drums: false,
            },
            vec![Note {
                macro_id: 379,
                key: 55,
                velocity: 104,
                pan: 64,
                priority: 64,
                max_voices: 255,
            }],
            tables(&executable)?,
            &sustains(&executable)?,
        )?);

        for hold_ms in [100, 6000] {
            let mut expected = None;
            for _ in 0..2 {
                let synth = Synthesizer::default();
                let streams = loaded
                    .iter()
                    .map(|loaded| Stream::in_synthesizer(loaded.clone(), false, &synth))
                    .collect::<Result<Vec<_>>>()?;
                let mut hashes: [Sha256; 3] = Default::default();
                let mut nonzero = [0; 3];
                let mut ends = [None; 3];
                let release_frame = u32::try_from(frames_from_millis(hold_ms)?)?;
                for frame in 0..release_frame + 5 * SOURCE_RATE {
                    if frame == release_frame {
                        for stream in &streams {
                            stream.set_shared_controls(
                                [LiveControls {
                                    release: true,
                                    ..Default::default()
                                };
                                    resonance_audio::CONTROLS_PER_BLOCK],
                            )?;
                        }
                    }
                    synth.advance()?;
                    for (i, stream) in streams.iter().enumerate() {
                        if let Some(pcm) = stream.shared_frame() {
                            assert!(ends[i].is_none(), "cue {i} resumed after finishing");
                            nonzero[i] +=
                                usize::from(pcm.iter().flatten().any(|&sample| sample != 0));
                            for sample in pcm.into_iter().flatten() {
                                hashes[i].update(sample.to_le_bytes());
                            }
                        } else {
                            ends[i].get_or_insert(frame);
                        }
                    }
                    if frame >= release_frame && ends.iter().all(Option::is_some) {
                        break;
                    }
                }
                assert!(
                    ends.iter().all(Option::is_some),
                    "cue release did not complete: {ends:?}"
                );
                assert!(ends[..2].iter().all(|end| end.unwrap() > release_frame));
                assert!(
                    nonzero.iter().all(|&frames| frames > 100),
                    "missing cue PCM: {nonzero:?}"
                );
                let random = synth.random_state();
                if hold_ms == 100 {
                    // Key-off skips both RandomWait draws, but each layer still
                    // executes RandomNote before its key-off-aware loop exits.
                    assert_eq!(random.1, 3);
                } else {
                    assert!(
                        random.1 > 3,
                        "authored loops did not advance the shared RNG"
                    );
                }
                for _ in 0..160 {
                    synth.advance()?;
                }
                assert_eq!(synth.random_state(), random, "finished cues kept drawing");
                let actual = (
                    hashes.map(|hash| format!("{:x}", hash.finalize())),
                    ends,
                    nonzero,
                    random,
                );
                if let Some(expected) = &expected {
                    assert_eq!(
                        &actual, expected,
                        "shared playback changed between identical runs"
                    );
                }
                expected = Some(actual);
            }
        }
        Ok(())
    }
}
