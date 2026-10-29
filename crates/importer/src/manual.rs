//! The training manual is one native table and one final JSON file.
use crate::{dol, text};
use anyhow::{Result, ensure};
use resonance_content::menu_data::{MANUAL_CHAPTERS, ManualChapter, ManualTopic, TrainingManual};
use std::{fs, path::Path};

/// Read the GameCube executable and write `game/menu/manual.json`.
pub fn cook_manual(extracted: &Path, library: &Path) -> Result<()> {
    let executable = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let document = convert(&executable)?;
    document.validate()?;
    let path = library.join(resonance_content::menu_data::MANUAL_PATH);
    crate::output::json(&path, &document)?;
    Ok(())
}

fn convert(exe: &[u8]) -> Result<TrainingManual> {
    let names = dol::slice(exe, 0x8019d7e4, MANUAL_CHAPTERS * 4)?;
    let counts = dol::slice(exe, 0x8019d808, MANUAL_CHAPTERS)?;
    let flags = dol::slice(exe, 0x8019d814, MANUAL_CHAPTERS * 5)?;
    let topics = dol::slice(exe, 0x801a0bdc, MANUAL_CHAPTERS * 5 * 8)?;
    let mut chapters = Vec::with_capacity(MANUAL_CHAPTERS);
    for chapter in 0..MANUAL_CHAPTERS {
        let count = usize::from(counts[chapter]);
        ensure!((1..=5).contains(&count), "invalid manual chapter length");
        let mut entries = Vec::with_capacity(count);
        for topic in 0..count {
            let index = chapter * 5 + topic;
            let row = &topics[index * 8..index * 8 + 8];
            let mut address = dol::u32(row, 4)?;
            ensure!(address != 0, "null selected manual body");
            let mut paragraphs = Vec::new();
            loop {
                let (source, end) = text::source(exe, address)?;
                paragraphs.push(text::decode(&source, 9)?);
                address = end;
                if dol::slice(exe, address, 1)? == [0] {
                    break;
                }
            }
            entries.push(ManualTopic {
                name: text::string(exe, dol::u32(row, 0)?)?,
                learned_flag: u16::from(flags[index]),
                paragraphs,
            });
        }
        chapters.push(ManualChapter {
            name: text::string(exe, dol::u32(names, chapter * 4)?)?,
            topics: entries,
        });
    }
    let title = dol::u32(dol::slice(exe, 0x8019d6f0, 4)?, 0)?;
    Ok(TrainingManual {
        title: text::string(exe, title)?,
        chapters,
    })
}
