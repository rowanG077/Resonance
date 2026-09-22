//! Complete title records and their character ranges, independent of menu selection.
use super::text::{TextPool, TextRef};
use crate::{
    dol,
    read::{Field, u32 as word},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
const STARTS: u32 = 0x80210920;
const ENTRIES: u32 = 0x80210934;
const COSTUME_TITLES: u32 = 0x80199d84;
const COSTUME_VARIANTS: u32 = 0x8035c938;
const COUNT: usize = 159;
const STRIDE: usize = 16;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub(crate) name: Option<TextRef>,
    pub(crate) description: Option<TextRef>,
    /// Unsigned additions to HP, TP, strength, defense, intelligence, evasion and accuracy.
    pub(crate) growth: [u8; 7],
    /// Final authored byte; its meaning is not established by the known title readers.
    pub(crate) storage: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct CostumeBindings {
    /// Three costume rows, each indexed by playable character. -1 means no title.
    pub(crate) titles: [[i16; 9]; 3],
    /// Raw costume selectors; playable preparation admits the supported variants.
    pub(crate) variants: [u8; 3],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) character_starts: [u16; 9],
    pub(crate) entries: Vec<Entry>,
    pub(crate) costumes: CostumeBindings,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }

    pub(crate) fn required_text(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required title text")?))
    }

    /// One-based playable character ID, matching the native title lookup.
    pub(crate) fn for_character(&self, character: u8) -> Result<&[Entry]> {
        let index = usize::from(
            character
                .checked_sub(1)
                .context("zero title character ID")?,
        );
        let start = usize::from(
            *self
                .character_starts
                .get(index)
                .context("unknown title character")?,
        );
        let end = self
            .character_starts
            .get(index + 1)
            .map_or(self.entries.len(), |&end| usize::from(end));
        self.entries
            .get(start..end)
            .context("title range outside catalogue")
    }
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let starts = dol::slice(executable, STARTS, 9 * 2)?;
    let rows = dol::slice(executable, ENTRIES, COUNT * STRIDE)?;
    let costume_titles = dol::slice(executable, COSTUME_TITLES, 3 * 9 * 2)?;
    let variants = dol::slice(executable, COSTUME_VARIANTS, 3)?;
    let mut texts = TextPool::default();
    let entries = rows
        .chunks_exact(STRIDE)
        .map(|row| {
            Ok(Entry {
                name: texts.reference(executable, word(row, 0)?)?,
                description: texts.reference(executable, word(row, 4)?)?,
                growth: row[8..15].try_into()?,
                storage: row[15],
            })
        })
        .collect::<Result<_>>()?;
    Ok(Catalogue {
        texts: texts.values,
        character_starts: Field::read(starts, 0)?,
        entries,
        costumes: CostumeBindings {
            titles: Field::read(costume_titles, 0)?,
            variants: variants.try_into()?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs, path::Path};

    const FAMILY: &str = "title-catalogue";

    #[test]
    #[ignore = "requires both extracted discs; publishes only title JSON"]
    fn original_title_catalogue_preserves_all_records_and_ranges() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let destination = output.join(format!("disc{disc}"));
                let catalogue = read(&executable)?;
                let paths = crate::embedded::write(&file, &destination, FAMILY, &catalogue)?;
                let restored: Catalogue = crate::embedded::read(&destination, FAMILY, "main.dol")?;
                assert_eq!(restored, catalogue);
                payloads.insert(paths[0].clone());
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                assert_eq!(
                    restored.character_starts,
                    [0, 28, 49, 69, 86, 103, 119, 134, 149]
                );
                assert_eq!(restored.entries[68].storage, 3);
                assert_eq!(
                    restored.required_text(restored.entries[68].name)?,
                    "I Hate Gels!"
                );
                let counts = (1..=9)
                    .map(|id| Ok(restored.for_character(id)?.len()))
                    .collect::<Result<Vec<_>>>()?;
                assert_eq!(counts, [28, 21, 20, 17, 17, 16, 15, 15, 10]);
                assert!(restored.for_character(0).is_err() && restored.for_character(10).is_err());
                assert_eq!(
                    restored.costumes.titles,
                    [
                        [5; 9],
                        [6, 6, 6, 6, 6, 6, 6, 6, -1],
                        [7, 7, 7, 7, 7, 7, 7, 7, -1],
                    ]
                );
                assert_eq!(restored.costumes.variants, [1, 2, 4]);
                for (id, name) in [(2, "Mature Kid"), (6, "Dream Traveler")] {
                    assert_eq!(
                        restored.required_text(restored.for_character(7)?[id - 1].name)?,
                        name
                    );
                }

                let alias = word(dol::slice(&executable, ENTRIES + STRIDE as u32, 4)?, 0)?;
                for (address, replacement) in [
                    (STARTS, 1u16.to_be_bytes().to_vec()),
                    (ENTRIES, 0u32.to_be_bytes().to_vec()),
                    (ENTRIES + 4, alias.to_be_bytes().to_vec()),
                    (ENTRIES + 8, vec![255, 128, 127, 0, 1, 2, 3, 0xa5]),
                    (COSTUME_TITLES, (-2i16).to_be_bytes().to_vec()),
                    (COSTUME_TITLES + 16, 0i16.to_be_bytes().to_vec()),
                    (COSTUME_VARIANTS, vec![255, 3, 0]),
                ] {
                    let source = dol::slice(&executable, address, replacement.len())?;
                    let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                    executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
                }
                let changed = read(&executable)?;
                assert_eq!(changed.entries.len(), COUNT);
                assert_eq!(changed.for_character(1)?.first(), changed.entries.get(1));
                assert!(changed.entries[0].name.is_none());
                assert!(changed.required_text(changed.entries[0].name).is_err());
                assert_eq!(changed.entries[0].description, changed.entries[1].name);
                assert_eq!(changed.entries[0].growth, [255, 128, 127, 0, 1, 2, 3]);
                assert_eq!(changed.entries[0].storage, 0xa5);
                assert_eq!(changed.costumes.titles[0][0], -2);
                assert_eq!(changed.costumes.titles[0][8], 0);
                assert_eq!(changed.costumes.variants, [255, 3, 0]);
            }
            assert_eq!(payloads.len(), 1);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
