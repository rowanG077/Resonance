//! Inspect the complete battle audio namespace without decoding PCM.
use super::super::actions::Rel;
use crate::{
    compression,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result, ensure};
use resonance_audio_cook::{
    bank::{Bank, ObjectKind, Page},
    instrument,
};
use resonance_content::battle::audio::inventory::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(crate) fn recover(extracted: &Path) -> Result<AudioInventory> {
    let mut sources = BTreeMap::new();
    let mut read = |path: &str| -> Result<Vec<u8>> {
        let bytes = fs::read(extracted.join(path)).with_context(|| format!("read {path}"))?;
        sources.insert(path.to_owned(), crate::digest(&bytes));
        Ok(bytes)
    };
    let executable = read("sys/main.dol")?;
    read("files/US_r_Top2Btl.rel")?;
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
    let native = crate::battle::all::Sources::read(extracted)?;
    let usual = read(&format!("files/{}", native.usual))?;
    let enemy_archive = read(&format!("files/{}", native.enemy))?;
    let sample_archive = read(&format!(
        "files/{}",
        crate::all_assets::voice_bank_path(extracted)?
    ))?;
    let voices = read(&format!(
        "files/{}",
        super::binding::source(extracted, &executable)?
    ))?;
    let mut banks = BTreeMap::new();
    let mut bank_bytes = BTreeMap::new();
    let resident = crate::all_assets::roles::resident_banks(extracted, &executable)?;
    let paths = super::selection::banks(extracted, &executable, &resident)?;
    for path in paths.into_keys().chain([resident[0].clone()]) {
        let path = format!("files/{path}");
        let bytes = read(&path)?;
        let group = half(sections(&bytes)?[0], 4)?;
        if let Some(previous) = bank_bytes.get(&group) {
            ensure!(previous == &bytes, "conflicting native sound group {group}");
            continue;
        }
        let bank = sound_bank(&path, &bytes).with_context(|| format!("inventory {path}"))?;
        banks.insert(group, bank);
        bank_bytes.insert(group, bytes);
    }
    let bitmap = section(&usual, 11)?;
    let timings = section(&usual, 12)?;
    let party = (1..=9)
        .map(|character| {
            let row = rel.at((5, 0x3d30 + (usize::from(character) - 1) * 0x1f0))?;
            Ok((character, native_voices(word(row, 0x104)?, bitmap, &banks)?))
        })
        .collect::<Result<_>>()?;
    let directory = section(&usual, 10)?;
    let payload_directory = section(&usual, 13)?;
    let mut enemies = BTreeMap::new();
    for monster in 0..251u8 {
        let bytes = compression::decode(range(&enemy_archive, directory, usize::from(monster))?)
            .with_context(|| format!("enemy {monster} audio package"))?;
        ensure!(bytes.starts_with(b"em8\0"), "invalid enemy audio package");
        let metadata = usize::from(half(&bytes, 4)?);
        let voices = native_voices(word(&bytes, metadata + 0x104)?, bitmap, &banks)
            .with_context(|| format!("enemy {monster} native voices"))?;
        let at = word(&bytes, 0x1e4)? as usize;
        let embedded_bank = if at == 0 {
            None
        } else {
            let group = u16::from(
                *bytes
                    .get(metadata + 0xa9)
                    .context("truncated enemy sound group")?,
            );
            ensure!(group >= 19, "invalid enemy sound group {group}");
            let standalone = sections(
                bank_bytes
                    .get(&group)
                    .context("missing standalone enemy bank")?,
            )?;
            let embedded = bytes.get(at..).context("invalid embedded sound table")?;
            let mut tables_match_standalone = !standalone[1].is_empty();
            for (index, original) in standalone[..3].iter().enumerate() {
                let start = word(embedded, index * 4)? as usize;
                tables_match_standalone &=
                    embedded.get(start..start + original.len()) == Some(*original);
            }
            let samples = range(&sample_archive, payload_directory, usize::from(group - 19))?;
            Some(EmbeddedBank {
                group,
                tables_match_standalone,
                samples_match_standalone: samples == standalone[3],
            })
        };
        enemies.insert(
            monster,
            EnemyAudio {
                voices,
                embedded_bank,
            },
        );
    }
    let streams = crate::afs::parse(&voices)?
        .into_iter()
        .enumerate()
        .map(|(id, member)| {
            let bytes = member.data;
            ensure!(
                member.name.ends_with(".adx") && half(bytes, 0)? == 0x8000 && bytes.len() >= 20,
                "invalid battle ADX member {id}"
            );
            let header = usize::from(half(bytes, 2)?) + 4;
            ensure!(
                header >= 20 && header <= bytes.len() && bytes[5] == 18 && bytes[6] == 4,
                "unsupported battle ADX layout {id}"
            );
            Ok(StreamSource {
                name: member.name.to_owned(),
                sample_rate: word(bytes, 8)?,
                frames: word(bytes, 12)?,
                channels: bytes[7],
                encoding: bytes[4],
                version: bytes[18],
                source_ticks: half(timings, id * 2)?,
                sha256: crate::digest(bytes),
            })
        })
        .collect::<Result<_>>()?;
    let music = (85..=96)
        .map(|id| {
            let path = format!("files/{}", crate::media::music_path(&executable, id)?);
            read(&path)?;
            let reverb_preset = crate::dol::slice(&executable, 0x8021_08b0 + u32::from(id), 1)?[0];
            Ok((
                id,
                MusicSource {
                    path,
                    reverb_preset,
                },
            ))
        })
        .collect::<Result<_>>()?;
    let inventory = AudioInventory {
        version: AudioInventory::VERSION,
        banks,
        party,
        enemies,
        streams,
        music,
        sources,
    };
    inventory.validate()?;
    Ok(inventory)
}

