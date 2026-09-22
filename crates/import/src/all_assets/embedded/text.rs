//! Shared text decoding with stable references inside each catalogue.
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TextRef(pub(crate) usize);

#[derive(Default)]
pub(crate) struct TextPool {
    ids: BTreeMap<u32, (TextRef, u32)>,
    pub values: Vec<String>,
}

impl TextPool {
    pub fn fixed(&mut self, executable: &[u8], address: u32, size: usize) -> Result<TextRef> {
        dol::slice(executable, address, size)?;
        let (reference, end) = self.read(executable, address)?;
        ensure!(
            (end - address) as usize <= size,
            "text exceeds its fixed source slot"
        );
        Ok(reference)
    }

    pub fn reference(&mut self, executable: &[u8], address: u32) -> Result<Option<TextRef>> {
        (address != 0)
            .then(|| self.required(executable, address))
            .transpose()
    }

    pub fn read(&mut self, executable: &[u8], address: u32) -> Result<(TextRef, u32)> {
        if let Some(reference) = self.ids.get(&address) {
            return Ok(*reference);
        }
        let (text, next) = crate::menu::source_text(executable, address)?;
        let reference = TextRef(self.values.len());
        self.values.push(text);
        self.ids.insert(address, (reference, next));
        Ok((reference, next))
    }

    pub fn required(&mut self, executable: &[u8], address: u32) -> Result<TextRef> {
        ensure!(address != 0, "null required UI text");
        Ok(self.read(executable, address)?.0)
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

// Keep each named JSON field beside its source-table binding.
macro_rules! record {
    ($(#[$meta:meta])* $vis:vis struct $name:ident($refs:ident: $reference:ty) {
        $($(#[$field_meta:meta])* $field_vis:vis $field:ident: $ty:ty => $value:expr,)*
    }) => {
        $(#[$meta])*
        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        $vis struct $name { $($(#[$field_meta])* $field_vis $field: $ty,)* }
        impl $name {
            fn from_refs($refs: &[$reference]) -> Self {
                Self { $($field: $value,)* }
            }
        }
    }
}
pub(crate) use record;
