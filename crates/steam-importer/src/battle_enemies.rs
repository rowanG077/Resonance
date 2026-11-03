//! Normalize enemy source tables for the shared GC action compiler.
use crate::{
    audio_effects, battle_normal::canonical, battle_profiles, battle_projectiles, battle_voices, pe,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{battle_action::EnemyActionVoices, battle_voice::Sound};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn section(bytes: &[u8], field: usize) -> Result<&[u8]> {
    let start = usize::from(pe::half(bytes, field)?);
    ensure!(start >= 0x1f8, "enemy section overlaps header");
    let mut end = bytes.len();
    for at in (4..20).step_by(2) {
        let other = usize::from(pe::half(bytes, at)?);
        if other > start {
            end = end.min(other);
        }
    }
    for at in (0x18..0x1f8).step_by(4) {
        let other = pe::word(bytes, at)? as usize;
        if other > start {
            end = end.min(other);
        }
    }
    bytes
        .get(start..end)
        .context("enemy section exceeds package")
}

fn records(bytes: &[u8], width: usize, fields: &[(usize, usize)]) -> Vec<u8> {
    let mut result = bytes.to_vec();
    for row in result.chunks_mut(width) {
        for &(at, size) in fields {
            if let Some(field) = row.get_mut(at..at + size) {
                field.reverse();
            }
        }
    }
    result
}

fn commands(bytes: &[u8], choices: &[u8]) -> Result<Vec<u8>> {
    let mut result = bytes.to_vec();
    let mut visited = BTreeSet::new();
    for row in choices.chunks_exact(72) {
        for field in [26, 54] {
            if field == 54 && pe::word(row, 8)? & 0x100 == 0 {
                continue;
            }
            let start = usize::from(pe::half(row, field)?) * 2;
            if field == 54 && start == 0 {
                continue;
            }
            let mut cursor = start;
            while visited.insert(cursor) {
                let time = pe::half(bytes, cursor)? as i16;
                if time == -1 {
                    result[cursor..cursor + 2].reverse();
                    break;
                }
                let opcode = pe::half(bytes, cursor + 2)? as i16;
                let (width, fields): (_, &[(usize, usize)]) = match opcode {
                    -5 | -3 | 31 => (4, &[(0, 2), (2, 2)]),
                    0..=3 | 9 | 20 | 21 | 41 | 48 => (6, &[(0, 2), (2, 2), (4, 2)]),
                    7 => (6, &[(0, 2), (2, 2)]),
                    5 | 12 | 13 | 17..=19 | 23 | 26 | 28 | 38 | 39 | 43 | 44 => {
                        (8, &[(0, 2), (2, 2), (4, 2), (6, 2)])
                    }
                    14 => (16, &[(0, 2), (2, 2), (4, 2), (6, 2), (8, 8)]),
                    25 => (16, &[(0, 2), (2, 2), (4, 2), (6, 2), (8, 4), (12, 4)]),
                    27 => (12, &[(0, 2), (2, 2), (4, 4), (8, 4)]),
                    29 | 45..=47 => (12, &[(0, 2), (2, 2), (4, 2), (6, 2), (8, 2), (10, 2)]),
                    _ => anyhow::bail!("enemy command {opcode} is not converted"),
                };
                let row = bytes
                    .get(cursor..cursor + width)
                    .context("truncated enemy command")?;
                result[cursor..cursor + width].copy_from_slice(&canonical(row, width, fields)?);
                if opcode == 25 {
                    // Repeat is the halfword at +14; +12 is unused, not the
                    // low word of a little-endian u32 repeat selector.
                    result[cursor + 12..cursor + 16]
                        .copy_from_slice(&u32::from(pe::half(row, 14)?).to_be_bytes());
                }
                cursor += width;
            }
        }
    }
    Ok(result)
}

fn modifiers(bytes: &[u8], contacts: &[u8]) -> Result<Vec<u8>> {
    let mut result = bytes.to_vec();
    let mut visited = BTreeSet::new();
    for contact in contacts.chunks_exact(32) {
        if pe::half(contact, 0)? == u16::MAX || contact[2] != 254 {
            continue;
        }
        let mut cursor = usize::from(pe::half(contact, 22)?);
        if cursor == 0 {
            continue;
        }
        while visited.insert(cursor) {
            let operation = pe::half(bytes, cursor)?;
            if operation == 0 {
                result[cursor..cursor + 2].reverse();
                break;
            }
            let width = match operation {
                1 => 16,
                2 | 4 | 5 => 8,
                _ => anyhow::bail!("enemy projectile modifier {operation} is not converted"),
            };
            let row = bytes
                .get(cursor..cursor + width)
                .context("truncated projectile modifier")?;
            let fields: Vec<_> = [(0, 2), (2, 2)]
                .into_iter()
                .chain((4..width).step_by(4).map(|at| (at, 4)))
                .collect();
            result[cursor..cursor + width].copy_from_slice(&canonical(row, width, &fields)?);
            cursor += width;
        }
    }
    Ok(result)
}

fn package(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid Steam enemy package");
    let choices = section(bytes, 10)?;
    ensure!(
        !choices.is_empty() && choices.len().is_multiple_of(72),
        "invalid enemy choices"
    );
    let mut result = vec![0; 0x1e8];
    result[..4].copy_from_slice(b"em8\0");
    result[0x14..0x18].copy_from_slice(&pe::word(bytes, 0x14)?.to_be_bytes());
    for field in (4..20).step_by(2) {
        let source = section(bytes, field)?;
        let data = match field {
            4 => battle_profiles::canonical_profile(source)?,
            6 => source.to_vec(),
            8 => canonical(source, 28, &[(0, 2), (8, 4), (14, 2), (16, 2)])?,
            10 => choices
                .chunks_exact(72)
                .map(|row| -> Result<Vec<u8>> {
                    let mut compact = row[..60].to_vec();
                    compact.extend([0; 4]); // Voice IDs are bound after the common decoder.
                    compact.extend_from_slice(&row[68..72]);
                    let fields: Vec<_> = [4, 8, 40, 44]
                        .into_iter()
                        .map(|at| (at, 4))
                        .chain(
                            [
                                12, 14, 16, 18, 20, 22, 24, 26, 28, 30, 36, 38, 54, 56, 58, 64, 66,
                            ]
                            .into_iter()
                            .map(|at| (at, 2)),
                        )
                        .collect();
                    canonical(&compact, 68, &fields)
                })
                .collect::<Result<Vec<_>>>()?
                .concat(),
            12 => {
                ensure!(source.len() >= 28, "truncated enemy policy");
                [source[..12].to_vec(), source[14..].to_vec()].concat()
            }
            14 => commands(source, choices)?,
            16 => records(source, 32, &[(0, 2), (8, 4), (12, 4), (22, 2), (28, 4)]),
            18 => records(source, 12, &[(0, 2), (8, 4)]),
            _ => unreachable!(),
        };
        let offset =
            u16::try_from(result.len()).context("canonical enemy section exceeds header range")?;
        result[field..field + 2].copy_from_slice(&offset.to_be_bytes());
        result.extend(data);
    }
    let mut members = BTreeMap::new();
    for field in (0x18..0x1e8).step_by(4) {
        let start = pe::word(bytes, field)? as usize;
        if start == 0 {
            continue;
        }
        let offset = if let Some(&offset) = members.get(&start) {
            offset
        } else {
            let member =
                crate::preview_model::member(bytes, field)?.context("missing enemy member")?;
            let data = if field == 0x1c8 {
                let end = member.len() / 400 * 400;
                ensure!(
                    end != 0
                        && (end == member.len()
                            || (start + end).next_multiple_of(16) == start + member.len()),
                    "invalid enemy projectile padding"
                );
                member[..end]
                    .chunks_exact(400)
                    .map(battle_projectiles::canonical_record)
                    .collect::<Result<Vec<_>>>()?
                    .concat()
            } else if field == 0x1dc {
                modifiers(member, section(bytes, 16)?)?
            } else {
                // Filename references and opaque artwork members are not read
                // by ordinary action recipes. Their presence remains explicit.
                member.to_vec()
            };
            let offset = u32::try_from(result.len())?;
            result.extend(data);
            members.insert(start, offset);
            offset
        };
        result[field..field + 4].copy_from_slice(&offset.to_be_bytes());
    }
    Ok(result)
}

fn common_actions(bytes: &[u8], choices: &[u8]) -> Result<Vec<u8>> {
    let count = pe::word(bytes, 0)? as usize;
    ensure!(
        count <= 65536 && 4 + count * 4 <= bytes.len(),
        "invalid common action directory"
    );
    let mut result = bytes.to_vec();
    for field in (0..4 + count * 4).step_by(4) {
        result[field..field + 4].reverse();
    }
    let mut visited = BTreeSet::new();
    for row in choices.chunks_exact(72) {
        let slot = usize::from(pe::half(row, 68)?);
        if !(1..200).contains(&slot) {
            continue;
        }
        ensure!(
            slot < count,
            "enemy physical action {slot} exceeds directory"
        );
        let start = pe::word(bytes, 4 + slot * 4)? as usize;
        if start == 0 || start == bytes.len() || !visited.insert(start) {
            continue;
        }
        let rules = start + pe::word(bytes, start)? as usize;
        let contacts = start + pe::word(bytes, start + 4)? as usize;
        ensure!(
            start + 16 <= rules
                && rules <= contacts
                && contacts <= bytes.len()
                && (rules - start - 16).is_multiple_of(28),
            "invalid common action {slot} sections"
        );
        for at in (start..start + 16).step_by(4) {
            result[at..at + 4].reverse();
        }
        for at in (start + 16..rules).step_by(28) {
            for offset in [0, 2] {
                result[at + offset..at + offset + 2].reverse();
            }
        }
        result[rules..contacts].copy_from_slice(&canonical(
            &bytes[rules..contacts],
            28,
            &[(0, 2), (8, 4), (14, 2), (16, 2)],
        )?);
    }
    Ok(result)
}

fn voice(bytes: &[u8]) -> Result<(Option<Sound>, bool, usize)> {
    let word = |at| -> Result<u32> {
        Ok(u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .context("truncated enemy voice")?
                .try_into()?,
        ))
    };
    let priority = word(4)?;
    ensure!(matches!(priority, 2 | 3), "invalid enemy voice priority");
    Ok((battle_voices::sound(word(8)?)?, priority == 3, 12))
}

