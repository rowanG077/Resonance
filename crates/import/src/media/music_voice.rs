//! Diagnostic instrument rendering from the original bank and executable tables.
use super::{Workspace, hash_file, write_json};
use anyhow::{Result, ensure};
use resonance_audio_cook::{
    bank::Bank,
    dls, instrument, modulation, music_voice,
    render::{PLAYBACK_RATE, SYNTHESIS_RATE},
    resample, reverb,
};
use serde_json::json;
use std::{fs, path::Path};

pub(super) fn tables(executable: &[u8], coefficients: &[u8]) -> Result<music_voice::Tables> {
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
        let temporary = path.with_extension("partial.wav");
        let mut writer = hound::WavWriter::create(
            &temporary,
            hound::WavSpec {
                channels: 2,
                sample_rate: PLAYBACK_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for sample in samples {
            writer.write_sample(*sample)?;
        }
        writer.finalize()?;
        fs::rename(temporary, &path)?;
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
    let temporary = path.with_extension("partial.wav");
    let mut writer = hound::WavWriter::create(
        &temporary,
        hound::WavSpec {
            channels: 2,
            sample_rate: PLAYBACK_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in preview.pcm {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    fs::rename(temporary, &path)?;
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
                    pan,
                    post: [127 << 7, 0],
                    scale: 1.0,
                    group_volume: 1.0,
                    aux_a: 128,
                    alternate: true
                }),
                expected
            );
        }
    }

    #[test]
    #[ignore = "requires local extracted instruments and RESONANCE_DSP_COEFFICIENTS"]
    fn continuous_music_worker_stops_when_its_bounded_consumer_disconnects() {
        let coefficient_path = std::env::var_os("RESONANCE_DSP_COEFFICIENTS")
            .expect("set RESONANCE_DSP_COEFFICIENTS to the pinned decoder coefficient resource");
        let executable = fs::read(extracted().join("sys/main.dol")).unwrap();
        let tables = tables(&executable, &fs::read(coefficient_path).unwrap()).unwrap();
        let bank_bytes = fs::read(extracted().join("files/S/inst.snd")).unwrap();
        let bank = Bank::parse(&bank_bytes).unwrap();
        let song = resonance_audio_cook::song::Song::parse(
            &fs::read(extracted().join("files/S/bgm_etc000.song")).unwrap(),
        )
        .unwrap();
        let setup = bank.music_setup(0, 1).unwrap();
        let (resources, score) =
            resonance_audio_cook::compile::music(&bank, &song, &setup).unwrap();
        let reverbs = super::super::music::title_reverbs(&executable).unwrap();
        let (send, receive) = std::sync::mpsc::sync_channel(2);
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                let mut disconnected = false;
                resonance_audio_cook::sequence::render_stream(
                    &resources,
                    &score,
                    &tables,
                    reverbs,
                    |_| 1.0,
                    |block| {
                        assert!(!disconnected, "renderer ignored sink cancellation");
                        disconnected = send.send(block.to_vec()).is_err();
                        !disconnected
                    },
                )
                .unwrap();
                assert!(disconnected);
            });
            for _ in 0..4 {
                assert_eq!(receive.recv().unwrap().len(), 320);
            }
            drop(receive);
            worker.join().unwrap();
        });
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
}
