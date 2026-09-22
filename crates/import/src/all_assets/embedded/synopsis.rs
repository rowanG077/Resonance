//! Complete synopsis records and the training manual's fixed topic grid.
use super::text::{TextPool, TextRef};
use crate::{
    dol,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result};
use resonance_content::menu_data::{MANUAL_CHAPTERS, SYNOPSIS_COUNT};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
const FAMILY: &str = "synopsis-manual";
const ENTRIES: u32 = 0x802a1c30;
const CHAPTERS: u32 = 0x8019d7e4;
const COUNTS: u32 = 0x8019d808;
const FLAGS: u32 = 0x8019d814;
const TOPICS: u32 = 0x801a0bdc;
const TOPICS_PER_CHAPTER: usize = 5;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) entries: Vec<Entry>,
    /// The second label is retained even though the synopsis renderer only uses the first.
    pub(crate) headings: [Option<TextRef>; 2],
    pub(crate) months: [Option<TextRef>; 12],
    pub(crate) formats: Formats,
    pub(crate) manual: Manual,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }

    pub(crate) fn required(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required reading-menu text")?))
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    /// No meaning is established for the first halfword; all shipped rows store zero.
    pub(crate) storage: u16,
    /// Below 0x100 selects Sylvarant; below 0x152 selects Tethe'alla; otherwise no map.
    pub(crate) location: u16,
    pub(crate) heading: Option<TextRef>,
    pub(crate) title: Option<TextRef>,
    pub(crate) text: [Option<TextRef>; 3],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Formats {
    pub(crate) date: TextRef,
    pub(crate) level: TextRef,
    pub(crate) position: TextRef,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Manual {
    pub(crate) title: Option<TextRef>,
    pub(crate) chapters: [Chapter; MANUAL_CHAPTERS],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Chapter {
    pub(crate) name: Option<TextRef>,
    pub(crate) topic_count: u8,
    pub(crate) topics: [Topic; TOPICS_PER_CHAPTER],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Topic {
    pub(crate) name: Option<TextRef>,
    /// Zero never unlocks a topic; other values are story-flag indices.
    pub(crate) learned_flag: u8,
    /// A null body differs from a present, empty paragraph. Every body ends in two zeroes.
    pub(crate) paragraphs: Option<Vec<TextRef>>,
}

fn paragraphs(
    texts: &mut TextPool,
    executable: &[u8],
    mut address: u32,
) -> Result<Option<Vec<TextRef>>> {
    if address == 0 {
        return Ok(None);
    }
    let mut paragraphs = Vec::new();
    loop {
        let (reference, next) = texts.read(executable, address)?;
        address = next;
        paragraphs.push(reference);
        if dol::slice(executable, address, 1)? == [0] {
            return Ok(Some(paragraphs));
        }
    }
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let mut texts = TextPool::default();
    let entries = dol::slice(executable, ENTRIES, SYNOPSIS_COUNT * 24)?
        .chunks_exact(24)
        .map(|row| {
            Ok(Entry {
                storage: half(row, 0)?,
                location: half(row, 2)?,
                heading: texts.reference(executable, word(row, 4)?)?,
                title: texts.reference(executable, word(row, 8)?)?,
                text: [
                    texts.reference(executable, word(row, 12)?)?,
                    texts.reference(executable, word(row, 16)?)?,
                    texts.reference(executable, word(row, 20)?)?,
                ],
            })
        })
        .collect::<Result<_>>()?;
    let names = texts.array::<MANUAL_CHAPTERS>(executable, CHAPTERS)?;
    let counts = dol::slice(executable, COUNTS, MANUAL_CHAPTERS)?;
    let flags = dol::slice(executable, FLAGS, MANUAL_CHAPTERS * TOPICS_PER_CHAPTER)?;
    let topics = dol::slice(executable, TOPICS, MANUAL_CHAPTERS * TOPICS_PER_CHAPTER * 8)?;
    let chapters = names
        .into_iter()
        .enumerate()
        .map(|(chapter, name)| {
            let topics = (0..TOPICS_PER_CHAPTER)
                .map(|topic| {
                    let index = chapter * TOPICS_PER_CHAPTER + topic;
                    let row = &topics[index * 8..index * 8 + 8];
                    Ok(Topic {
                        name: texts.reference(executable, word(row, 0)?)?,
                        learned_flag: flags[index],
                        paragraphs: paragraphs(&mut texts, executable, word(row, 4)?)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap();
            Ok(Chapter {
                name,
                topic_count: counts[chapter],
                topics,
            })
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    Ok(Catalogue {
        entries,
        headings: texts.array(executable, 0x8035d8f8)?,
        months: texts.array(executable, 0x801df814)?,
        formats: Formats {
            date: texts.required(executable, 0x801df844)?,
            level: texts.required(executable, 0x8035d9a4)?,
            position: texts.required(executable, 0x8035d9ac)?,
        },
        manual: Manual {
            title: texts.reference(executable, word(dol::slice(executable, 0x8019d6f0, 4)?, 0)?)?,
            chapters,
        },
        texts: texts.values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    #[ignore = "requires both extracted discs; only publishes JSON tables"]
    fn original_synopsis_manual_preserves_all_records_and_unselected_topics() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let catalogue = read(&executable)?;
                let destination = output.join(format!("disc{disc}"));
                let paths = crate::embedded::write(&file, &destination, FAMILY, &catalogue)?;
                let restored: Catalogue =
                    serde_json::from_slice(&fs::read(destination.join(&paths[0]))?)?;
                assert_eq!(restored, catalogue);
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["data"], paths[0]);
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                payloads.insert(paths[0].clone());

                assert_eq!(restored.entries.len(), SYNOPSIS_COUNT);
                assert_eq!(
                    restored
                        .manual
                        .chapters
                        .iter()
                        .map(|c| c.topic_count)
                        .sum::<u8>(),
                    30
                );
                let topics: Vec<_> = restored
                    .manual
                    .chapters
                    .iter()
                    .flat_map(|chapter| &chapter.topics)
                    .collect();
                assert_eq!(topics.len(), MANUAL_CHAPTERS * TOPICS_PER_CHAPTER);
                assert_eq!(
                    restored.manual.chapters[2].name,
                    restored.manual.chapters[2].topics[0].name
                );
                assert_eq!(
                    restored.manual.chapters[3].name,
                    restored.manual.chapters[3].topics[0].name
                );
                let hidden = &restored.manual.chapters[8].topics[2];
                assert_eq!(restored.manual.chapters[8].topic_count, 2);
                assert_eq!(restored.required(hidden.name)?, "Practice Battle");
                assert_eq!(hidden.learned_flag, 0);
                assert_eq!(
                    hidden
                        .paragraphs
                        .as_ref()
                        .unwrap()
                        .iter()
                        .map(|&r| restored.text(r))
                        .collect::<Vec<_>>(),
                    ["*"]
                );
                assert!(
                    topics
                        .iter()
                        .any(|t| t.name.is_none() && t.paragraphs.is_none())
                );
                assert!(restored.texts.iter().any(|text| text.contains("\u{b}\0")));

                // Physical recovery retains unused and out-of-range values. Only
                // projection into the playable manual admits selectable counts.
                for (address, bytes) in [
                    (ENTRIES, vec![0xab, 0xcd, 0xff, 0xff]),
                    (COUNTS, vec![255]),
                    (FLAGS + 42, vec![255]),
                    (TOPICS + 42 * 8, vec![0; 8]),
                ] {
                    let source = dol::slice(&executable, address, bytes.len())?;
                    let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                    executable[offset..offset + bytes.len()].copy_from_slice(&bytes);
                }
                let changed = read(&executable)?;
                assert_eq!(changed.entries[0].storage, 0xabcd);
                assert_eq!(changed.entries[0].location, 0xffff);
                assert_eq!(changed.manual.chapters[0].topic_count, 255);
                let hidden = &changed.manual.chapters[8].topics[2];
                assert_eq!(hidden.learned_flag, 255);
                assert!(hidden.name.is_none() && hidden.paragraphs.is_none());
            }
            assert_eq!(payloads.len(), 1, "both discs share one semantic payload");
            Ok(())
        })();
        let _ = fs::remove_dir_all(output);
        result
    }
}
