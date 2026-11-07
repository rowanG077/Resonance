//! Character ABNF banks write ordinary WAV speech and shared stream identities.
use crate::{audio_streams, output, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_voice::Sound,
    field_audio::{document, speech},
};
use std::{collections::BTreeSet, fs, ops::Range, path::Path};
use symphonia_constants::PartyCharacterId;

/// Keep packed native bank/row selectors inside this importer.
pub(crate) fn sound(id: u32) -> Result<Option<Sound>> {
    if id == 0 {
        return Ok(None);
    }
    let id = if id < 0x8000 {
        id
    } else {
        let bank = id >> 16;
        ensure!(
            (1..=80).contains(&bank) && id & 0xffff < 256,
            "unsupported actor voice {id:#x}"
        );
        0x8000 + (bank - 1) * 256 + (id & 0xffff)
    };
    Ok(Some(Sound::Stream(id.try_into()?)))
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    ensure!(
        image.slice(0x428cc3, 7)? == [0x8b, 0x0c, 0x85, 0x20, 0x98, 0x8e, 0]
            && image.slice(0x428cd0, 7)? == [0x8b, 0x04, 0x95, 0x48, 0x98, 0x8e, 0],
        "changed party voice directory"
    );
    let mut counts = None;
    for (language, table) in [("english", 0x8e9848), ("japanese", 0x8e9820)] {
        let path = library.join(format!("audio/speech/{language}/mappings.json"));
        let mut mappings: speech::Mappings = serde_json::from_slice(&fs::read(&path)?)?;
        let mut rows = Vec::new();
        for character in PartyCharacterId::ALL {
            let id = u32::from(character.raw());
            let filename = image.text(pe::word(image.slice(table + id * 4, 4)?, 0)?)?;
            let bytes = source.require(filename)?;
            let ranges = ranges(&bytes, id as u16)?;
            rows.push(ranges.len());
            let name = character
                .resource_name()
                .context("unnamed voice character")?;
            eprintln!("  Party voices: {language}/{name} ({} slots)", ranges.len());
            for (index, range) in ranges.into_iter().enumerate() {
                if range.is_empty() {
                    continue;
                }
                let target = format!("{language}/party/{name}/line-{index:03}.wav");
                audio_streams::wave(&bytes[range], &library.join("audio/speech").join(&target))?;
                let key = 0x8000 + (id - 1) * 256 + index as u32;
                let entry = speech::Source::Wave(target);
                if let Some(previous) = mappings.insert(key, entry.clone()) {
                    ensure!(
                        previous == entry,
                        "actor voice collides with speech ID {key}"
                    );
                }
            }
        }
        if let Some(previous) = &counts {
            ensure!(
                previous == &rows,
                "party voice languages have different slots"
            );
        } else {
            counts = Some(rows);
        }
        output::json(&path, &mappings)?;
        if language == "english" {
            output::json(&library.join(document::SPEECH), &mappings)?;
        }
    }
    Ok(())
}

/// The native enemy loader registers English ABNs only. Keep that language
/// selection in the cooked speech catalogue rather than in the battle player.
pub(crate) fn enemies(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    ensure!(
        image.slice(0x428bfb, 4)? == [0x66, 0x83, 0xf8, 0x50]
            && image.slice(0x458ddc, 8)? == [0x38, 0x8a, 0x74, 2, 0, 0, 0x75, 0x2d]
            && image.slice(0x458de4, 7)? == [0x8b, 0x1c, 0x9d, 0x58, 0x5f, 0x8e, 0],
        "enemy voice directory changed"
    );
    let mut english: speech::Mappings = serde_json::from_slice(&fs::read(
        library.join("audio/speech/english/mappings.json"),
    )?)?;
    let mut japanese: speech::Mappings = serde_json::from_slice(&fs::read(
        library.join("audio/speech/japanese/mappings.json"),
    )?)?;
    let mut names = BTreeSet::new();
    let mut recordings = 0;
    let mut empty = 0;
    for bank in 10..=80u16 {
        let filename = format!("{bank:02}e.abn");
        if !source.contains(&filename)? {
            continue;
        }
        let bytes = source.require(&filename)?;
        names.insert(filename);
        for (index, range) in ranges(&bytes, bank)?.into_iter().enumerate() {
            let Sound::Stream(id) = sound((u32::from(bank) << 16) | index as u32)?.unwrap() else {
                unreachable!()
            };
            let name = format!("enemy-{bank:02}/line-{index:03}");
            // Neither an empty native slot nor an unregistered Japanese bank
            // plays a recording. The common silence recipe expresses both
            // without manufacturing WAV files or changing voice identities.
            let silence = speech::Source::Silence {
                name,
                silence: speech::Clock {
                    frames: 1,
                    sample_rate: 48000,
                    channels: 1,
                },
            };
            let entry = if range.is_empty() {
                empty += 1;
                silence.clone()
            } else {
                let target = format!("english/enemy-{bank:02}/line-{index:03}.wav");
                audio_streams::wave(&bytes[range], &library.join("audio/speech").join(&target))?;
                recordings += 1;
                speech::Source::Wave(target)
            };
            for (mappings, entry) in [(&mut english, entry), (&mut japanese, silence)] {
                if let Some(previous) = mappings.insert(u32::from(id), entry.clone()) {
                    ensure!(
                        previous == entry,
                        "enemy voice collides with speech ID {id}"
                    );
                }
            }
        }
    }
    for id in 0..crate::monsters::count(&image)? {
        let pointer = pe::word(image.slice(0x8e5f58 + id as u32 * 4, 4)?, 0)?;
        ensure!(
            pointer == 0 || names.contains(image.text(pointer)?),
            "missing declared enemy voice bank for enemy {id}"
        );
    }
    output::json(
        &library.join("audio/speech/english/mappings.json"),
        &english,
    )?;
    output::json(
        &library.join("audio/speech/japanese/mappings.json"),
        &japanese,
    )?;
    output::json(&library.join(document::SPEECH), &english)?;
    eprintln!(
        "  Enemy voices: {} banks, {recordings} recordings, {empty} empty slots",
        names.len()
    );
    Ok(())
}