/// Compile an enemy's Steam tables into shared editable action definitions.
/// Model-contact recipes require their effect artwork to have been cooked first.
/// Native recipes without a shared decoder return an error; this does not yet
/// publish complete enemy owners in the full cook.
pub fn actions(
    bytes: &[u8],
    common: &[u8],
    library: &Path,
    id: u16,
) -> Result<resonance_importer::EnemyActionTables> {
    let common = common_actions(common, section(bytes, 10)?)?;
    let canonical = package(bytes)?;
    let mut result = resonance_importer::decode_enemy_actions(
        &canonical,
        library,
        &crate::monsters::symbol(id),
        Some(&common),
        id,
        voice,
    )?;
    for (choice, row) in result
        .actions
        .rows
        .iter_mut()
        .zip(section(bytes, 10)?.chunks_exact(72))
    {
        choice.voices = EnemyActionVoices {
            begin: battle_voices::sound(pe::word(row, 60)?)?,
            release: battle_voices::sound(pe::word(row, 64)?)?,
        };
    }
    for contact in result.contacts.values_mut() {
        contact.hit.sound = contact.hit.sound.map(audio_effects::id).transpose()?;
    }
    for shot in result.projectiles.values_mut() {
        shot.hit.sound = shot.hit.sound.map(audio_effects::id).transpose()?;
        if let Some(feedback) = &mut shot.feedback {
            feedback.sound = audio_effects::id(feedback.sound)?;
        }
    }
    for attack in result.attacks.values_mut() {
        for (_, event) in &mut attack.events {
            if let resonance_content::battle_enemy::rules::Event::Command(
                resonance_content::battle_action::EnemyEvent::Sound(sound),
            ) = event
            {
                *sound = audio_effects::id(*sound)?;
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, package::Package, source::Source};
    use std::path::PathBuf;

    #[test]
    fn command_storage_preserves_wide_voice_ids_and_halfword_repeat() -> Result<()> {
        let mut source = vec![0; 30];
        source[2..4].copy_from_slice(&27u16.to_le_bytes());
        source[4..8].copy_from_slice(&3u32.to_le_bytes());
        source[8..12].copy_from_slice(&0x500003u32.to_le_bytes());
        source[14..16].copy_from_slice(&25u16.to_le_bytes());
        source[16..18].copy_from_slice(&1u16.to_le_bytes());
        source[18..20].copy_from_slice(&31u16.to_le_bytes());
        source[20..24].copy_from_slice(&0.5f32.to_le_bytes());
        source[24..26].copy_from_slice(&0x9999u16.to_le_bytes());
        source[26..28].copy_from_slice(&1u16.to_le_bytes());
        source[28..30].copy_from_slice(&u16::MAX.to_le_bytes());
        let normalized = commands(&source, &[0; 72])?;
        assert_eq!(voice(&normalized)?, (Some(Sound::Stream(0xcf03)), true, 12));
        assert_eq!(&normalized[24..28], &1u32.to_be_bytes());
        assert_eq!(&normalized[18..20], &31u16.to_be_bytes());
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; audits remaining enemy action conversion gaps"]
    fn installed_enemy_actions_use_shared_gc_compiler() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let library = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let usual = source.require("btlusual.pac")?;
        let package = Package::parse(&usual)?;
        let common = package.member(8)?.context("missing common actions")?;
        let mut failures = BTreeMap::<String, Vec<u16>>::new();
        let mut prepared = 0;
        for id in 0..crate::monsters::count(&image)? as u16 {
            let filename =
                image.text(pe::word(image.slice(0x8e5b50 + u32::from(id) * 4, 4)?, 0)?)?;
            let bytes = source.require(filename)?;
            match actions(&bytes, common, &library, id) {
                Ok(tables) => {
                    prepared += 1;
                    assert_eq!(tables.actions.rows.len(), section(&bytes, 10)?.len() / 72);
                }
                Err(error) => {
                    failures.entry(format!("{error:#}")).or_default().push(id);
                }
            }
        }
        eprintln!("Enemy action owners decoded: {prepared}; remaining: {failures:#?}");
        ensure!(failures.is_empty(), "enemy action conversion is incomplete");
        Ok(())
    }
}
