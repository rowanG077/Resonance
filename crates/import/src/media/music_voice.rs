//! Diagnostic instrument rendering from the original bank and executable tables.
use super::{Workspace, hash_file, write_json};
use anyhow::{Result, ensure};
use resonance_asset_writer::wav::write_pcm16;
use resonance_audio_cook::{
    bank::Bank,
    dls, instrument, modulation, music_voice,
    render::{PLAYBACK_RATE, SYNTHESIS_RATE},
    resample, reverb,
};
use serde_json::json;
use std::{fs, path::Path};

pub(crate) fn tables(executable: &[u8], coefficients: &[u8]) -> Result<music_voice::Tables> {
    let bytes = |address, size| crate::dol::slice(executable, address, size);
    let float = |address| -> Result<f32> { Ok(f32::from_be_bytes(bytes(address, 4)?.try_into()?)) };
    let mut attenuation = [0; 194];
    for (i, value) in attenuation.iter_mut().enumerate() {
        *value = u16::from_be_bytes(bytes(0x802a_88a0 + i as u32 * 2, 2)?.try_into()?);
    }
    let inverse = bytes(0x802a_8a24, 1024)?.try_into()?;
    let mut sustain = [0.; 128];
    for (i, value) in sustain.iter_mut().enumerate() {
        *value = float(0x802a_8e24 + i as u32 * 4)?;
    }
    ensure!(
        float(0x802a_8e24 + 128 * 4)? == 1.0,
        "unexpected full-scale DLS sustain endpoint"
    );
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
        pitch: super::pitched_sample::pitch_tables(executable)?,
        dls: dls::Tables {
            attenuation,
            inverse,
            sustain,
        },
        modulation: modulation::Tables { sine, tremolo },
        coefficients: resample::Coefficients::from_be_bytes(coefficients)?,
    };
    tables.validate()?;
    Ok(tables)
}

pub struct MusicVoiceOptions<'a> {
    pub extracted: &'a Path,
    pub coefficients: &'a Path,
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
    let coefficient_bytes = fs::read(options.coefficients)?;
    let tables = tables(&executable_bytes, &coefficient_bytes)?;
    let bank = Bank::parse(&bank_bytes)?;
    let resources = resonance_audio_cook::compile::programs(&bank, [options.macro_id])?;
    let mut voice = music_voice::Voice::new(
        &resources,
        &tables,
        instrument::Voice {
            macro_id: options.macro_id,
            key: options.key,
            velocity: options.velocity,
            pan: 64,
            priority: 64,
            max_voices: 255,
        },
    )?;
    let mut buses: [Vec<i16>; 3] = Default::default();
    let mut frame = 0;
    while !voice.is_done() {
        let mut block = [[[0; 2]; 3]; 160];
        let mut count = 0;
        for _ in 0..160 {
            ensure!(
                frame < options.max_ms * 32,
                "music voice exceeded its render limit"
            );
            if frame == options.hold_ms * 32 {
                voice.key_off()?;
            }
            voice.prepare_frame(music_voice::Controls::default())?;
            if voice.is_done() {
                break;
            }
            count += 1;
            frame += 1;
        }
        voice.mix_block(&mut block)?;
        for output in &block[..count] {
            for (bus, samples) in buses.iter_mut().zip(output) {
                bus.extend(samples.map(|s| s as i16));
            }
        }
    }
    let parameters = super::music::title_reverbs(&executable_bytes)?;
    let studio = reverb::mix_studio(&buses, parameters)?;
    let mut assets = serde_json::Map::new();
    for (name, samples) in ["direct", "aux-a", "aux-b"]
        .into_iter()
        .zip(&buses)
        .chain(std::iter::once(("studio", &studio)))
    {
        let name = format!("macro-{}-key-{}-{name}.wav", options.macro_id, options.key);
        let path = workspace.output.join(&name);
        let temporary = crate::temporary_path(&path);
        write_pcm16(&temporary, 2, PLAYBACK_RATE, samples.iter().copied())?;
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
            "hold_ms":options.hold_ms,"macro_frames":frame,"synthesis_rate":SYNTHESIS_RATE,
            "sample_rate":PLAYBACK_RATE,"channels":2,"assets":assets,
        }),
    )?;
    println!(
        "Rendered music macro {} through note-off and release ({frame} frames), without playback",
        options.macro_id
    );
    Ok(())
}

