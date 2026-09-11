//! Diagnostic Rust synthesis with separate voice buses and a studio mix.
use super::{Workspace, hash_file, write_json, write_pcm16};
use anyhow::{Result, ensure};
use resonance_audio_cook::{bank::Bank, mix::Tables, render, reverb::mix_studio};
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
        (1..=32000 * 120).contains(&frames) && !events.is_empty() && events.len() <= 1024,
        "invalid sound sequence length"
    );
    ensure!(
        events.windows(2).all(|w| w[0].0 <= w[1].0)
            && events
                .iter()
                .all(|(frame, _)| *frame < frames && frame.is_multiple_of(160)),
        "sound events must be ordered at shared block boundaries"
    );
    let workspace = Workspace::open(extracted, output)?;
    let executable = fs::read(workspace.extracted.join("sys/main.dol"))?;
    let bytes = fs::read(workspace.extracted.join("files/S/se.snd"))?;
    let bank = Bank::parse(&bytes)?;
    let tables = tables(&executable)?;
    let parameters = super::music::title_reverbs(&executable)?;
    let mut studio = resonance_audio::cue::Studio::new(parameters)?;
    let mut cues = BTreeMap::new();
    for &(_, id) in events {
        if cues.contains_key(&id) {
            continue;
        }
        let voice = render::render_voice_buses(&bank, id, &tables, render::SYNTHESIS_RATE * 10)?;
        let pcm = (0..voice.buses[0].len() / 2)
            .map(|frame| {
                std::array::from_fn(|bus| {
                    [voice.buses[bus][frame * 2], voice.buses[bus][frame * 2 + 1]]
                })
            })
            .collect();
        cues.insert(id, Arc::new(resonance_audio::cue::Cue::new(pcm)?));
    }
    let path = workspace.output.join("sound-sequence.wav");
    let temporary = path.with_extension("partial.wav");
    let mut writer = hound::WavWriter::create(
        &temporary,
        hound::WavSpec {
            channels: 2,
            sample_rate: render::PLAYBACK_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    let mut events_at = 0;
    for frame in 0..frames {
        while let Some(&(at, id)) = events.get(events_at)
            && at == frame
        {
            studio.play(cues[&id].clone())?;
            events_at += 1;
        }
        for sample in studio.next_frame() {
            writer.write_sample(sample)?;
        }
    }
    writer.finalize()?;
    fs::rename(temporary, &path)?;
    write_json(
        &workspace.output.join("sound-sequence.json"),
        &json!({
            "version":1,"bank_sha256":crate::digest(&bytes),"executable_sha256":crate::digest(&executable),
            "renderer_sha256":hash_file(&std::env::current_exe()?)?,"sha256":hash_file(&path)?,
            "audio_device":false,"frames":frames,"events":events,"auxiliary_reverbs":parameters,
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
    })
}

pub fn render_sound_buses(extracted: &Path, bank: &Path, id: u16, output: &Path) -> Result<()> {
    let workspace = Workspace::open(extracted, output)?;
    let executable = workspace.extracted.join("sys/main.dol");
    let bytes = fs::read(&executable)?;
    let tables = tables(&bytes)?;
    let bank_bytes = fs::read(bank)?;
    let parsed = Bank::parse(&bank_bytes)?;
    let rendered = render::render_voice_buses(&parsed, id, &tables, render::SYNTHESIS_RATE * 10)?;
    let parameters = super::music::title_reverbs(&bytes)?;
    let studio = mix_studio(&rendered.buses, parameters)?;
    let mut buses = serde_json::Map::new();
    for (name, samples) in ["direct", "aux-a", "aux-b"]
        .into_iter()
        .zip(&rendered.buses)
        .chain(std::iter::once(("studio", &studio)))
    {
        let studio_effects_applied = name == "studio";
        let name = format!("sound-{id}-{name}.wav");
        let path = workspace.output.join(&name);
        let temporary = path.with_extension("partial.wav");
        write_pcm16(
            &temporary,
            2,
            render::PLAYBACK_RATE,
            samples.iter().copied(),
        )?;
        fs::rename(temporary, &path)?;
        buses.insert(
            name,
            json!({"sha256": hash_file(&path)?, "frames": samples.len() / 2,
                "studio_effects_applied": studio_effects_applied}),
        );
    }
    write_json(
        &workspace.output.join(format!("sound-{id}-buses.json")),
        &json!({
            "version": 1, "renderer": "resonance-audio-cook", "bank_sha256": crate::digest(&bank_bytes),
            "executable_sha256": crate::digest(&bytes), "sound_id": id, "macro_id": rendered.macro_id,
            "sample_ids": rendered.samples, "synthesis_rate": render::SYNTHESIS_RATE,
        "sample_rate": render::PLAYBACK_RATE, "channels": 2, "auxiliary_reverbs": parameters,
            "audio_device": false, "buses": buses,
        }),
    )?;
    println!("Rendered sound {id} to diagnostic PCM buses and a studio mix");
    Ok(())
}
