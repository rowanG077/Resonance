//! Executable bank selections become ordinary WAV music, speech and effects.
use crate::{audio_effects, awb, output, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field_audio::{document, speech},
    library::Mappings,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    ops::Range,
    path::Path,
};
use symphonia_constants::MusicId;

pub(crate) const RAW: &str = "P12E/Data/TOS_Server/Rawdata/WIN";

pub(crate) fn cook(source: &Source, library: &Path) -> Result<BTreeMap<u32, String>> {
    let image = pe::Image::parse(&source.executable)?;
    let mut speech_bases = BTreeMap::new();
    let directory = image.slice(0x8adc78, 18 * 12)?;
    ensure!(
        directory[17 * 12..] == [0; 12],
        "missing speech directory terminator"
    );
    for row in directory[..17 * 12].chunks_exact(12) {
        let base = pe::word(row, 4)?;
        ensure!(base != 0 && base & 0xffff == 0, "invalid speech group");
        ensure!(
            speech_bases
                .insert(u32::from(pe::half(row, 8)?), base)
                .is_none(),
            "duplicate speech bank"
        );
    }
    let songs = image.slice(0x8ad188, 115 * 8)?;
    ensure!(
        pe::half(songs, 114 * 8)? == 0xffff,
        "missing music directory terminator"
    );
    let mut music_ids = BTreeSet::new();
    for row in songs[..114 * 8].chunks_exact(8) {
        let id = pe::half(row, 0)?;
        match pe::half(row, 2)? {
            0 => {}
            4 => {
                music_ids.insert(id);
            }
            mode => anyhow::bail!("unsupported music mode {mode}"),
        }
    }
    let mut voices = [speech::Mappings::new(), speech::Mappings::new()];
    let mut battle_counts = [None; 2];
    let mut music = BTreeMap::new();
    let mut effects = BTreeMap::new();
    let mut resources = BTreeSet::new();
    for (bank, row) in image.slice(0x8acf98, 20 * 8)?.chunks_exact(8).enumerate() {
        let name = image.text(pe::word(row, 0)?)?;
        crate::source::validate_name(name)?;
        let resource = pe::word(row, 4)?;
        ensure!(
            resources.insert(resource) && name.ends_with(".AWB"),
            "invalid stream bank"
        );
        let owner = name
            .strip_suffix(".AWB")
            .unwrap()
            .to_ascii_lowercase()
            .replace('_', "-");
        for (language, directory) in ["english", "japanese"].into_iter().enumerate() {
            if language == 1 && bank >= 18 {
                continue;
            }
            let raw = source.root.join(RAW);
            let path = if language == 1 {
                raw.join("VOICE_JP").join(name)
            } else {
                raw.join(name)
            };
            let mut input =
                fs::File::open(&path).with_context(|| format!("open {}", path.display()))?;
            let entries = awb::index(&mut input)?;
            if matches!(resource, 120 | 130) {
                ensure!(
                    entries
                        .iter()
                        .enumerate()
                        .all(|(id, entry)| usize::from(entry.id) == id),
                    "unordered battle stream IDs"
                );
                if resource == 120 {
                    battle_counts[language] = Some(entries.len());
                } else {
                    ensure!(
                        entries.len()
                            > battle_counts[language]
                                .context("legacy voices precede battle streams")?,
                        "legacy voices do not extend battle streams"
                    );
                }
            }
            eprintln!(
                "  Audio bank: {directory}/{name} ({} recordings)",
                entries.len()
            );
            for entry in entries {
                let bytes = entry.read(&mut input)?;
                let path = if resource == 200 {
                    let name = MusicId::try_from(entry.id)
                        .ok()
                        .and_then(MusicId::resource_name)
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("song-{:04}", entry.id));
                    let target = format!("{name}.wav");
                    if music_ids.remove(&entry.id) {
                        music.insert(u32::from(entry.id), target.clone());
                    }
                    format!("audio/music/{target}")
                } else if resource == 121 {
                    format!("audio/effects/streams/{owner}/stream-{:04}.wav", entry.id)
                } else {
                    format!("audio/speech/{directory}/{owner}/line-{:04}.wav", entry.id)
                };
                let repeat = wave(&bytes, &library.join(&path))
                    .with_context(|| format!("decode {directory}/{name}:{}", entry.id))?;
                if resource == 121 {
                    ensure!(entry.id < 256, "stream effect ID exceeds selector");
                    let definition = path.replace(".wav", ".json");
                    audio_effects::recording(library, &definition, &path, repeat, 1.)?;
                    effects.insert(
                        u32::from(audio_effects::id(0x8000 | entry.id)?),
                        definition.strip_prefix("audio/").unwrap().to_owned(),
                    );
                } else if let Some(&base) = speech_bases.get(&resource) {
                    let value = speech::Source::Wave(
                        path.strip_prefix("audio/speech/").unwrap().to_owned(),
                    );
                    ensure!(
                        voices[language]
                            .insert(base | u32::from(entry.id), value.clone())
                            .is_none(),
                        "duplicate speech ID"
                    );
                    if resource == 120 {
                        ensure!(
                            entry.id < 0x8000
                                && voices[language]
                                    .insert(u32::from(entry.id), value)
                                    .is_none(),
                            "invalid battle voice alias"
                        );
                    }
                } else if resource == 130
                    && usize::from(entry.id) >= battle_counts[language].unwrap()
                {
                    let value = speech::Source::Wave(
                        path.strip_prefix("audio/speech/").unwrap().to_owned(),
                    );
                    ensure!(
                        voices[language]
                            .insert(u32::from(entry.id), value)
                            .is_none(),
                        "duplicate legacy battle voice ID"
                    );
                } else {
                    ensure!(
                        resource == 200 || resource == 130,
                        "unbound speech bank {resource}"
                    );
                }
            }
        }
    }
    // The executable also declares tos_ending.adx (42156), absent from BGM.AWB.
    // Its owner is the ending media pass; do not map it to another recording.
    if !music_ids.is_empty() {
        eprintln!("  Music declarations without bank recordings: {music_ids:?}");
    }
    output::json(&library.join(document::MUSIC), &Mappings::new(music)?)?;
    for (language, mapping) in ["english", "japanese"].into_iter().zip(&voices) {
        output::json(
            &library.join(format!("audio/speech/{language}/mappings.json")),
            mapping,
        )?;
    }
    output::json(&library.join(document::SPEECH), &voices[0])?;
    silence_skits(source, library)?;
    Ok(effects)
}

