//! Complete synopsis records and the training manual's fixed topic grid.
use super::text::{TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result};
use resonance_content::menu_data::{MANUAL_CHAPTERS, SYNOPSIS_COUNT};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

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
    pub(crate) count_storage: [u8; 3],
    pub(crate) flag_storage: [u8; 3],
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
    start: u32,
    bodies: &mut BTreeMap<u32, u32>,
) -> Result<Option<Vec<TextRef>>> {
    if start == 0 {
        return Ok(None);
    }
    let mut address = start;
    let mut paragraphs = Vec::new();
    loop {
        let reference = texts.required(executable, address)?;
        let source = &texts.sources[reference.0];
        address = source
            .address
            .checked_add(source.source_size)
            .context("manual text address overflow")?;
        paragraphs.push(reference);
        if dol::slice(executable, address, 1)? == [0] {
            bodies.insert(
                start,
                address
                    .checked_add(1)
                    .context("manual text address overflow")?
                    - start,
            );
            return Ok(Some(paragraphs));
        }
    }
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>, Vec<TextSource>)> {
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
    let counts = dol::slice(executable, COUNTS, 12)?;
    let flags = dol::slice(executable, FLAGS, 48)?;
    let topics = dol::slice(executable, TOPICS, MANUAL_CHAPTERS * TOPICS_PER_CHAPTER * 8)?;
    let mut bodies = BTreeMap::new();
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
                        paragraphs: paragraphs(&mut texts, executable, word(row, 4)?, &mut bodies)?,
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
    Ok((
        Catalogue {
            entries,
            headings: texts.array(executable, 0x8035d8f8)?,
            months: texts.array(executable, 0x801df814)?,
            formats: Formats {
                date: texts.required(executable, 0x801df844)?,
                level: texts.required(executable, 0x8035d9a4)?,
                position: texts.required(executable, 0x8035d9ac)?,
            },
            manual: Manual {
                title: texts
                    .reference(executable, word(dol::slice(executable, 0x8019d6f0, 4)?, 0)?)?,
                chapters,
                count_storage: counts[MANUAL_CHAPTERS..].try_into()?,
                flag_storage: flags[MANUAL_CHAPTERS * TOPICS_PER_CHAPTER..].try_into()?,
            },
            texts: texts.values,
        },
        texts.sources,
        bodies
            .into_iter()
            .map(|(address, source_size)| TextSource {
                address,
                source_size,
            })
            .collect(),
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, texts, bodies) = parse(executable)?;
    crate::embedded::write(
        file,
        output,
        FAMILY,
        &catalogue,
        serde_json::json!({
            "synopsis":{"address":ENTRIES,"count":200,"stride":24,"source_size":4800},
            "headings":{"address":0x8035d8f8u32,"count":2,"stride":4,"source_size":8},
            "months":{"address":0x801df814u32,"count":12,"stride":4,"source_size":48},
            "manual_title":{"address":0x8019d6f0u32,"source_size":4},
            "manual_chapters":{"address":CHAPTERS,"count":9,"stride":4,"source_size":36},
            "manual_counts":{"address":COUNTS,"count":9,"storage":3,"source_size":12},
            "manual_flags":{"address":FLAGS,"count":45,"storage":3,"source_size":48},
            "manual_topics":{"address":TOPICS,"count":45,"stride":8,"source_size":360},
            "manual_bodies":bodies,
            "texts":texts,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    fn original_text(text: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {
            if matches!(ch, '\u{b}' | '\u{c}') {
                bytes.extend([
                    ch as u8,
                    u8::try_from(chars.next().unwrap() as u32).unwrap(),
                ]);
            } else {
                let text = ch.to_string();
                let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(&text);
                assert!(!invalid);
                bytes.extend(encoded.as_ref());
            }
        }
        bytes.push(0);
        bytes
    }

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
                let (catalogue, sources, bodies) = parse(&executable)?;
                let destination = output.join(format!("disc{disc}"));
                let paths = cook(&file, &executable, &destination)?;
                let restored: Catalogue =
                    serde_json::from_slice(&fs::read(destination.join(&paths[0]))?)?;
                assert_eq!(restored, catalogue);
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["data"], paths[0]);
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                payloads.insert(paths[0].clone());

                let pointer = |reference: Option<TextRef>| {
                    reference.map_or(0, |r| sources[r.0].address).to_be_bytes()
                };
                let entries: Vec<_> = restored
                    .entries
                    .iter()
                    .flat_map(|row| {
                        row.storage
                            .to_be_bytes()
                            .into_iter()
                            .chain(row.location.to_be_bytes())
                            .chain(
                                [row.heading, row.title]
                                    .into_iter()
                                    .chain(row.text)
                                    .flat_map(pointer),
                            )
                    })
                    .collect();
                assert_eq!(
                    entries,
                    dol::slice(&executable, ENTRIES, SYNOPSIS_COUNT * 24)?
                );
                for (address, references) in [
                    (0x8035d8f8, restored.headings.to_vec()),
                    (0x801df814, restored.months.to_vec()),
                    (0x8019d6f0, vec![restored.manual.title]),
                    (
                        CHAPTERS,
                        restored.manual.chapters.iter().map(|c| c.name).collect(),
                    ),
                ] {
                    let bytes: Vec<_> = references.iter().copied().flat_map(pointer).collect();
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, references.len() * 4)?
                    );
                }
                let counts: Vec<_> = restored
                    .manual
                    .chapters
                    .iter()
                    .map(|c| c.topic_count)
                    .chain(restored.manual.count_storage)
                    .collect();
                assert_eq!(counts, dol::slice(&executable, COUNTS, 12)?);
                let topics: Vec<_> = restored
                    .manual
                    .chapters
                    .iter()
                    .flat_map(|c| &c.topics)
                    .collect();
                assert_eq!(topics.len(), 45);
                let flags: Vec<_> = topics
                    .iter()
                    .map(|t| t.learned_flag)
                    .chain(restored.manual.flag_storage)
                    .collect();
                assert_eq!(flags, dol::slice(&executable, FLAGS, 48)?);
                let bindings: Vec<_> = topics
                    .iter()
                    .flat_map(|t| {
                        pointer(t.name)
                            .into_iter()
                            .chain(pointer(t.paragraphs.as_ref().map(|p| p[0])))
                    })
                    .collect();
                assert_eq!(bindings, dol::slice(&executable, TOPICS, 45 * 8)?);
                for (source, text) in sources.iter().zip(&restored.texts) {
                    assert_eq!(
                        original_text(text),
                        dol::slice(&executable, source.address, source.source_size as usize)?
                    );
                }
                for topic in &topics {
                    if let Some(paragraphs) = &topic.paragraphs {
                        let start = sources[paragraphs[0].0].address;
                        let source = bodies.iter().find(|b| b.address == start).unwrap();
                        let bytes: Vec<_> = paragraphs
                            .iter()
                            .flat_map(|&r| original_text(restored.text(r)))
                            .chain([0])
                            .collect();
                        assert_eq!(
                            bytes,
                            dol::slice(&executable, start, source.source_size as usize)?
                        );
                    }
                }
                for (reference, address) in [
                    (restored.formats.date, 0x801df844),
                    (restored.formats.level, 0x8035d9a4),
                    (restored.formats.position, 0x8035d9ac),
                ] {
                    assert_eq!(sources[reference.0].address, address);
                }
                assert_eq!(counts[..MANUAL_CHAPTERS].iter().copied().sum::<u8>(), 30);
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
                    (COUNTS + 9, vec![71, 72, 73]),
                    (FLAGS + 42, vec![255]),
                    (FLAGS + 45, vec![81, 82, 83]),
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
                assert_eq!(changed.manual.count_storage, [71, 72, 73]);
                assert_eq!(changed.manual.flag_storage, [81, 82, 83]);
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