fn sound_bank(path: &str, bytes: &[u8]) -> Result<SoundBank> {
    let bank = Bank::parse(bytes)?;
    let samples = bank.sample_ids()?;
    let mut silent_samples = BTreeSet::new();
    for &id in &samples {
        // The original dummy instrument uses one zero-valued sample. Inspect
        // tiny placeholders only; full audio decoding is a separate test/cook.
        if bank.sample_frames(id)? <= 1 && bank.sample(id)?.pcm.iter().all(|&sample| sample == 0) {
            silent_samples.insert(id);
        }
    }
    let programs = bank
        .object_ids(ObjectKind::Macro)
        .map(|id| {
            let bytes = bank.object(ObjectKind::Macro, id)?;
            ensure!(
                !bytes.is_empty() && bytes.len().is_multiple_of(8),
                "invalid sound program {id}"
            );
            let mut opcodes = BTreeMap::new();
            let mut samples = BTreeSet::new();
            for command in bytes.chunks_exact(8) {
                let first = word(command, 0)?;
                *opcodes.entry(first as u8).or_default() += 1;
                if first as u8 == 0x10 {
                    samples.insert((first >> 8) as u16);
                }
            }
            Ok((
                id,
                ProgramSource {
                    instructions: (bytes.len() / 8) as u32,
                    opcodes,
                    samples,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let sounds = bank
        .sound_ids()
        .map(|id| {
            let sound = bank.sound(id)?;
            let recovered = instrument::resolve(
                &bank,
                Page {
                    object: sound.object,
                    priority: 64,
                    max_voices: 255,
                },
                sound.key,
                sound.volume,
                sound.pan,
            );
            let (roots, unresolved): (BTreeSet<_>, _) = match recovered {
                Ok(notes) => (notes.into_iter().map(|note| note.macro_id).collect(), None),
                Err(error) => (BTreeSet::new(), Some(format!("{error:#}"))),
            };
            let empty = unresolved.is_none()
                && roots
                    .iter()
                    .map(|&id| Ok(word(bank.object(ObjectKind::Macro, id)?, 0)? as u8 == 0))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .all(|empty| empty);
            let zero_sample_program = !roots.is_empty()
                && roots.iter().all(|id| {
                    let program = &programs[id];
                    !program.samples.is_empty()
                        && program.samples.is_subset(&silent_samples)
                        && program
                            .opcodes
                            .keys()
                            .all(|opcode| matches!(opcode, 0 | 7 | 0x10 | 0x11 | 0x31 | 0x38))
                });
            let silent_samples = if zero_sample_program {
                roots
                    .iter()
                    .flat_map(|id| programs[id].samples.iter().copied())
                    .collect()
            } else {
                BTreeSet::new()
            };
            Ok((
                id,
                SoundSource {
                    object: sound.object,
                    key: sound.key,
                    volume: sound.volume,
                    pan: sound.pan,
                    programs: roots,
                    empty,
                    unresolved,
                    silent_samples,
                },
            ))
        })
        .collect::<Result<_>>()?;
    Ok(SoundBank {
        path: path.to_owned(),
        sounds,
        programs,
        samples,
    })
}

fn native_voices(
    base: u32,
    bitmap: &[u8],
    banks: &BTreeMap<u16, SoundBank>,
) -> Result<NativeVoices> {
    if base == 0 {
        return Ok(NativeVoices::Voiceless);
    }
    ensure!(base < 0x8000, "invalid native voice base {base}");
    let base = base as u16;
    let bank = banks
        .values()
        .find(|bank| bank.sounds.contains_key(&(base + 501)))
        .context("native voice base has no sound bank")?;
    let end = bank
        .sounds
        .last_key_value()
        .context("empty native voice bank")?
        .0
        + 1
        - 501;
    ensure!(
        end > base && end - base <= 128,
        "invalid native voice family {base}..{end}"
    );
    let ids = (base..end)
        .map(|id| {
            let bits = bitmap
                .get(usize::from(id / 8))
                .context("voice exceeds selection bitmap")?;
            Ok(id
                | if bits & (1 << (id % 8)) != 0 {
                    0x8000
                } else {
                    0
                })
        })
        .collect::<Result<_>>()?;
    Ok(NativeVoices::Voiced { base, ids })
}

fn section(bytes: &[u8], index: usize) -> Result<&[u8]> {
    let count = word(bytes, 0)? as usize;
    ensure!(index < count, "missing battle audio section");
    let start = word(bytes, 4 + index * 4)? as usize;
    let end = if index + 1 == count {
        bytes.len()
    } else {
        word(bytes, 8 + index * 4)? as usize
    };
    bytes
        .get(start..end)
        .context("invalid battle audio section")
}

fn range<'a>(bytes: &'a [u8], directory: &[u8], index: usize) -> Result<&'a [u8]> {
    let start = word(directory, index * 4)? as usize;
    let end = word(directory, (index + 1) * 4)? as usize;
    bytes.get(start..end).context("invalid audio source range")
}

fn sections(bytes: &[u8]) -> Result<[&[u8]; 4]> {
    ensure!(word(bytes, 0)? == 4, "invalid SND section count");
    let mut result = [&[][..]; 4];
    for (index, section) in result.iter_mut().enumerate() {
        let start = word(bytes, 4 + index * 8)? as usize;
        let length = word(bytes, 8 + index * 8)? as usize;
        *section = bytes
            .get(start..start + length)
            .context("truncated SND source section")?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the original extracted disc; decodes in memory, never opens an audio device"]
    fn every_public_battle_cue_compiles_and_decodes_its_sample_closure() {
        let root = std::env::var_os("RESONANCE_TEST_EXTRACTED").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1"),
            Into::into,
        );
        let inventory = recover(&root).unwrap();
        let executable = fs::read(root.join("sys/main.dol")).unwrap();
        let [instruments, common] =
            crate::all_assets::roles::resident_banks(&root, &executable).unwrap();
        let common = fs::read(root.join("files").join(common)).unwrap();
        let instruments = fs::read(root.join("files").join(instruments)).unwrap();
        let common = Bank::parse(&common).unwrap();
        let instruments = Bank::parse(&instruments).unwrap();
        let mut failures = Vec::new();
        let mut checked = 0;
        for source in inventory.banks.values() {
            if source
                .sounds
                .values()
                .any(|sound| sound.unresolved.is_some())
            {
                failures.push(format!("{}: unresolved source instruments", source.path));
                continue;
            }
            let bytes = fs::read(root.join(&source.path)).unwrap();
            let mut bank = Bank::parse(&bytes).unwrap();
            bank.inherit_tables(&common);
            bank.inherit_tables(&instruments);
            bank.inherit_samples(&instruments);
            bank.inherit_samples(&common);
            let roots: BTreeSet<_> = source
                .sounds
                .values()
                .flat_map(|sound| sound.programs.iter().copied())
                .collect();
            match resonance_audio_cook::compile::programs(&bank, roots) {
                Ok(resources) => {
                    checked += source.sounds.len();
                    resources.validate().unwrap();
                }
                Err(error) => failures.push(format!("{}: {error:#}", source.path)),
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        assert_eq!(
            checked,
            inventory
                .banks
                .values()
                .map(|bank| bank.sounds.len())
                .sum::<usize>()
        );
    }
}
