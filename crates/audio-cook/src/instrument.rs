//! Expand keymaps and instrument layers into playable voices.
//! This resolves resources and initial controls; voice allocation is separate.
use crate::{
    bank::{Bank, ObjectKind, Page},
    read,
};
use anyhow::{Result, bail, ensure};
pub use resonance_audio::data::Note as Voice;

pub fn resolve(bank: &Bank<'_>, page: Page, key: u8, velocity: u8, pan: u8) -> Result<Vec<Voice>> {
    ensure!(
        key < 128 && velocity < 128 && pan <= 128,
        "invalid instrument input"
    );
    let mut voices = Vec::new();
    expand(
        bank,
        page.object,
        Voice {
            macro_id: 0,
            key,
            velocity,
            pan,
            priority: page.priority,
            max_voices: page.max_voices,
        },
        &mut Vec::new(),
        &mut voices,
    )?;
    Ok(voices)
}

fn panning(original: u8, offset: u8) -> u8 {
    if offset & 128 != 0 {
        128
    } else {
        (i16::from(original) + i16::from(offset) - 64).clamp(0, 127) as u8
    }
}

fn expand(
    bank: &Bank<'_>,
    id: u16,
    voice: Voice,
    path: &mut Vec<u16>,
    output: &mut Vec<Voice>,
) -> Result<()> {
    if id == u16::MAX {
        return Ok(());
    }
    ensure!(
        path.len() < 32 && !path.contains(&id),
        "cyclic or excessive instrument nesting"
    );
    ensure!(output.len() < 256, "instrument expands to too many voices");
    path.push(id);
    match id & 0xc000 {
        0 => {
            bank.object(ObjectKind::Macro, id)?;
            output.push(Voice {
                macro_id: id,
                ..voice
            });
        }
        0x4000 => {
            let bytes = bank.object(ObjectKind::Keymap, id)?;
            let entry = read::slice(bytes, usize::from(voice.key) * 8, 8)?;
            let child = read::u16(entry, 0)?;
            // The original rejects another keymap in this slot.
            ensure!(child & 0xc000 != 0x4000, "keymap references another keymap");
            let voice = Voice {
                key: (i16::from(voice.key) + i16::from(entry[2] as i8)).clamp(0, 127) as u8,
                pan: panning(voice.pan, entry[3]),
                priority: i16::from(voice.priority)
                    .wrapping_add(read::u16(entry, 4)? as i16)
                    .clamp(0, 255) as u8,
                ..voice
            };
            expand(bank, child, voice, path, output)?;
        }
        0x8000 => {
            let bytes = bank.object(ObjectKind::Layer, id)?;
            let count = usize::from(read::u16(bytes, 2)?);
            let entries = read::slice(bytes, 4, count * 12)?;
            let mut priority = voice.priority;
            for entry in entries.chunks_exact(12) {
                let child = read::u16(entry, 0)?;
                if child == u16::MAX || !(entry[2]..=entry[3]).contains(&voice.key) {
                    continue;
                }
                // Original priority adjustments accumulate across matching layers.
                priority = i16::from(priority)
                    .wrapping_add(read::u16(entry, 6)? as i16)
                    .clamp(0, 255) as u8;
                expand(
                    bank,
                    child,
                    Voice {
                        key: (i16::from(voice.key) + i16::from(entry[4] as i8)).clamp(0, 127) as u8,
                        velocity: (u16::from(voice.velocity) * u16::from(entry[5]) / 127) as u8,
                        pan: panning(voice.pan, entry[8]),
                        priority,
                        ..voice
                    },
                    path,
                    output,
                )?;
            }
        }
        _ => bail!("unsupported instrument object class {id:#06x}"),
    }
    path.pop();
    Ok(())
}
