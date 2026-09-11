//! Inspect the original score through the Rust readers, without MIDI conversion.
use super::{Workspace, write_json};
use anyhow::Result;
use resonance_audio_cook::{
    bank::{Bank, ObjectKind},
    instrument,
    song::{EventKind, Song},
};
use serde_json::json;
use std::{collections::BTreeSet, fs, path::Path};

pub fn inspect_title_audio(extracted: &Path, output: &Path) -> Result<()> {
    let workspace = Workspace::open(extracted, output)?;
    let bytes = fs::read(workspace.extracted.join("files/S/bgm_etc000.song"))?;
    let bank_bytes = fs::read(workspace.extracted.join("files/S/inst.snd"))?;
    let song = Song::parse(&bytes)?;
    let bank = Bank::parse(&bank_bytes)?;
    let setup = bank.music_setup(0, 1)?;
    let mut programs = setup.channels.map(|c| c.program);
    let mut objects = BTreeSet::new();
    let mut macros = BTreeSet::new();
    let mut notes = 0;
    let mut events = Vec::new();
    for event in song.events() {
        let channel = usize::from(event.channel);
        let kind = match event.kind {
            EventKind::Pattern { program, volume } => {
                if let Some(program) = program
                    && setup.page(event.channel, program).is_some()
                {
                    programs[channel] = program;
                }
                json!({"kind":"pattern", "program":program, "volume":volume})
            }
            EventKind::Command { command, value } => {
                if command == 0 && setup.page(event.channel, value).is_some() {
                    programs[channel] = value;
                }
                json!({"kind":"command", "command":command, "value":value})
            }
            EventKind::Note {
                key,
                velocity,
                length,
            } => {
                notes += 1;
                let page = setup.page(event.channel, programs[channel]);
                if let Some(page) = page {
                    objects.insert(page.object);
                }
                let voices = page
                    .map(|page| instrument::resolve(&bank, page, key, velocity, 64))
                    .transpose()?
                    .unwrap_or_default();
                let voices: Vec<_> = voices
                    .into_iter()
                    .map(|v| {
                        macros.insert(v.macro_id);
                        json!({"macro":v.macro_id,"key":v.key,"velocity":v.velocity,"pan":v.pan,
                        "priority":v.priority,"max_voices":v.max_voices})
                    })
                    .collect();
                json!({"kind":"note", "key":key, "velocity":velocity, "length_ticks":length,
                    "program":programs[channel], "object":page.map(|p| p.object), "voices":voices})
            }
            EventKind::PitchBend(value) => json!({"kind":"pitch_bend", "value":value}),
            EventKind::Modulation(value) => json!({"kind":"modulation", "value":value}),
        };
        events.push(
            json!({"tick":event.tick,"track":event.track,"channel":event.channel,"event":kind}),
        );
    }
    let channels: Vec<_> = setup.channels.iter().map(|c| json!({
        "program":c.program,"volume":c.volume,"pan":c.pan,"reverb":c.reverb,"chorus":c.chorus,
    })).collect();
    let tracks: Vec<_> = song.tracks.iter().map(|t| json!({
        "id":t.id,"channel":t.channel,"regions":t.regions,"end_tick":t.end_tick,"loop_region":t.loop_region,
    })).collect();
    let tempos: Vec<_> = song
        .tempos
        .iter()
        .map(|t| json!({"tick":t.tick,"bpm_1024":t.bpm_1024}))
        .collect();
    let mut macro_programs = Vec::new();
    let mut sample_ids = BTreeSet::new();
    for id in &macros {
        let bytes = bank.object(ObjectKind::Macro, *id)?;
        anyhow::ensure!(bytes.len().is_multiple_of(8), "unaligned music macro");
        let commands: Vec<_> = bytes
            .chunks_exact(8)
            .map(|c| {
                let a = u32::from_be_bytes(c[..4].try_into().unwrap());
                let b = u32::from_be_bytes(c[4..].try_into().unwrap());
                if a as u8 == 0x10 {
                    sample_ids.insert((a >> 8) as u16);
                }
                json!({"opcode":a as u8,"command":format!("{a:08x}"),"argument":format!("{b:08x}")})
            })
            .collect();
        macro_programs.push(json!({"id":id,"commands":commands}));
    }
    let mut samples = Vec::new();
    for id in sample_ids {
        let sample = bank.sample(id)?;
        let pcm: Vec<_> = sample.pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
        let loop_pcm: Vec<_> = sample
            .loop_pcm
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        samples.push(json!({"id":id,"key":sample.key,"sample_rate":sample.rate,
            "frames":sample.pcm.len(),"loop_start":sample.loop_start,"loop_frames":sample.loop_length,
            "pcm_s16le_sha256":crate::digest(&pcm),"loop_pcm_s16le_sha256":crate::digest(&loop_pcm)}));
    }
    write_json(
        &workspace.output.join("title-score.json"),
        &json!({
            "version":1,"reader":"resonance-audio-cook","audio_device":false,
            "song_sha256":crate::digest(&bytes),"bank_sha256":crate::digest(&bank_bytes),
            "group":0,"setup":1,"initial_bpm_1024":song.initial_bpm_1024,
            "loop_ticks":song.playback_interval()?,"tempos":tempos,"channels":channels,
            "tracks":tracks,"notes":notes,"objects":objects,"macros":macro_programs,"samples":samples,"events":events,
        }),
    )?;
    println!(
        "Read {} tracks, {notes} notes and {} instrument objects",
        song.tracks.len(),
        objects.len()
    );
    Ok(())
}
