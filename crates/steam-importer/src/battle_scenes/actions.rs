//! Participant tracks are storage-normalized, then compiled by the GC reader.
use super::*;
use crate::battle_voices;
use resonance_content::{battle_normal::Event, battle_scene::SceneAction, battle_voice::Sound};

fn commands(bytes: &[u8], start: usize, voices: &mut Vec<Option<Sound>>) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut at = start;
    loop {
        let tick = pe::half(bytes, at)?;
        result.extend(tick.to_be_bytes());
        if tick == u16::MAX {
            return Ok(result);
        }
        let opcode = pe::half(bytes, at + 2)?;
        result.extend(opcode.to_be_bytes());
        if opcode == 27 {
            ensure!(
                pe::word(bytes, at + 4)? == 2,
                "unsupported participant voice priority"
            );
            let voice = battle_voices::sound(pe::word(bytes, at + 8)?)?;
            let index = voices.len();
            voices.push(voice);
            result.extend((0x8000 | u16::try_from(index)?).to_be_bytes());
            result.extend(2u16.to_be_bytes());
            at += 12;
        } else {
            let width = match opcode {
                0 | 1 | 3 | 9 => 6,
                5 | 13 | 28 => 8,
                _ => anyhow::bail!("participant command {opcode} is not converted"),
            };
            for field in (4..width).step_by(2) {
                result.extend(pe::half(bytes, at + field)?.to_be_bytes());
            }
            at += width;
        }
    }
}

