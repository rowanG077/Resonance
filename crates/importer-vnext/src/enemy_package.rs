//! Read one original enemy package without building a whole source inventory.
use crate::{compression, dol};
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(crate) struct EnemyPackages {
    file: File,
    directory: Vec<u8>,
}

impl EnemyPackages {
    pub(crate) fn open(extracted: &Path) -> Result<Self> {
        let usual = std::fs::read(extracted.join("files/BTL/BTLusual.dat"))?;
        let count = dol::u32(&usual, 0)? as usize;
        ensure!(count > 10, "enemy directory is absent");
        let start = dol::u32(&usual, 4 + 10 * 4)? as usize;
        let end = dol::u32(&usual, 4 + 11 * 4)? as usize;
        let directory = usual.get(start..end).context("invalid enemy directory")?;
        Ok(Self {
            file: File::open(extracted.join("files/BTL/BTLenemy.dat"))?,
            directory: directory.to_vec(),
        })
    }

    pub(crate) fn read(&mut self, id: u8) -> Result<Vec<u8>> {
        let slot = usize::from(id) * 4;
        let first = dol::u32(&self.directory, slot)? as u64;
        let last = dol::u32(&self.directory, slot + 4)? as u64;
        ensure!(
            first < last && last <= self.file.metadata()?.len(),
            "invalid enemy package range"
        );
        self.file.seek(SeekFrom::Start(first))?;
        let mut compressed = vec![0; usize::try_from(last - first)?];
        self.file.read_exact(&mut compressed)?;
        compression::decode(&compressed)
    }
}