fn ranges(bytes: &[u8], character: u16) -> Result<Vec<Range<usize>>> {
    ensure!(
        bytes.get(..4) == Some(b"ABNF")
            && pe::half(bytes, 4)? == character
            && bytes.get(6..16) == Some(&[0; 10]),
        "invalid actor voice bank"
    );
    let count = pe::word(bytes, 16)? as usize;
    ensure!((1..=256).contains(&count), "invalid voice slot count");
    let header = 20 + count * 8;
    let mut ranges = Vec::<Range<usize>>::new();
    for index in 0..count {
        let offset = pe::word(bytes, 20 + index * 8)? as usize;
        let size = pe::word(bytes, 24 + index * 8)? as usize;
        if offset == 0 && size == 0 {
            ranges.push(0..0);
            continue;
        }
        let start = 16usize
            .checked_add(offset)
            .context("voice offset overflow")?;
        let end = start.checked_add(size).context("voice length overflow")?;
        ensure!(
            start >= header
                && start < end
                && end <= bytes.len()
                && ranges
                    .iter()
                    .all(|range| range.end <= start || end <= range.start),
            "voice outside bank or overlapping another slot"
        );
        ranges.push(start..end);
    }
    Ok(ranges)
}

/// The native player (0x428c14) accepts row == count, then reads the following
/// offset/size pair. Some banks pad that terminal pair with zeroes: it has no
/// recording to prepare. Do not extend this to arbitrary out-of-range slots.
pub(crate) fn empty_terminal(bytes: &[u8], character: u16, row: usize) -> Result<bool> {
    let ranges = ranges(bytes, character)?;
    if row < ranges.len() {
        return Ok(false);
    }
    ensure!(row == ranges.len(), "actor voice exceeds native bank count");
    let terminal = 20 + row * 8;
    let first_payload = ranges
        .iter()
        .filter(|range| !range.is_empty())
        .map(|range| range.start)
        .min()
        .unwrap_or(bytes.len());
    ensure!(
        terminal + 8 <= first_payload && bytes.get(terminal..terminal + 8) == Some(&[0; 8]),
        "actor voice terminal has no proven empty source entry"
    );
    Ok(true)
}

/// Native role indices are common; only their source sound storage differs.
pub(crate) fn selectors(image: &pe::Image<'_>) -> Result<Vec<u32>> {
    let mut address = 0x442027;
    let mut roles = Vec::new();
    for slot in 0..58_i32 {
        let displacement = -236 + slot * 4;
        let width = if displacement < -128 { 10 } else { 7 };
        let code = image.slice(address, width)?;
        if width == 10 {
            ensure!(
                code[..2] == [0xc7, 0x85] && pe::word(code, 2)? as i32 == displacement,
                "voice role initializer changed"
            );
        } else {
            ensure!(
                code[..2] == [0xc7, 0x45] && i32::from(code[2] as i8) == displacement,
                "voice role initializer changed"
            );
        }
        roles.push(pe::word(code, width - 4)?);
        address += width as u32;
    }
    ensure!(address == 0x44220e, "voice role count changed");
    Ok(roles)
}

pub(crate) fn role(selector: u32, row: &[u8]) -> Result<Sound> {
    let id = if selector & 0x10000000 != 0 {
        ensure!(selector & 0xefff0000 == 0, "unknown stream voice role");
        pe::word(row, 0x110)?.checked_add(selector & 0xffff)
    } else {
        ensure!(selector < 256, "unknown actor voice role");
        pe::word(row, 0x10c)?.checked_add(selector)
    }
    .context("actor voice role overflow")?;
    sound(id)?.context("actor voice role resolves to silence")
}

pub(crate) fn actor(
    selectors: &[u32],
    row: &[u8],
) -> Result<Option<resonance_content::battle_voice::Voices>> {
    if pe::word(row, 0x10c)? == 0 {
        return Ok(None);
    }
    let lines = selectors
        .iter()
        .map(|&selector| role(selector, row))
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(resonance_importer::decode_actor_voice_roles(&lines)))
}

