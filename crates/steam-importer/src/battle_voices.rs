//! Character ABNF banks write ordinary WAV speech and shared stream identities.
use crate::{audio_streams, output, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_voice::Sound,
    field_audio::{document, speech},
};
use std::{fs, ops::Range, path::Path};
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
