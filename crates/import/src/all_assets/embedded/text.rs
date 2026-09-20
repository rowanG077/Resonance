//! Text aliases are identities of source pointers, not equal strings.
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TextRef(pub(crate) usize);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct FixedText {
    pub text: TextRef,
    /// Bytes after the terminator within the declared source slot.
    pub storage: Vec<u8>,
}

#[derive(Serialize)]
pub(crate) struct TextSource {
    pub address: u32,
    pub source_size: u32,
}

#[derive(Default)]
pub(crate) struct TextPool {
    ids: BTreeMap<u32, TextRef>,
    pub values: Vec<String>,
    pub sources: Vec<TextSource>,
}

impl TextPool {
    pub fn fixed(&mut self, executable: &[u8], address: u32, size: usize) -> Result<FixedText> {
        let text = self.required(executable, address)?;
        let used = self.sources[text.0].source_size as usize;
        let storage = dol::slice(executable, address, size)?
            .get(used..)
            .context("text exceeds its fixed source slot")?
            .to_vec();
        Ok(FixedText { text, storage })
    }

    pub fn reference(&mut self, executable: &[u8], address: u32) -> Result<Option<TextRef>> {
        if address == 0 {
            return Ok(None);
        }
        if let Some(reference) = self.ids.get(&address) {
            return Ok(Some(*reference));
        }
        let (text, next) = crate::menu::source_text(executable, address)?;
        let reference = TextRef(self.values.len());
        self.values.push(text);
        self.sources.push(TextSource {
            address,
            source_size: next - address,
        });
        self.ids.insert(address, reference);
        Ok(Some(reference))
    }

    pub fn required(&mut self, executable: &[u8], address: u32) -> Result<TextRef> {
        self.reference(executable, address)?
            .context("null required UI text")
    }

    pub fn table(
        &mut self,
        executable: &[u8],
        address: u32,
        count: usize,
    ) -> Result<Vec<Option<TextRef>>> {
        let size = count
            .checked_mul(4)
            .context("UI text table size overflow")?;
        dol::slice(executable, address, size)?
            .chunks_exact(4)
            .map(|pointer| self.reference(executable, word(pointer, 0)?))
            .collect()
    }

    pub fn array<const N: usize>(
        &mut self,
        executable: &[u8],
        address: u32,
    ) -> Result<[Option<TextRef>; N]> {
        Ok(self.table(executable, address, N)?.try_into().unwrap())
    }

    pub fn required_table(
        &mut self,
        executable: &[u8],
        address: u32,
        count: usize,
    ) -> Result<Vec<TextRef>> {
        self.table(executable, address, count)?
            .into_iter()
            .map(|reference| reference.context("null required UI text"))
            .collect()
    }
}