pub(super) fn read(bytes: &[u8]) -> Result<BTreeMap<u8, SceneAction>> {
    let offsets = [0, 4, 8, 12]
        .map(|at| pe::word(bytes, at).map(|value| value as usize))
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        offsets[0] == 16 + 4 * 28
            && offsets.windows(2).all(|pair| pair[0] <= pair[1])
            && offsets[3] <= bytes.len(),
        "invalid Steam participant table"
    );
    let mut result = vec![0; offsets[0]];
    let mut rows = Vec::new();
    for row in bytes[16..offsets[0]].chunks_exact(28) {
        ensure!(
            pe::word(row, 12)? == 0,
            "participant hit-rule base is not prepared"
        );
        rows.push(canonical(
            row,
            28,
            &[
                (0, 2),
                (2, 2),
                (4, 2),
                (6, 2),
                (8, 2),
                (10, 2),
                (12, 4),
                (16, 4),
                (20, 4),
                (24, 4),
            ],
        )?);
    }
    let rules = &bytes[offsets[0]..offsets[1]];
    ensure!(
        rules.len().is_multiple_of(28),
        "misaligned participant hit rules"
    );
    let hits = canonical(rules, 28, &[(0, 2), (8, 4), (14, 2), (16, 2)])?;
    let start = result.len() as u32;
    result[0..4].copy_from_slice(&start.to_be_bytes());
    result.extend(hits);
    let start = result.len() as u32;
    result[4..8].copy_from_slice(&start.to_be_bytes());
    result.extend(canonical(
        &bytes[offsets[1]..offsets[2]],
        32,
        &[(0, 2), (8, 4), (12, 4), (22, 2), (28, 4)],
    )?);
    let start = result.len() as u32;
    result[8..12].copy_from_slice(&start.to_be_bytes());
    result.extend(canonical(
        &bytes[offsets[2]..offsets[3]],
        12,
        &[(0, 2), (8, 4)],
    )?);
    let start = result.len() as u32;
    result[12..16].copy_from_slice(&start.to_be_bytes());
    let mut packed = Vec::new();
    let mut roots = BTreeMap::new();
    let mut voices = Vec::new();
    for (row, source) in rows.iter_mut().zip(bytes[16..offsets[0]].chunks_exact(28)) {
        if pe::half(source, 0)? == 0 {
            continue;
        }
        let source = usize::try_from(pe::word(source, 24)?)? * 2;
        let root = if let Some(root) = roots.get(&source) {
            *root
        } else {
            let root = u32::try_from(packed.len() / 2)?;
            packed.extend(commands(&bytes[offsets[3]..], source, &mut voices)?);
            roots.insert(source, root);
            root
        };
        row[24..28].copy_from_slice(&root.to_be_bytes());
    }
    for (slot, row) in rows.into_iter().enumerate() {
        result[16 + slot * 28..44 + slot * 28].copy_from_slice(&row);
    }
    result.extend(packed);
    // Each participant variant is independently cold in the shared format.
    // An unsupported pose on one variant must not discard the other tracks.
    let mut actions = BTreeMap::new();
    for (index, row) in result[16..offsets[0]].chunks_exact(28).enumerate() {
        if row[..2] == [0, 0] {
            continue;
        }
        let mut selected = result.clone();
        for other in 0..4 {
            if other != index {
                selected[16 + other * 28..18 + other * 28].fill(0);
            }
        }
        match resonance_importer::decode_scene_actions(&selected) {
            Ok(mut prepared) => {
                actions.insert(
                    index as u8,
                    prepared
                        .remove(&(index as u8))
                        .context("participant variant disappeared")?,
                );
            }
            Err(error) => {
                actions.insert(
                    index as u8,
                    SceneAction::Unsupported {
                        reason: format!("{error:#}"),
                    },
                );
            }
        }
    }
    for action in actions.values_mut() {
        let SceneAction::Prepared(attack) = action else {
            continue;
        };
        for (_, event) in &mut attack.events {
            match event {
                Event::Voice(sound) => {
                    let token = match *sound {
                        Some(Sound::Stream(token)) => usize::from(token),
                        _ => anyhow::bail!("invalid participant voice token"),
                    };
                    *sound = *voices.get(token).context("missing participant voice")?;
                }
                Event::Sound(sound) => *sound = audio_effects::id(*sound)?,
                Event::Contact(contact) => {
                    contact.hit.sound = contact.hit.sound.map(audio_effects::id).transpose()?
                }
                _ => {}
            }
        }
    }
    Ok(actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expanded_voice_command_keeps_bank_identity_after_shared_track_compile() -> Result<()> {
        let mut bytes = vec![0; 258];
        for (at, value) in [(0, 128u32), (4, 156), (8, 220), (12, 244)] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[16..18].copy_from_slice(&30u16.to_le_bytes());
        bytes[26..28].copy_from_slice(&u16::MAX.to_le_bytes());
        let hit = &mut bytes[128..156];
        hit[0..2].copy_from_slice(&0x20u16.to_le_bytes());
        hit[2] = 10;
        hit[13] = 1;
        hit[14..16].copy_from_slice(&100u16.to_le_bytes());
        let contact = &mut bytes[156..188];
        contact[2] = 2;
        contact[3] = 1;
        contact[4] = 255;
        contact[20] = 1;
        for at in [8, 12] {
            contact[at..at + 4].copy_from_slice(&10f32.to_le_bytes());
        }
        bytes[188..190].copy_from_slice(&u16::MAX.to_le_bytes());
        bytes[226] = 8;
        bytes[227] = 255;
        bytes[228..232].copy_from_slice(&1f32.to_le_bytes());
        bytes[232..234].copy_from_slice(&(-2i16).to_le_bytes());
        bytes[246..248].copy_from_slice(&27u16.to_le_bytes());
        bytes[248..252].copy_from_slice(&2u32.to_le_bytes());
        bytes[252..256].copy_from_slice(&0x0a0001u32.to_le_bytes());
        bytes[256..258].copy_from_slice(&u16::MAX.to_le_bytes());
        // Stored combinations also change the current clip's speed without
        // rebinding a pose, using the same control as ordinary martial tracks.
        let mut rate = [0; 12];
        rate[..2].copy_from_slice(&14u16.to_le_bytes());
        rate[2] = 254;
        rate[8..12].copy_from_slice(&0.5f32.to_le_bytes());
        bytes.splice(232..232, rate);
        bytes[12..16].copy_from_slice(&256u32.to_le_bytes());
        let actions = read(&bytes)?;
        let SceneAction::Prepared(action) = &actions[&0] else {
            anyhow::bail!("participant action was not prepared")
        };
        ensure!(
            action
                .events
                .iter()
                .any(|(at, event)| *at == 14 && matches!(event, Event::AnimationRate(0.5))),
            "combination rate control was not compiled"
        );
        ensure!(
            action
                .events
                .iter()
                .any(|(_, event)| matches!(event, Event::Voice(Some(Sound::Stream(0x8901))))),
            "expanded voice lost its native bank"
        );
        ensure!(
            action
                .events
                .iter()
                .any(|(_, event)| matches!(event,Event::Contact(contact) if contact.radius==10.)),
            "contact geometry was not compiled"
        );
        Ok(())
    }
}