/// The native English setting mutes the three skit banks. Preserve their exact
/// clocks in the existing silence representation; Japanese keeps its recordings.
pub(crate) fn silence_skits(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let branch = image.slice(0x5710b9, 30)?;
    ensure!(
        branch[..2] == [0x83, 0xc6]
            && branch[3..6] == [0x66, 0x83, 0xfe]
            && branch[7..]
                == [
                    0x77, 0x24, 0xa1, 0x68, 0x3f, 0xad, 0, 0x80, 0xb8, 0x74, 2, 0, 0, 0, 0x75, 7,
                    0xb9, 0x40, 0xfc, 0xff, 0xff, 0xeb, 0x35
                ],
        "changed skit voice mute branch"
    );
    let first = u32::try_from(-i32::from(branch[2] as i8))?;
    let last = first + u32::from(branch[6]);
    ensure!(first > 0 && last < 17, "invalid muted speech bank range");
    let bases = (first..=last)
        .map(|slot| pe::word(image.slice(0x8adc78 + slot * 12, 12)?, 4))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut voices: speech::Mappings = serde_json::from_slice(&fs::read(
        library.join("audio/speech/english/mappings.json"),
    )?)?;
    for (id, source) in &mut voices {
        if !bases.contains(&(*id & 0xffff0000)) {
            continue;
        }
        if let speech::Source::Wave(path) = source {
            let wave = hound::WavReader::open(library.join("audio/speech").join(&path))?;
            let spec = wave.spec();
            let silence = speech::Clock {
                frames: wave.duration(),
                sample_rate: spec.sample_rate,
                channels: spec.channels,
            };
            silence.validate()?;
            *source = speech::Source::Silence {
                name: path.strip_suffix(".wav").unwrap_or(path).to_owned(),
                silence,
            };
        }
    }
    output::json(&library.join("audio/speech/english/mappings.json"), &voices)?;
    output::json(&library.join(document::SPEECH), &voices)
}

