//! Steam tables normalize into the GC normal-attack decoder and document.
use crate::{audio_effects, battle_voices::sound, output, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_normal::{
        Attack, Event,
        document::{Character, NormalAttacks},
    },
    library::Mappings,
};
use std::{collections::BTreeMap, path::Path};
use symphonia_constants::PartyCharacterId;

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let flights = canonical(
        image.slice(0x8e78fc, 48)?,
        16,
        &[(0, 2), (2, 2), (4, 4), (8, 4), (12, 4)],
    )?;
    let flights = resonance_importer::decode_weapon_flights(&flights)?;
    let directory = image.slice(0x8e9778, 9 * 12)?;
    let data = |address| -> Result<&[u8]> {
        ensure!(
            (0x8e7084..0x8e9778).contains(&address),
            "normal attack pointer outside tables"
        );
        image.slice(address, (0x8e9778 - address) as usize)
    };
    let mut characters = BTreeMap::new();
    let mut mappings = BTreeMap::new();
    for (character, row) in PartyCharacterId::ALL
        .into_iter()
        .zip(directory.as_chunks::<12>().0)
    {
        let pointers = image.slice(pe::word(row, 0)?, 7 * 16)?;
        let rules = data(pe::word(row, 4)?)?;
        let selectors = image.slice(pe::word(row, 8)?, 7 * 4)?;
        let attacks = (0..7)
            .map(|variant| -> Result<Attack> {
                let pointer = |member: usize| data(pe::word(pointers, variant * 16 + member * 4)?);
                let config = canonical(
                    pointer(0)?
                        .get(..24)
                        .context("truncated attack configuration")?,
                    2,
                    &[(0, 2)],
                )?;
                let contacts = stream(
                    pointer(1)?,
                    32,
                    -1,
                    &[(0, 2), (8, 4), (12, 4), (22, 2), (28, 4)],
                )?;
                let count = contacts
                    .as_chunks::<32>()
                    .0
                    .iter()
                    .map(|row| usize::from(row[18]) + 1)
                    .max()
                    .unwrap_or(0);
                let rules = canonical(
                    rules
                        .get(..count * 28)
                        .context("truncated attack hit rules")?,
                    28,
                    &[(0, 2), (8, 4), (14, 2), (16, 2)],
                )?;
                let poses = stream(pointer(2)?, 12, -2, &[(0, 2), (8, 4)])?;
                let commands = commands(pointer(3)?)?;
                let mut attack = resonance_importer::decode_normal_attack(
                    resonance_importer::NormalAttackSource {
                        config: &config,
                        rules: &rules,
                        contacts: &contacts,
                        poses: &poses,
                        commands: &commands,
                    },
                    resonance_importer::normal_attack_continuation(selectors, variant)?,
                    &flights,
                    voice,
                )?;
                for (_, event) in &mut attack.events {
                    match event {
                        Event::Sound(id) => *id = audio_effects::id(*id)?,
                        Event::Contact(contact) => {
                            contact.hit.sound =
                                contact.hit.sound.map(audio_effects::id).transpose()?
                        }
                        Event::Throw { hit, .. } => {
                            hit.sound = hit.sound.map(audio_effects::id).transpose()?
                        }
                        _ => (),
                    }
                }
                Ok(attack)
            })
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("{} normal attacks", character.resource_name().unwrap()))?;
        let name = character
            .resource_name()
            .context("unnamed party character")?;
        mappings.insert(u32::from(character.raw()), name.to_owned());
        characters.insert(
            name.to_owned(),
            Character::from_attacks(attacks.try_into().unwrap())?,
        );
    }
    output::json(
        &library.join(resonance_content::battle_rules::NORMAL_ATTACKS),
        &NormalAttacks {
            mappings: Mappings::new(mappings)?,
            characters,
        },
    )
}

pub(crate) fn canonical(bytes: &[u8], width: usize, fields: &[(usize, usize)]) -> Result<Vec<u8>> {
    ensure!(bytes.len().is_multiple_of(width), "partial attack record");
    let mut bytes = bytes.to_vec();
    for row in bytes.chunks_exact_mut(width) {
        for &(at, length) in fields {
            row.get_mut(at..at + length)
                .context("numeric field exceeds attack record")?
                .reverse();
        }
    }
    Ok(bytes)
}

fn stream(bytes: &[u8], width: usize, end: i16, fields: &[(usize, usize)]) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    for at in (0..bytes.len()).step_by(width) {
        let time = pe::half(bytes, at)? as i16;
        if time == end {
            result.extend(time.to_be_bytes());
            return Ok(result);
        }
        result.extend(canonical(
            bytes
                .get(at..at + width)
                .context("truncated attack stream")?,
            width,
            fields,
        )?);
    }
    anyhow::bail!("unterminated attack stream")
}

fn commands(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut at = 0;
    loop {
        let time = pe::half(bytes, at)? as i16;
        if time == -1 {
            result.extend(time.to_be_bytes());
            return Ok(result);
        }
        let opcode = pe::half(bytes, at + 2)?;
        let (width, fields): (_, &[(usize, usize)]) = match opcode {
            0 | 1 => (6, &[(0, 2), (2, 2), (4, 2)]),
            7 => (6, &[(0, 2), (2, 2)]), // Slot/visibility are two byte fields.
            28 => (8, &[(0, 2), (2, 2), (4, 2), (6, 2)]),
            27 => (12, &[(0, 2), (2, 2), (4, 4), (8, 4)]),
            _ => anyhow::bail!("unsupported normal command {opcode}"),
        };
        result.extend(canonical(
            bytes
                .get(at..at + width)
                .context("truncated attack command")?,
            width,
            fields,
        )?);
        at += width;
    }
}

fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(at..at + 4)
            .context("truncated attack voice")?
            .try_into()?,
    ))
}
fn half(bytes: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(
        bytes
            .get(at..at + 2)
            .context("truncated attack voice")?
            .try_into()?,
    ))
}

pub(crate) fn voice(bytes: &[u8]) -> Result<(Event, usize)> {
    if half(bytes, 2)? == 42 {
        ensure!(
            (2..=3).contains(&half(bytes, 4)?),
            "invalid voice-choice priority"
        );
        let choice = resonance_content::battle_voice::Choice {
            first_chance: half(bytes, 6)?.try_into()?,
            sounds: [sound(word(bytes, 8)?)?, sound(word(bytes, 12)?)?],
        };
        choice.validate()?;
        return Ok((Event::VoiceChoice(choice), 16));
    }
    ensure!(
        half(bytes, 2)? == 27 && (2..=3).contains(&word(bytes, 4)?),
        "invalid voice priority"
    );
    Ok((Event::Voice(sound(word(bytes, 8)?)?), 12))
}