pub(crate) fn party_extras(
    image: &pe::Image<'_>,
    profiles: &mut [resonance_content::battle_profile::Profile],
) -> Result<()> {
    let selector = |address| -> Result<Sound> {
        let code = image.slice(address, 5)?;
        ensure!(code[0] == 0xbe, "character voice selector changed");
        sound(pe::word(code, 1)?)?.context("character voice is silent")
    };
    ensure!(
        image.slice(0x485158, 5)? == [0x68, 0x92, 0, 0, 0],
        "Lucky Charge consumer changed"
    );
    let charge = selector(0x485181)?;
    ensure!(
        charge == selector(0x48528c)?,
        "Charge success branches differ"
    );
    let presea = profiles[6]
        .voices
        .as_mut()
        .context("missing Presea voices")?;
    presea.charge = Some(charge);
    presea.charge_failed = Some(selector(0x48524a)?);
    ensure!(
        image.slice(0x465f4c, 3)? == [0x80, 0xfa, 2]
            && image.slice(0x465f51, 7)? == [0x80, 0xbf, 0xb0, 1, 0, 0, 0x11],
        "Colette stun consumer changed"
    );
    profiles[1]
        .voices
        .as_mut()
        .context("missing Colette voices")?
        .stunned = Some(selector(0x465f5d)?);
    // Role 15 also supplies Raine's discovery line.
    let raine = profiles[3]
        .voices
        .as_mut()
        .context("missing Raine voices")?;
    raine.item_discovery = raine.acquisition;
    Ok(())
}

pub(crate) fn casting_policies(
    image: &pe::Image<'_>,
    library: &Path,
) -> Result<Vec<resonance_content::battle_profile::VoicePolicy>> {
    use resonance_content::battle_profile::{VoicePolicy, VoiceSequence};
    let recordings: speech::Mappings =
        serde_json::from_slice(&fs::read(library.join(document::SPEECH))?)?;
    let ticks = |voice: Option<Sound>| -> Result<u16> {
        let Some(Sound::Stream(id)) = voice else {
            return Ok(0);
        };
        let path = recordings
            .get(&u32::from(id))
            .with_context(|| format!("missing casting voice {id}"))?
            .resolve()?;
        let path = path.path().context("casting voice has no recording")?;
        let wave = resonance_audio::recording::Prepared::new(fs::File::open(library.join(path))?)?;
        // The retained resident duration table still indexes GC voices. Steam's
        // actual AWB/ABN recordings supply the common 30 Hz clock instead.
        Ok((u64::from(wave.frames()) * 30)
            .div_ceil(u64::from(wave.sample_rate()))
            .try_into()?)
    };
    let selectors = selectors(image)?;
    (0..9)
        .map(|index| {
            let row = image.slice(0x8e1f18 + index * 512, 512)?;
            let chant = Some(role(selectors[7], row)?);
            let default = VoiceSequence {
                chant,
                chant_ticks: ticks(chant)?,
                self_chant: chant,
                release: None,
                fallback: chant,
                fallback_ticks: ticks(chant)?,
            };
            let mut policy = VoicePolicy {
                default,
                techniques: Default::default(),
            };
            let pointer = pe::word(image.slice(0x91902c + (index + 1) * 4, 4)?, 0)?;
            if pointer != 0 {
                let mut terminated = false;
                for entry in 0..128 {
                    let row = image.slice(pointer + entry * 12, 12)?;
                    let technique = pe::word(row, 0)?;
                    if technique == 0 {
                        terminated = true;
                        break;
                    }
                    let chant = sound(pe::word(row, 4)?)?.or(default.chant);
                    ensure!(
                        policy
                            .techniques
                            .insert(
                                technique.try_into()?,
                                VoiceSequence {
                                    chant,
                                    chant_ticks: ticks(chant)?,
                                    self_chant: if index == 1 {
                                        chant
                                    } else {
                                        default.self_chant
                                    },
                                    release: sound(pe::word(row, 8)?)?,
                                    ..default
                                }
                            )
                            .is_none(),
                        "duplicate casting voice policy"
                    );
                }
                ensure!(terminated, "unterminated casting voice policy");
            }
            Ok(policy)
        })
        .collect()
}

pub(crate) fn overlimit(image: &pe::Image<'_>) -> Result<[Option<Sound>; 9]> {
    ensure!(
        image.slice(0x439438, 4)? == [0x8b, 0x74, 0x8c, 0x38],
        "Over Limit voice consumer changed"
    );
    (0..9u32)
        .map(|index| {
            let code = image.slice(0x439080 + index * 8, 8)?;
            ensure!(
                code[..4] == [0xc7, 0x44, 0x24, 0x3c + index as u8 * 4],
                "Over Limit voice initializer changed"
            );
            sound(pe::word(code, 4)?)
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing Over Limit voice"))
}
