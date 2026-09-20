//! Authored casting voice overrides, independent of selected techniques or audio conversion.
use super::actions::{Rel, member};
use super::embedded::{self, Layout, PARTY_COUNT, SETTINGS_BYTES};
use crate::cooked::Source;
use crate::read::{u16 as half, u32 as word};
use anyhow::{Context, Result, ensure};
use resonance_content::battle::{actions::CastingVoices, arte_inventory::CastingVoiceBinding};
use serde::{Deserialize, Serialize};
use std::path::Path;

const ROW_BYTES: usize = 6;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    native_id: u16,
    /// An absent override selects the character's default casting voice.
    begin: Option<u16>,
    release: Option<u16>,
}

#[derive(Serialize, Deserialize)]
struct Character {
    character: u8,
    voice_base: u32,
    entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct Tables {
    characters: Vec<Character>,
    trailing_storage: Vec<u8>,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
pub(super) struct VoiceDurations {
    pub(super) duration_ticks: Vec<u16>,
}

impl VoiceDurations {
    pub fn bind(source: &Source<'_>) -> Result<Self> {
        let (_, bytes) = source.resolve("battle/all/usual/12.json")?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn get(&self, voice: u16) -> Result<u16> {
        self.duration_ticks
            .get(usize::from(voice & 0x7fff))
            .copied()
            .context("missing cooked voice duration")
    }

    pub fn remaining(&self, voice: u16) -> Result<u16> {
        self.get(voice)?
            .checked_add(20)
            .context("casting voice duration overflow")
    }

    fn read(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len().is_multiple_of(2),
            "misaligned voice duration table"
        );
        Ok(Self {
            duration_ticks: bytes
                .chunks_exact(2)
                .map(|row| half(row, 0))
                .collect::<Result<_>>()?,
        })
    }

    #[cfg(test)]
    pub fn original(usual: &[u8]) -> Result<Self> {
        Self::read(member(usual, 12)?)
    }
}

pub(super) struct CastingTables {
    characters: Vec<Character>,
    pub durations: VoiceDurations,
}

impl Character {
    fn default_begin(&self) -> Result<u16> {
        Ok(u16::try_from(
            self.voice_base
                .checked_add(7)
                .context("casting voice base overflow")?,
        )? | 0x8000)
    }
}

impl CastingTables {
    pub fn bind(module: &Source<'_>, usual: &Source<'_>) -> Result<Self> {
        let tables: Tables = module.embedded("battle-casting-voices", "US_r_Top2Btl.rel")?;
        ensure!(
            tables.characters.len() == usize::from(PARTY_COUNT)
                && tables
                    .characters
                    .iter()
                    .enumerate()
                    .all(|(index, row)| usize::from(row.character) == index + 1),
            "invalid casting voice owners"
        );
        Ok(Self {
            characters: tables.characters,
            durations: VoiceDurations::bind(usual)?,
        })
    }

    #[cfg(test)]
    pub fn original(rel: &Rel, usual: &[u8]) -> Result<Self> {
        Ok(Self {
            characters: (1..=PARTY_COUNT)
                .map(|character| {
                    read(
                        rel,
                        Layout::RETAIL.casting_voices,
                        Layout::RETAIL.party_settings,
                        character,
                    )
                    .map(|(data, _)| data)
                })
                .collect::<Result<_>>()?,
            durations: VoiceDurations::original(usual)?,
        })
    }

    fn character(&self, character: u8) -> Result<&Character> {
        self.characters
            .iter()
            .find(|row| row.character == character)
            .context("missing casting voice owner")
    }

    pub fn selected(&self, character: u8, native: u16) -> Result<CastingVoices> {
        let character = self.character(character)?;
        // A later authored row overrides an earlier match.
        let entry = character
            .entries
            .iter()
            .rev()
            .find(|entry| entry.native_id == native)
            .with_context(|| {
                format!(
                    "missing casting voice binding for character {}, arte {native}",
                    character.character
                )
            })?;
        let begin = match entry.begin {
            Some(voice) => voice,
            None => character.default_begin()?,
        };
        Ok(CastingVoices {
            begin,
            begin_remaining: self.remaining(begin)?,
            release: entry.release.context("incomplete casting voice binding")?,
        })
    }

    pub fn remaining(&self, voice: u16) -> Result<u16> {
        self.durations.remaining(voice)
    }

    pub fn default_begin(&self, character: u8) -> Result<Option<u16>> {
        let character = self.character(character)?;
        (character.voice_base != 0)
            .then(|| character.default_begin())
            .transpose()
    }

    pub fn self_voices(&self, character: u8, mut voices: CastingVoices) -> Result<CastingVoices> {
        if character != 2
            && let Some(begin) = self.default_begin(character)?
        {
            voices.begin = begin;
            voices.begin_remaining = self.remaining(begin)?;
        }
        Ok(voices)
    }
}

#[derive(Clone, Copy, Serialize)]
struct Address {
    section: usize,
    offset: usize,
}

#[derive(Serialize)]
struct ListSource {
    character: u8,
    voice_base: Address,
    pointer: Address,
    target: Option<Address>,
    /// Includes the terminating row, but excludes alignment between lists.
    bytes: usize,
}

fn parse(bytes: &[u8]) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for row in bytes.chunks_exact(ROW_BYTES) {
        let native_id = half(row, 0)?;
        if native_id == 0 {
            ensure!(row == [0; ROW_BYTES], "nonzero casting voice terminator");
            return Ok(entries);
        }
        let voice = |at| half(row, at).map(|id| (id != 0).then_some(id));
        entries.push(Entry {
            native_id,
            begin: voice(2)?,
            release: voice(4)?,
        });
    }
    anyhow::bail!("unterminated casting voice list")
}

fn read(rel: &Rel, table: usize, records: usize, character: u8) -> Result<(Character, ListSource)> {
    ensure!(
        (1..=PARTY_COUNT).contains(&character),
        "invalid casting voice owner"
    );
    let pointer = Address {
        section: 5,
        offset: table + usize::from(character - 1) * 4,
    };
    let voice_base = Address {
        section: 5,
        offset: records + usize::from(character - 1) * SETTINGS_BYTES + 0x104,
    };
    let target = match rel.pointer(pointer.section, pointer.offset) {
        Ok((section, offset)) => {
            ensure!(
                section == 5,
                "casting voices point outside the data section"
            );
            Some(Address { section, offset })
        }
        Err(_) if word(rel.at((pointer.section, pointer.offset))?, 0)? == 0 => None,
        Err(error) => return Err(error),
    };
    let entries = target
        .map(|at| parse(rel.at((at.section, at.offset))?))
        .transpose()?
        .unwrap_or_default();
    let bytes = if target.is_some() {
        (entries.len() + 1) * ROW_BYTES
    } else {
        0
    };
    Ok((
        Character {
            character,
            voice_base: word(rel.at((voice_base.section, voice_base.offset))?, 0)?,
            entries,
        },
        ListSource {
            character,
            voice_base,
            pointer,
            target,
            bytes,
        },
    ))
}

pub(super) fn inventory(
    rel: &Rel,
    usual: &[u8],
    character: u8,
) -> Result<Vec<CastingVoiceBinding>> {
    let (data, _) = read(
        rel,
        Layout::RETAIL.casting_voices,
        Layout::RETAIL.party_settings,
        character,
    )?;
    if data.entries.is_empty() {
        return Ok(Vec::new());
    }
    let default_begin = data.default_begin()?;
    let durations = VoiceDurations::read(member(usual, 12)?)?;
    data.entries
        .into_iter()
        .map(|entry| {
            let begin = entry.begin.unwrap_or(default_begin);
            Ok(CastingVoiceBinding {
                native_id: entry.native_id,
                begin,
                release: entry.release,
                begin_remaining_ticks: durations.remaining(begin)?,
            })
        })
        .collect()
}

fn trailing_storage(rel: &Rel, start: usize, end: usize) -> Result<Vec<u8>> {
    let bytes = end
        .checked_sub(start)
        .context("reversed casting voice table")?;
    let tail = usize::from(PARTY_COUNT) * 4;
    let storage = rel
        .at((5, start))?
        .get(tail..bytes)
        .context("truncated casting voice pointer table")?;
    ensure!(
        rel.pointers
            .range((5, start + tail)..(5, end))
            .next()
            .is_none(),
        "additional casting voice pointer requires a character binding"
    );
    Ok(storage.to_vec())
}

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    let rel = Rel::read(file)?;
    let trailing_storage =
        trailing_storage(&rel, layout.casting_voices, layout.casting_voices_end)?;
    let mut characters = Vec::new();
    let mut lists = Vec::new();
    for character in 1..=PARTY_COUNT {
        let (data, source) = read(
            &rel,
            layout.casting_voices,
            layout.party_settings,
            character,
        )?;
        characters.push(data);
        lists.push(source);
    }
    embedded::write(
        file,
        output,
        "battle-casting-voices",
        &Tables { characters, trailing_storage },
        serde_json::json!({
            "table": {"section": 5, "offset": layout.casting_voices, "end": layout.casting_voices_end},
            "party_records": Address { section: 5, offset: layout.party_settings }, "lists": lists,
        }),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    fn casting_voice_lists_require_a_complete_terminator_and_keep_the_last_override() -> Result<()>
    {
        assert!(parse(&[0, 200, 0, 0, 0x81, 0x2c]).is_err());
        assert!(parse(&[0, 0, 0, 1, 0, 0]).is_err());
        assert!(parse(&[0, 0]).is_err());
        let mut tables = CastingTables {
            characters: (1..=2)
                .map(|character| {
                    Ok(Character {
                        character,
                        voice_base: 1,
                        entries: parse(&[
                            0, 200, 0, 9, 0, 10, 0, 200, 0, 0, 0, 12, 0, 0, 0, 0, 0, 0,
                        ])?,
                    })
                })
                .collect::<Result<_>>()?,
            durations: VoiceDurations {
                duration_ticks: vec![30; 16],
            },
        };
        let voices = tables.selected(1, 200)?;
        assert_eq!(
            (voices.begin, voices.begin_remaining, voices.release),
            (0x8008, 50, 12)
        );
        assert_eq!(tables.remaining(8)?, tables.remaining(0x8008)?);
        let other = CastingVoices {
            begin: 9,
            begin_remaining: 77,
            release: 12,
        };
        assert_eq!(tables.self_voices(1, other)?.begin, 0x8008);
        assert_eq!(tables.self_voices(2, other)?.begin, 9);
        tables.characters[0].voice_base = 0;
        assert_eq!(tables.self_voices(1, other)?.begin, 9);
        assert!(tables.selected(1, 201).is_err());
        tables.characters[0].entries.last_mut().unwrap().release = None;
        assert!(tables.selected(1, 200).is_err());
        Ok(())
    }

    #[test]
    fn pointer_tail_retains_storage_but_rejects_additional_bindings() -> Result<()> {
        let mut rel = Rel {
            bytes: [vec![0; 37], vec![1, 2, 3, 4]].concat(),
            sections: vec![(1, 40); 6],
            pointers: Default::default(),
            local_targets: Default::default(),
        };
        assert_eq!(trailing_storage(&rel, 0, 40)?, [1, 2, 3, 4]);
        assert!(trailing_storage(&rel, 0, 35).is_err());
        assert!(trailing_storage(&rel, 0, 41).is_err());
        assert!(trailing_storage(&rel, 40, 0).is_err());
        rel.pointers.insert((5, 36), (5, 0));
        assert!(trailing_storage(&rel, 0, 40).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires original battle modules; no media conversion"]
    fn original_casting_voice_tables_cover_and_deduplicate_every_module() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("casting-voice-tables"));
        let mut data_paths = BTreeSet::new();
        for (disc, namespace) in [(1, "disc1"), (2, "disc2")] {
            let files = extracted.join(namespace).join("files");
            let active = Rel::read(&files.join("US_r_Top2Btl.rel"))?;
            let usual = fs::read(files.join("BTL/BTLusual.dat"))?;
            let published = extracted.parent().unwrap().join("all-assets");
            let bound = CastingTables::bind(
                &Source::open(&published, disc, "US_r_Top2Btl.rel")?,
                &Source::open(&published, disc, "BTL/BTLusual.dat")?,
            )?;
            assert_eq!(bound.durations, VoiceDurations::original(&usual)?);
            let bitmap = member(&usual, 11)?;
            assert_eq!(bitmap.len() * 8, 2560);
            assert_eq!(
                bitmap.iter().map(|byte| byte.count_ones()).sum::<u32>(),
                1378
            );
            let durations = member(&usual, 12)?;
            assert_eq!(durations.len(), 2512 * 2);
            assert_eq!(
                durations
                    .chunks_exact(2)
                    .filter(|row| *row == [0, 0])
                    .count(),
                706
            );
            for (id, row) in durations.chunks_exact(2).enumerate() {
                assert_eq!(bound.remaining(id as u16 | 0x8000)?, half(row, 0)? + 20);
            }
            for module in [
                "US_r_Top2Btl.rel",
                "r_Top2Btl.rel",
                "US_Top2Btl.rel",
                "US_m_Top2Btl.rel",
                "Top2Btl.rel",
                "m_Top2Btl.rel",
                "Top2BtlD.rel",
            ] {
                let paths = cook(&files.join(module), &output)?.unwrap();
                let data: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[0]))?)?;
                let characters = &data["characters"];
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(provenance["module"], module);
                assert_eq!(provenance["data"], paths[0]);
                let (table, records) = match module {
                    "US_r_Top2Btl.rel" | "r_Top2Btl.rel" => (0x59d8, 0x3d30),
                    "Top2BtlD.rel" => (0x6350, 0x42f0),
                    _ => (0x6838, 0x4b80),
                };
                assert_eq!(
                    provenance["table"],
                    serde_json::json!({ "section": 5, "offset": table, "end": table + 40 })
                );
                assert_eq!(
                    provenance["party_records"],
                    serde_json::json!({ "section": 5, "offset": records })
                );
                assert_eq!(
                    characters.as_array().unwrap().len(),
                    usize::from(PARTY_COUNT)
                );
                let rel = Rel::read(&files.join(module))?;
                for root in [table, table + 40] {
                    assert!(rel.local_targets().contains(&(5, root)));
                }
                assert_eq!(
                    data["trailing_storage"],
                    serde_json::json!(&rel.at((5, table))?[36..40])
                );
                assert_eq!(data["trailing_storage"], serde_json::json!([0, 0, 0, 0]));
                let bases = [1, 121, 241, 362, 469, 587, 699, 807, 909];
                for (index, count) in [0, 7, 36, 26, 14, 12, 0, 3, 12].into_iter().enumerate() {
                    assert_eq!(characters[index]["character"], index + 1);
                    assert_eq!(characters[index]["voice_base"], bases[index]);
                    assert_eq!(
                        provenance["lists"][index]["voice_base"],
                        serde_json::json!({ "section": 5, "offset": records + index * 496 + 0x104 })
                    );
                    assert_eq!(
                        characters[index]["entries"].as_array().unwrap().len(),
                        count
                    );
                    assert_eq!(provenance["lists"][index]["target"].is_null(), count == 0);
                    assert_eq!(
                        provenance["lists"][index]["pointer"],
                        serde_json::json!({ "section": 5, "offset": table + index * 4 })
                    );
                    assert_eq!(
                        provenance["lists"][index]["bytes"],
                        if count == 0 {
                            0
                        } else {
                            (count + 1) * ROW_BYTES
                        }
                    );
                }
                assert_eq!(
                    characters
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|row| row["entries"].as_array().unwrap().len())
                        .sum::<usize>(),
                    110
                );
                assert_eq!(
                    characters[2]["entries"][0],
                    serde_json::json!({ "native_id": 200, "begin": null, "release": 33068 })
                );
                assert_eq!(characters[4]["entries"][2]["begin"], 574);
                assert_eq!(characters[8]["entries"][1]["begin"], 1019);
                data_paths.insert(paths[0].clone());
            }
            for character in 1..=PARTY_COUNT {
                for entry in inventory(&active, &usual, character)? {
                    let selected = bound.selected(character, entry.native_id)?;
                    assert_eq!(selected.begin, entry.begin);
                    assert_eq!(Some(selected.release), entry.release);
                    assert_eq!(selected.begin_remaining, entry.begin_remaining_ticks);
                    let base = word(
                        active.at((
                            5,
                            Layout::RETAIL.party_settings
                                + usize::from(character - 1) * SETTINGS_BYTES,
                        ))?,
                        0x104,
                    )?;
                    let self_voices = bound.self_voices(character, selected)?;
                    let expected = if character != 2 && base != 0 {
                        (base + 7) as u16 | 0x8000
                    } else {
                        selected.begin
                    };
                    assert_eq!(self_voices.begin, expected);
                    assert_eq!(
                        self_voices.begin_remaining,
                        half(durations, usize::from(expected & 0x7fff) * 2)? + 20
                    );
                    assert_eq!(self_voices.release, selected.release);
                }
            }
        }
        assert_eq!(data_paths.len(), 1);
        fs::remove_dir_all(output)?;
        Ok(())
    }
}