pub fn render_title_audio_preview(
    extracted: &Path,
    coefficients: &Path,
    output: &Path,
    frames: u32,
    master_fade_lead_ms: Option<u16>,
) -> Result<()> {
    let workspace = Workspace::open(extracted, output)?;
    let _publications = crate::publication::Session::start_if_needed(output)?;
    let executable_bytes = fs::read(workspace.extracted.join("sys/main.dol"))?;
    let bank_bytes = fs::read(workspace.extracted.join("files/S/inst.snd"))?;
    let song_bytes = fs::read(workspace.extracted.join("files/S/bgm_etc000.song"))?;
    let coefficient_bytes = fs::read(coefficients)?;
    let tables = tables(&executable_bytes, &coefficient_bytes)?;
    let bank = Bank::parse(&bank_bytes)?;
    let song = resonance_audio_cook::song::Song::parse(&song_bytes)?;
    let parameters = super::music::title_reverbs(&executable_bytes)?;
    let setup = bank.music_setup(0, 1)?;
    let (resources, score) = resonance_audio_cook::compile::music(&bank, &song, &setup)?;
    let preview = if let Some(lead) = master_fade_lead_ms {
        ensure!(
            lead.is_multiple_of(5),
            "master fade lead must align to a five-ms block"
        );
        let mut master = resonance_audio_cook::volume::Fade::new(0.0, 1.0, 2000)?;
        let mut sequence = resonance_audio_cook::volume::Fade::new(0.0, 1.0, 100)?;
        for _ in 0..lead / 5 {
            master.advance_block();
        }
        resonance_audio_cook::sequence::render_preview_with_volume(
            &resources,
            &score,
            &tables,
            parameters,
            frames,
            |frame| {
                let volume = sequence.value() * master.value();
                if frame.is_multiple_of(160) {
                    master.advance_block();
                    sequence.advance_block();
                }
                volume
            },
        )?
    } else {
        resonance_audio_cook::sequence::render_preview(
            &resources, &score, &tables, parameters, frames,
        )?
    };
    let path = workspace.output.join("title-preview.wav");
    let temporary = crate::temporary_path(&path);
    write_pcm16(&temporary, 2, PLAYBACK_RATE, preview.pcm)?;
    crate::publication::install(&temporary, &path, &hash_file(&temporary)?)?;
    write_json(
        &path.with_extension("json"),
        &json!({
        "version":1,"renderer":"resonance-audio-cook","audio_device":false,
        "renderer_sha256":hash_file(&std::env::current_exe()?)?,
            "bank_sha256":crate::digest(&bank_bytes),"song_sha256":crate::digest(&song_bytes),
            "executable_sha256":crate::digest(&executable_bytes),"coefficients_sha256":crate::digest(&coefficient_bytes),
            "notes_started":preview.notes,"maximum_voices":preview.maximum_voices,"final_tick":preview.final_tick,
            "voice_pool":{"free":preview.free_voices,"lfo_counters":preview.lfo_counters.as_slice()},
            "voice_lifetimes":preview.voice_lifetimes.iter().map(|v| json!({
                "slot":v.slot,"start_frame":v.start_frame,"end_frame":v.end_frame,
                "macro":v.macro_id,"key":v.key,
            })).collect::<Vec<_>>(),
            "auxiliary_reverbs":parameters,"synthesis_rate":SYNTHESIS_RATE,
            "loop_restart_implemented":true,"loop_start_frames":preview.loop_starts,"oracle_accepted":false,
            "startup_fades":master_fade_lead_ms.map(|lead| json!({
                "master_ms":2000,"sequence_ms":100,"master_lead_ms":lead,
            })),
            "asset":{"path":"title-preview.wav","sha256":hash_file(&path)?,"frames":frames,
                "channels":2,"sample_rate":PLAYBACK_RATE},
        }),
    )?;
    println!(
        "Rendered {} score notes to {frames} diagnostic frames without playback",
        preview.notes
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_audio_cook::{bank::ObjectKind, mix};

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
        let coefficient_path = std::env::var_os("RESONANCE_DSP_COEFFICIENTS")
            .expect("set RESONANCE_DSP_COEFFICIENTS to the pinned decoder coefficient resource");
        let executable = fs::read(extracted().join("sys/main.dol")).unwrap();
        let coefficient_bytes = fs::read(coefficient_path).unwrap();
        let tables = tables(&executable, &coefficient_bytes).unwrap();
        let bytes = fs::read(extracted().join("files/S/inst.snd")).unwrap();
        let bank = Bank::parse(&bytes).unwrap();
        let parameters =
            resonance_audio_cook::parameters::dls(bank.object(ObjectKind::Table, 168).unwrap())
                .unwrap()
                .resolve(&tables.dls, 104, 78)
                .unwrap();
        assert_eq!(
            (
                parameters.attack_ms,
                parameters.decay_ms,
                parameters.sustain,
                parameters.release_ms
            ),
            (0, 2745, 193, 110)
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
                let resources = resonance_audio_cook::compile::programs(&bank, [macro_id]).unwrap();
                let mut voice = music_voice::Voice::new(
                    &resources,
                    &tables,
                    instrument::Voice {
                        macro_id,
                        key,
                        velocity: 104,
                        pan: 64,
                        priority: 64,
                        max_voices: 255,
                    },
                )
                .unwrap();
                let mut nonzero = 0;
                let mut finished_at = None;
                for start in (0..32000 * 5).step_by(160) {
                    let mut block = [[[0; 2]; 3]; 160];
                    for frame in start..start + 160 {
                        if frame == hold_ms * 32 {
                            voice.key_off().unwrap();
                        }
                        voice
                            .prepare_frame(music_voice::Controls::default())
                            .unwrap();
                        if voice.is_done() {
                            finished_at = Some(frame);
                            break;
                        }
                    }
                    voice.mix_block(&mut block).unwrap();
                    nonzero += block.iter().filter(|pcm| pcm[0] != [0; 2]).count();
                    if voice.is_done() {
                        break;
                    }
                }
                assert!(
                    voice.is_done(),
                    "macro {macro_id} did not finish after note-off at {hold_ms} ms"
                );
                assert!(
                    nonzero > 100,
                    "macro {macro_id} did not render instrument PCM"
                );
                if macro_id == 744 && hold_ms == 100 {
                    // Table 157 starts a 250-ms release at 100 ms. The second
                    // KeyOff after its absolute 333-ms wait must not extend it.
                    assert_eq!(finished_at, Some(350 * 32));
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
        ) -> Result<Arc<Loaded>> {
            let resources = decode::programs(bank, notes.iter().map(|note| note.macro_id))?;
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
            let package = super::super::sound_library::Resources {
                version: 1,
                programs: resources.programs,
                samples,
                score: Some(score),
            }
            .package(tables, [[0., 0., 1., 0., 0.]; 2])?;
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

        let coefficients = fs::read(std::env::var_os("RESONANCE_DSP_COEFFICIENTS").context(
            "set RESONANCE_DSP_COEFFICIENTS to the pinned decoder coefficient resource",
        )?)?;
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
                tables(&executable, &coefficients)?,
            )?);
        }
        // These are the actual layered routes that previously failed admission:
        // two random roots and callback loops, plus a timer/key-off/sample wait.
        for (id, upper_ms) in [(487, 3000), (488, 2000)] {
            let program = &loaded[0].resources.programs[&id];
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
            loaded[1].resources.programs[&69][7],
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
            tables(&executable, &coefficients)?,
        )?);

        for hold_ms in [100, 6000] {
            let mut expected = None;
            for _ in 0..2 {
                let synth = Synthesizer::default();
                let streams = loaded
                    .iter()
                    .map(|loaded| Stream::in_synthesizer(loaded.clone(), false, &synth))
                    .collect::<Result<Vec<_>>>()?;
                assert!(streams.iter().all(Stream::is_shared));
                let mut hashes: [Sha256; 3] = Default::default();
                let mut nonzero = [0; 3];
                let mut ends = [None; 3];
                let release_frame = hold_ms * (SYNTHESIS_RATE / 1000);
                for frame in 0..release_frame + 5 * SYNTHESIS_RATE {
                    if frame == release_frame {
                        if hold_ms == 100 {
                            // Only cue490 has drawn yet; cue416 is in its initial waits.
                            assert_eq!(synth.random_state().1, 1);
                        }
                        for stream in &streams {
                            stream.set_shared_controls(
                                [LiveControls {
                                    release: true,
                                    ..Default::default()
                                }; 5],
                            )?;
                        }
                    }
                    synth.advance()?;
                    for (i, stream) in streams.iter().enumerate() {
                        if let Some(pcm) = stream.shared_frame()? {
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
