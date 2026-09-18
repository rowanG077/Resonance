//! Complete title records and their character ranges, independent of menu selection.
use super::text::{TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{Field, u16 as half, u32 as word},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
const FAMILY: &str = "title-catalogue";
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
    /// Remaining halfword and byte after their respective consumed tables.
    pub(crate) title_storage: i16,
    pub(crate) variant_storage: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) character_starts: [u16; 9],
    /// Remaining halfword after the nine playable-character starts.
    pub(crate) character_start_storage: u16,
    pub(crate) entries: Vec<Entry>,
    pub(crate) costumes: CostumeBindings,
    /// Four authored bytes after the final title record; meaning unresolved.
    pub(crate) storage: u32,
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

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let starts = dol::slice(executable, STARTS, 20)?;
    let rows = dol::slice(executable, ENTRIES, COUNT * STRIDE + 4)?;
    let costume_titles = dol::slice(executable, COSTUME_TITLES, 56)?;
    let variants = dol::slice(executable, COSTUME_VARIANTS, 4)?;
    let mut texts = TextPool::default();
    let entries = rows[..COUNT * STRIDE]
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
    Ok((
        Catalogue {
            texts: texts.values,
            character_starts: Field::read(starts, 0)?,
            character_start_storage: half(starts, 18)?,
            entries,
            costumes: CostumeBindings {
                titles: Field::read(costume_titles, 0)?,
                variants: variants[..3].try_into()?,
                title_storage: i16::from_be_bytes(costume_titles[54..].try_into()?),
                variant_storage: variants[3],
            },
            storage: word(rows, COUNT * STRIDE)?,
        },
        texts.sources,
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

#[cfg(test)]
pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, _) = parse(executable)?;
    crate::embedded::write(file, output, FAMILY, &catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    fn reconstruct(catalogue: &Catalogue, sources: &[TextSource]) -> [(u32, Vec<u8>); 4] {
        let starts = catalogue
            .character_starts
            .iter()
            .copied()
            .chain([catalogue.character_start_storage])
            .flat_map(u16::to_be_bytes)
            .collect();
        let mut rows = Vec::new();
        for entry in &catalogue.entries {
            for reference in [entry.name, entry.description] {
                rows.extend(
                    reference
                        .map_or(0, |id| sources[id.0].address)
                        .to_be_bytes(),
                );
            }
            rows.extend(entry.growth);
            rows.push(entry.storage);
        }
        rows.extend(catalogue.storage.to_be_bytes());
        let costume_titles = catalogue
            .costumes
            .titles
            .iter()
            .flatten()
            .copied()
            .chain([catalogue.costumes.title_storage])
            .flat_map(i16::to_be_bytes)
            .collect();
        let mut variants = catalogue.costumes.variants.to_vec();
        variants.push(catalogue.costumes.variant_storage);
        [
            (STARTS, starts),
            (ENTRIES, rows),
            (COSTUME_TITLES, costume_titles),
            (COSTUME_VARIANTS, variants),
        ]
    }

    #[test]
    #[ignore = "requires both extracted discs; publishes only title JSON"]
    fn original_title_catalogue_preserves_all_records_ranges_and_storage() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let destination = output.join(format!("disc{disc}"));
                let (catalogue, sources) = parse(&executable)?;
                let paths = cook(&file, &executable, &destination)?;
                let restored: Catalogue = crate::embedded::read(&destination, FAMILY, "main.dol")?;
                assert_eq!(restored, catalogue);
                payloads.insert(paths[0].clone());
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                for (address, bytes) in reconstruct(&restored, &sources) {
                    assert_eq!(bytes, dol::slice(&executable, address, bytes.len())?);
                }
                for (source, text) in sources.iter().zip(&restored.texts) {
                    let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(text);
                    assert!(!invalid);
                    let bytes = [encoded.as_ref(), &[0]].concat();
                    assert_eq!(bytes.len() as u32, source.source_size);
                    assert_eq!(bytes, dol::slice(&executable, source.address, bytes.len())?);
                }
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

                // Make the first row inactive, without losing its null, alias or high-bit bytes.
                let alias = sources[restored.entries[1]
                    .name
                    .context("missing second title name")?
                    .0]
                    .address;
                for (address, replacement) in [
                    (STARTS, 1u16.to_be_bytes().to_vec()),
                    (STARTS + 18, 0xbeefu16.to_be_bytes().to_vec()),
                    (ENTRIES, 0u32.to_be_bytes().to_vec()),
                    (ENTRIES + 4, alias.to_be_bytes().to_vec()),
                    (ENTRIES + 8, vec![255, 128, 127, 0, 1, 2, 3, 0xa5]),
                    (
                        ENTRIES + (COUNT * STRIDE) as u32,
                        0x12345678u32.to_be_bytes().to_vec(),
                    ),
                    (COSTUME_TITLES, (-2i16).to_be_bytes().to_vec()),
                    (COSTUME_TITLES + 16, 0i16.to_be_bytes().to_vec()),
                    (COSTUME_TITLES + 54, 0xbeefu16.to_be_bytes().to_vec()),
                    (COSTUME_VARIANTS, vec![255, 3, 0, 0xa5]),
                ] {
                    let source = dol::slice(&executable, address, replacement.len())?;
                    let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                    executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
                }
                let (changed, sources) = parse(&executable)?;
                assert_eq!(changed.entries.len(), COUNT);
                assert_eq!(changed.for_character(1)?.first(), changed.entries.get(1));
                assert!(changed.entries[0].name.is_none());
                assert!(changed.required_text(changed.entries[0].name).is_err());
                assert_eq!(changed.entries[0].description, changed.entries[1].name);
                assert_eq!(changed.entries[0].growth, [255, 128, 127, 0, 1, 2, 3]);
                assert_eq!(changed.entries[0].storage, 0xa5);
                assert_eq!(changed.character_start_storage, 0xbeef);
                assert_eq!(changed.storage, 0x12345678);
                assert_eq!(changed.costumes.titles[0][0], -2);
                assert_eq!(changed.costumes.titles[0][8], 0);
                assert_eq!(changed.costumes.title_storage as u16, 0xbeef);
                assert_eq!(changed.costumes.variants, [255, 3, 0]);
                assert_eq!(changed.costumes.variant_storage, 0xa5);
                for (address, bytes) in reconstruct(&changed, &sources) {
                    assert_eq!(bytes, dol::slice(&executable, address, bytes.len())?);
                }
                let modified = destination.join("modified.dol");
                fs::write(&modified, &executable)?;
                cook(&modified, &executable, &destination)?;
                assert_eq!(
                    crate::embedded::read::<Catalogue>(&destination, FAMILY, "modified.dol")?,
                    changed
                );
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
