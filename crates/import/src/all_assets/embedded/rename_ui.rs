//! Name-editor labels, character defaults and the shared input/render keyboard binding.
use super::text::{FixedText, TextPool, TextRef, TextSource};
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "rename-ui";
const LABELS: u32 = 0x8019d3e8;
const KEYBOARD: u32 = 0x8019d424;
const KEYBOARD_SIZE: usize = 108;
const BINDING: u32 = 0x8035cf68;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Labels {
    pub heading: Option<TextRef>,
    pub delete: Option<TextRef>,
    pub default: Option<TextRef>,
    pub decision: Option<TextRef>,
    pub restore: Option<TextRef>,
    pub cancel: Option<TextRef>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Keyboard {
    pub authored: FixedText,
    /// Input and rendering both follow this pointer; it can alias the authored grid.
    pub bound_cells: Option<TextRef>,
    /// Unconsumed word after the pointer in its eight-byte declaration.
    pub binding_storage: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub labels: Labels,
    /// One-based playable character order, independent of the editable saved names.
    pub defaults: [Option<TextRef>; 9],
    pub keyboard: Keyboard,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }

    pub(crate) fn required_text(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required rename text")?))
    }
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let mut texts = TextPool::default();
    let labels: [_; 15] = texts.array(executable, LABELS)?;
    let binding = dol::slice(executable, BINDING, 8)?;
    let keyboard = Keyboard {
        authored: texts.fixed(executable, KEYBOARD, KEYBOARD_SIZE)?,
        bound_cells: texts.reference(executable, word(binding, 0)?)?,
        binding_storage: word(binding, 4)?,
    };
    Ok((
        Catalogue {
            texts: texts.values,
            labels: Labels {
                heading: labels[0],
                delete: labels[1],
                default: labels[2],
                decision: labels[3],
                restore: labels[4],
                cancel: labels[5],
            },
            defaults: labels[6..].try_into()?,
            keyboard,
        },
        texts.sources,
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, texts) = parse(executable)?;
    crate::embedded::write(
        file,
        output,
        FAMILY,
        &catalogue,
        serde_json::json!({
            "labels":{"address":LABELS,"count":15,"stride":4,"source_size":60},
            "keyboard":{"address":KEYBOARD,"rows":8,"columns":13,"source_size":KEYBOARD_SIZE},
            "binding":{"address":BINDING,"source_size":8,"bound_address":word(dol::slice(executable,BINDING,4)?,0)?},
            "texts":texts,
        }),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use anyhow::ensure;
    use std::fs;

    fn reconstruct(c: &Catalogue, sources: &[TextSource]) -> Result<Vec<(u32, Vec<u8>)>> {
        let pointer = |reference: Option<TextRef>| reference.map_or(0, |id| sources[id.0].address);
        let text = |reference: TextRef| -> Result<Vec<u8>> {
            let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(c.text(reference));
            ensure!(!invalid, "rename text cannot reconstruct source encoding");
            Ok([bytes.as_ref(), &[0]].concat())
        };
        let l = &c.labels;
        let labels = [
            l.heading, l.delete, l.default, l.decision, l.restore, l.cancel,
        ]
        .into_iter()
        .chain(c.defaults)
        .flat_map(|id| pointer(id).to_be_bytes())
        .collect();
        let mut keyboard = text(c.keyboard.authored.text)?;
        keyboard.extend(&c.keyboard.authored.storage);
        let binding = [pointer(c.keyboard.bound_cells), c.keyboard.binding_storage]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect();
        let mut spans = vec![(LABELS, labels), (KEYBOARD, keyboard), (BINDING, binding)];
        for (index, source) in sources.iter().enumerate() {
            let bytes = text(TextRef(index))?;
            assert_eq!(bytes.len() as u32, source.source_size);
            spans.push((source.address, bytes));
        }
        Ok(spans)
    }

    fn check_bytes(executable: &[u8], catalogue: &Catalogue, sources: &[TextSource]) -> Result<()> {
        for (address, bytes) in reconstruct(catalogue, sources)? {
            assert_eq!(
                bytes,
                dol::slice(executable, address, bytes.len())?,
                "span {address:#x}"
            );
        }
        Ok(())
    }

    /// Shared by the single integration test with the menu projection.
    pub(crate) fn recover(
        file: &Path,
        executable: &[u8],
        output: &Path,
    ) -> Result<(Catalogue, String)> {
        let (catalogue, sources) = parse(executable)?;
        let paths = cook(file, executable, output)?;
        let restored: Catalogue = crate::embedded::read(output, FAMILY, "main.dol")?;
        assert_eq!(restored, catalogue);
        check_bytes(executable, &restored, &sources)?;
        assert_eq!(
            restored.keyboard.bound_cells,
            Some(restored.keyboard.authored.text)
        );
        assert_eq!(restored.text(restored.keyboard.authored.text).len(), 13 * 8);
        assert_eq!(restored.keyboard.authored.storage, [0; 3]);
        assert_eq!(restored.required_text(restored.defaults[1])?, "Collet");
        assert_eq!(restored.required_text(restored.defaults[2])?, "Genius");
        let provenance: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
        assert_eq!(provenance["source_sha256"], crate::digest(executable));
        assert_eq!(provenance["labels"]["source_size"], 60);
        assert_eq!(provenance["keyboard"]["source_size"], 108);
        assert_eq!(provenance["binding"]["source_size"], 8);

        let mut changed = executable.to_vec();
        for (address, bytes) in [
            (LABELS, 0u32.to_be_bytes().to_vec()),
            (
                LABELS + 4,
                word(dol::slice(executable, LABELS + 8, 4)?, 0)?
                    .to_be_bytes()
                    .to_vec(),
            ),
            (KEYBOARD + 105, vec![0x80, 0xff, 0x11]),
            (
                BINDING,
                [0u32.to_be_bytes(), 0xdeadbeefu32.to_be_bytes()].concat(),
            ),
        ] {
            let at = dol::slice(&changed, address, bytes.len())?.as_ptr() as usize
                - changed.as_ptr() as usize;
            changed[at..at + bytes.len()].copy_from_slice(&bytes);
        }
        let (altered, sources) = parse(&changed)?;
        let roundtrip: Catalogue = serde_json::from_slice(&serde_json::to_vec(&altered)?)?;
        assert_eq!(roundtrip, altered);
        assert_eq!(altered.labels.heading, None);
        assert_eq!(altered.labels.delete, altered.labels.default);
        assert_eq!(altered.keyboard.bound_cells, None);
        assert_eq!(altered.keyboard.binding_storage, 0xdeadbeef);
        assert_eq!(altered.keyboard.authored.storage, [0x80, 0xff, 0x11]);
        check_bytes(&changed, &roundtrip, &sources)?;
        Ok((restored, paths[0].clone()))
    }
}
