//! Diagnostic Rust synthesis with separate voice buses and a studio mix.
use super::{Workspace, hash_file, write_json};
use anyhow::{Result, ensure};
use resonance_asset_writer::wav::write_pcm16;
use resonance_audio::{
    BLOCK_FRAMES, SOURCE_RATE, music_voice,
    package::Loaded,
    reverb::{Studio, mix_studio},
    sequence::{BusFrame, shared::Synthesizer, stream::Stream},
};
use resonance_audio_cook::{bank::Bank, mix::Tables};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

/// Diagnostic cue scheduling; output uses shared effects, never recorded PCM.
pub fn render_sound_sequence(
    extracted: &Path,
    output: &Path,
    frames: u32,
    events: &[(u32, u16)],
) -> Result<()> {
    ensure!(
        (1..=SOURCE_RATE * 120).contains(&frames) && !events.is_empty() && events.len() <= 1024,
        "invalid sound sequence length"
    );
    ensure!(
        events.windows(2).all(|w| w[0].0 <= w[1].0)
            && events
                .iter()
                .all(|(frame, _)| *frame < frames && frame.is_multiple_of(BLOCK_FRAMES as u32)),
        "sound events must be ordered at shared block boundaries"
    );
    let workspace = Workspace::open(extracted, output)?;
    let _publications = crate::publication::Session::start_if_needed(output)?;
    let executable = fs::read(workspace.extracted.join("sys/main.dol"))?;
    let bytes = fs::read(workspace.extracted.join("files/S/se.snd"))?;
    let pools = super::library::Pools::read(&workspace.extracted)?;
    let bank = pools.bank(&bytes)?;
    let parameters = super::music::title_reverbs(&executable)?;
    let mut studio = Studio::new(parameters)?;
    let synthesizer = Synthesizer::default();
    let mut cues = BTreeMap::new();
    for &(_, id) in events {
        if let std::collections::btree_map::Entry::Vacant(entry) = cues.entry(id) {
            entry.insert(load_sound(
                &bank,
                id,
                &pools.sustains,
                super::synthesis_tables(&executable)?,
                parameters,
            )?);
        }
    }
    let path = workspace.output.join("sound-sequence.wav");
    let temporary = crate::temporary_path(&path);
    let mut writer = hound::WavWriter::create(
        &temporary,
        hound::WavSpec {
            channels: 2,
            sample_rate: SOURCE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    let mut events_at = 0;
    let mut streams = Vec::new();
    for frame in 0..frames {
        while let Some(&(at, id)) = events.get(events_at)
            && at == frame
        {
            streams.push(Stream::in_synthesizer(
                cues[&id].clone(),
                false,
                &synthesizer,
            )?);
            events_at += 1;
        }
        for sample in studio.process(next_frame(&synthesizer, &mut streams)?) {
            writer.write_sample(pcm16(sample))?;
        }
    }
    writer.finalize()?;
    crate::publication::install(&temporary, &path, &hash_file(&temporary)?)?;
    write_json(
        &workspace.output.join("sound-sequence.json"),
        &json!({
            "version":1,"bank_sha256":crate::digest(&bytes),"executable_sha256":crate::digest(&executable),
            "renderer_sha256":hash_file(&std::env::current_exe()?)?,"sha256":hash_file(&path)?,
            "audio_device":false,"frames":frames,"events":events,"auxiliary_reverbs":parameters,
            "sample_rate":SOURCE_RATE,"coefficients_sha256":crate::digest(&resonance_audio_cook::interpolation::coefficients()),
        }),
    )?;
    println!("Rendered {frames} cue frames with shared effects, without playback");
    Ok(())
}

pub(super) fn tables(bytes: &[u8]) -> Result<Tables> {
    let float = |address| -> Result<f32> {
        Ok(f32::from_be_bytes(
            crate::dol::slice(bytes, address, 4)?.try_into()?,
        ))
    };
    let mut volume = [0.0; 129];
    let mut alternate_volume = [0.0; 129];
    let mut pan = [0.0; 4];
    for (i, value) in volume.iter_mut().enumerate() {
        *value = float(0x802a_aa98 + i as u32 * 4)?;
    }
    for (i, value) in alternate_volume.iter_mut().enumerate() {
        *value = float(0x802a_8e24 + i as u32 * 4)?;
    }
    for (i, value) in pan.iter_mut().enumerate() {
        *value = float(0x802a_aa98 + (129 + i) as u32 * 4)?;
    }
    Ok(Tables {
        volume,
        alternate_volume,
        pan,
        volume_16_scale: float(0x8035_e1d4)?,
        controller_14_scale: float(0x8035_e1ec)?,
        pan_16_scale: float(0x8035_e2a8)?,
        spatial: Some(resonance_audio::mix::Spatial {
            pan_scale: float(0x8035_e2bc)?,
            left_delay: crate::dol::slice(bytes, 0x801e_0618, 128 * 2)?
                .chunks_exact(2)
                .map(|bytes| Ok(u8::try_from(u16::from_be_bytes(bytes.try_into()?))?))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .expect("fixed spatial table length"),
        }),
    })
}

pub fn render_sound_buses(extracted: &Path, bank: &Path, id: u16, output: &Path) -> Result<()> {
    let workspace = Workspace::open(extracted, output)?;
    let _publications = crate::publication::Session::start_if_needed(output)?;
    let executable = workspace.extracted.join("sys/main.dol");
    let bytes = fs::read(&executable)?;
    let bank_bytes = fs::read(bank)?;
    let pools = super::library::Pools::read(&workspace.extracted)?;
    let parsed = pools.bank(&bank_bytes)?;
    let parameters = super::music::title_reverbs(&bytes)?;
    let sound = load_sound(
        &parsed,
        id,
        &pools.sustains,
        super::synthesis_tables(&bytes)?,
        parameters,
    )?;
    let macro_ids: Vec<_> = sound.resources().programs.keys().copied().collect();
    let sample_ids: Vec<_> = sound.resources().samples.keys().copied().collect();
    let synthesizer = Synthesizer::default();
    let mut streams = vec![Stream::in_synthesizer(sound, false, &synthesizer)?];
    let mut rendered = [Vec::new(), Vec::new(), Vec::new()];
    loop {
        let frame = next_frame(&synthesizer, &mut streams)?;
        if streams.is_empty() {
            break;
        }
        ensure!(
            rendered[0].len() / 2 < (SOURCE_RATE * 10) as usize,
            "sound exceeded its ten-second render limit"
        );
        for (bus, frame) in rendered.iter_mut().zip(frame) {
            bus.extend(frame);
        }
    }
    let studio = mix_studio(&rendered, parameters)?;
    let mut buses = serde_json::Map::new();
    for (name, samples) in ["direct", "aux-a", "aux-b"]
        .into_iter()
        .zip(
            rendered
                .iter()
                .map(|bus| bus.iter().copied().map(pcm16).collect::<Vec<_>>()),
        )
        .chain(std::iter::once(("studio", studio)))
    {
        let studio_effects_applied = name == "studio";
        let name = format!("sound-{id}-{name}.wav");
        let path = workspace.output.join(&name);
        let temporary = crate::temporary_path(&path);
        write_pcm16(&temporary, 2, SOURCE_RATE, samples.iter().copied())?;
        crate::publication::install(&temporary, &path, &hash_file(&temporary)?)?;
        buses.insert(
            name,
            json!({"sha256": hash_file(&path)?, "frames": samples.len() / 2,
                "studio_effects_applied": studio_effects_applied}),
        );
    }
    write_json(
        &workspace.output.join(format!("sound-{id}-buses.json")),
        &json!({
            "version": 2, "renderer": "resonance-audio", "bank_sha256": crate::digest(&bank_bytes),
            "executable_sha256": crate::digest(&bytes), "sound_id": id, "macro_ids": macro_ids,
            "sample_ids": sample_ids, "synthesis_rate": SOURCE_RATE,
            "sample_rate": SOURCE_RATE, "channels": 2, "auxiliary_reverbs": parameters,
            "coefficients_sha256": crate::digest(&resonance_audio_cook::interpolation::coefficients()),
            "audio_device": false, "buses": buses,
        }),
    )?;
    println!("Rendered sound {id} to diagnostic PCM buses and a studio mix");
    Ok(())
}

fn load_sound(
    bank: &Bank<'_>,
    id: u16,
    sustains: &resonance_audio_cook::parameters::Sustains,
    tables: music_voice::Tables,
    reverbs: [[f32; 5]; 2],
) -> Result<Arc<Loaded>> {
    let (resources, score) = super::sound_library::sound(bank, id, sustains)?;
    Ok(Arc::new(Loaded::new(resources, score, tables, reverbs)?))
}

fn next_frame(synthesizer: &Synthesizer, streams: &mut Vec<Stream>) -> Result<BusFrame> {
    synthesizer.advance()?;
    let mut output = [[0i32; 2]; 3];
    let mut index = 0;
    while index < streams.len() {
        if let Some(frame) = streams[index].shared_frame() {
            for (bus, frame) in output.iter_mut().zip(frame) {
                for (sample, value) in bus.iter_mut().zip(frame) {
                    *sample = sample.saturating_add(value);
                }
            }
            index += 1;
        } else {
            streams.swap_remove(index);
        }
    }
    Ok(output)
}

fn pcm16(sample: i32) -> i16 {
    sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_audio::{
        data::{Command, Interpolation, Note, Resources},
        sample::Sample,
    };

    #[test]
    fn scheduled_native_sound_preserves_all_buses_and_finishes() -> Result<()> {
        let mut tables = super::super::tests::audio_tables();
        tables.mix.volume = std::array::from_fn(|i| (i as f32 / 127.).min(1.));
        tables.mix.alternate_volume = tables.mix.volume;
        tables.mix.volume_16_scale = 1. / (127. * 65536.);
        tables.mix.controller_14_scale = 1. / 16383.;
        tables.mix.pan_16_scale = 1. / (63. * 65536.);
        let sound = Arc::new(Loaded::new(
            Resources {
                programs: BTreeMap::from([(
                    1,
                    vec![
                        Command::Interpolation {
                            mode: Interpolation::Direct,
                            coefficients: 0,
                        },
                        Command::VolumeControl { value: 16383 },
                        Command::Auxiliary { bus: 0, value: 127 },
                        Command::Auxiliary { bus: 1, value: 127 },
                        Command::StartSample { sample: 1 },
                        Command::Wait {
                            milliseconds: None,
                            from_start: false,
                            key_off: false,
                            sample_end: true,
                        },
                        Command::End,
                    ],
                )]),
                samples: BTreeMap::from([(
                    1,
                    Arc::new(Sample {
                        key: 60,
                        rate: 32000,
                        pcm: vec![8192; 128],
                        loop_pcm: vec![],
                        loop_start: 0,
                        loop_length: 0,
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
            tables,
            [[0., 0., 1., 0., 0.]; 2],
        )?);
        let synthesizer = Synthesizer::default();
        let mut streams = Vec::new();
        let mut audible = [false; 3];
        for frame in 0..1024 {
            if frame == 160 {
                streams.push(Stream::in_synthesizer(sound.clone(), false, &synthesizer)?);
            }
            let output = next_frame(&synthesizer, &mut streams)?;
            if frame < 160 {
                assert_eq!(output, [[0; 2]; 3]);
            }
            for (audible, bus) in audible.iter_mut().zip(output) {
                *audible |= bus.iter().any(|&sample| sample != 0);
            }
        }
        assert_eq!(audible, [true; 3]);
        assert!(
            streams.is_empty(),
            "finite sound must retire after its output completes"
        );
        Ok(())
    }
}