/// Reuse GC's decoder; standard RIFF sampler metadata retains ADX's loop clock.
pub(crate) fn wave(bytes: &[u8], path: &Path) -> Result<Option<Range<u32>>> {
    resonance_importer::convert_adx(bytes, path)?;
    let repeat = repeat(bytes)?;
    if let Some(repeat) = &repeat {
        let mut file = fs::OpenOptions::new().read(true).write(true).open(path)?;
        let rate = u32::from_be_bytes(bytes[8..12].try_into()?);
        resonance_asset_writer::wav::append_loop(&mut file, rate, repeat.clone())?;
    }
    Ok(repeat)
}

fn repeat(bytes: &[u8]) -> Result<Option<Range<u32>>> {
    let word = |at| -> Result<u32> {
        Ok(u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .context("truncated ADX loop")?
                .try_into()?,
        ))
    };
    ensure!(bytes.len() >= 20, "truncated ADX header");
    let end = usize::from(u16::from_be_bytes(bytes[2..4].try_into()?))
        .checked_sub(2)
        .context("invalid ADX loop offset")?;
    let at = match u16::from_be_bytes(bytes[18..20].try_into()?) {
        0x0300 => 0x14,
        0x0400 => 0x20,
        0x0500 => return Ok(None),
        _ => anyhow::bail!("unsupported ADX loop version"),
    };
    if at + 24 > end || word(at + 4)? == 0 {
        return Ok(None);
    }
    let repeat = word(at + 8)?..word(at + 16)?;
    ensure!(
        repeat.start < repeat.end && repeat.end <= word(12)?,
        "ADX loop outside recording"
    );
    Ok(Some(repeat))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loops_use_sample_frames_and_survive_the_shared_wave_writer() -> Result<()> {
        for (version, at) in [(0x0300u16, 0x14), (0x0400, 0x20)] {
            let mut bytes = vec![0; 0x40 + 18];
            bytes[..8].copy_from_slice(&[0x80, 0, 0, 0x3c, 2, 18, 4, 1]);
            bytes[8..12].copy_from_slice(&32000u32.to_be_bytes());
            bytes[12..16].copy_from_slice(&3u32.to_be_bytes());
            bytes[18..20].copy_from_slice(&version.to_be_bytes());
            bytes[at + 4..at + 8].copy_from_slice(&1u32.to_be_bytes());
            bytes[at + 8..at + 12].copy_from_slice(&1u32.to_be_bytes());
            bytes[at + 16..at + 20].copy_from_slice(&3u32.to_be_bytes());
            bytes[0x3a..0x40].copy_from_slice(b"(c)CRI");
            bytes[0x42..0x44].copy_from_slice(&[0x12, 0x30]);
            let output = tempfile::tempdir()?;
            let path = output.path().join("loop.wav");
            assert_eq!(wave(&bytes, &path)?, Some(1..3));
            let music = resonance_audio::recording::Prepared::new(fs::File::open(&path)?)?;
            assert_eq!(
                (music.frames(), music.sample_rate(), music.repeat()),
                (3, 32000, Some(1..3))
            );
            let mut player = music.start(32000)?;
            let expected = [1., 2., 3., 2., 3.];
            for sample in expected {
                let frame = player.next_frame()?.unwrap();
                assert!(frame.iter().all(|value| {
                    (value - sample * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.00001
                }));
            }
            bytes[at + 16..at + 20].copy_from_slice(&4u32.to_be_bytes());
            assert!(repeat(&bytes).is_err());
        }
        Ok(())
    }
}
