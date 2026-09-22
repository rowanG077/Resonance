//! Name-editor labels, character defaults and the shared input/render keyboard binding.
use super::text::{TextPool, TextRef};
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
const FAMILY: &str = "rename-ui";
const LABELS: u32 = 0x8019d3e8;
const KEYBOARD: u32 = 0x8019d424;
const KEYBOARD_SIZE: usize = 108;
const BINDING: u32 = 0x8035cf68;

super::text::record! {
    pub(crate) struct Labels(r: Option<TextRef>) {
        pub heading: Option<TextRef> => r[0],
        pub delete: Option<TextRef> => r[1],
        pub default: Option<TextRef> => r[2],
        pub decision: Option<TextRef> => r[3],
        pub restore: Option<TextRef> => r[4],
        pub cancel: Option<TextRef> => r[5],
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Keyboard {
    pub authored: TextRef,
    /// Input and rendering both follow this pointer; it can alias the authored grid.
    pub bound_cells: Option<TextRef>,
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

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let mut texts = TextPool::default();
    let labels: [_; 15] = texts.array(executable, LABELS)?;
    let binding = dol::slice(executable, BINDING, 4)?;
    let keyboard = Keyboard {
        authored: texts.fixed(executable, KEYBOARD, KEYBOARD_SIZE)?,
        bound_cells: texts.reference(executable, word(binding, 0)?)?,
    };
    Ok(Catalogue {
        texts: texts.values,
        labels: Labels::from_refs(&labels),
        defaults: labels[6..].try_into()?,
        keyboard,
    })
}

#[cfg(test)]
pub(crate) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let catalogue = read(executable)?;
    crate::embedded::write(file, output, FAMILY, &catalogue)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;

    /// Shared by the single integration test with the menu projection.
    pub(crate) fn recover(
        file: &Path,
        executable: &[u8],
        output: &Path,
    ) -> Result<(Catalogue, String)> {
        let catalogue = read(executable)?;
        let paths = cook(file, executable, output)?;
        let restored: Catalogue = crate::embedded::read(output, FAMILY, "main.dol")?;
        assert_eq!(restored, catalogue);
        assert_eq!(
            restored.keyboard.bound_cells,
            Some(restored.keyboard.authored)
        );
        assert_eq!(restored.text(restored.keyboard.authored).len(), 13 * 8);
        assert_eq!(restored.required_text(restored.defaults[1])?, "Collet");
        assert_eq!(restored.required_text(restored.defaults[2])?, "Genius");
        let provenance: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
        assert_eq!(provenance["source_sha256"], crate::digest(executable));

        let mut changed = executable.to_vec();
        for (address, bytes) in [
            (LABELS, 0u32.to_be_bytes().to_vec()),
            (
                LABELS + 4,
                word(dol::slice(executable, LABELS + 8, 4)?, 0)?
                    .to_be_bytes()
                    .to_vec(),
            ),
            (BINDING, 0u32.to_be_bytes().to_vec()),
        ] {
            let at = dol::slice(&changed, address, bytes.len())?.as_ptr() as usize
                - changed.as_ptr() as usize;
            changed[at..at + bytes.len()].copy_from_slice(&bytes);
        }
        let altered = read(&changed)?;
        let roundtrip: Catalogue = serde_json::from_slice(&serde_json::to_vec(&altered)?)?;
        assert_eq!(roundtrip, altered);
        assert_eq!(altered.labels.heading, None);
        assert_eq!(altered.labels.delete, altered.labels.default);
        assert_eq!(altered.keyboard.bound_cells, None);
        let at = dol::slice(&changed, KEYBOARD, KEYBOARD_SIZE)?.as_ptr() as usize
            - changed.as_ptr() as usize;
        changed[at..at + KEYBOARD_SIZE].fill(b'A');
        assert!(
            read(&changed).is_err(),
            "keyboard must fit its fixed source slot"
        );
        Ok((restored, paths[0].clone()))
    }
}
