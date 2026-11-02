//! Steam's 32-bit TOFHDB directory and TLZC resource compression.
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
};

const HEADER_SIZE: usize = 40;
const RECORD_SIZE: usize = 40;
pub(super) const RESOURCE_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub name_hash: u32,
    pub size: u64,
    pub stored_size: u64,
    pub offset: u64,
    /// Some shipped records have garbage after a NUL or an invalid length.
    /// Keep the original field; never use it as an unchecked filesystem path.
    pub extension_bytes: [u8; 10],
    pub extension_length: u8,
    pub indexed: bool,
}

impl Entry {
    pub fn extension(&self) -> &str {
        let raw = &self.extension_bytes[..usize::from(self.extension_length).min(9)];
        let end = raw.iter().position(|&byte| byte == 0).unwrap_or(raw.len());
        std::str::from_utf8(&raw[..end]).unwrap_or("")
    }

    pub fn read(&self, reader: &mut (impl Read + Seek)) -> Result<Vec<u8>> {
        ensure!(
            self.size <= RESOURCE_LIMIT && self.stored_size <= RESOURCE_LIMIT,
            "Steam resource exceeds 64 MiB"
        );
        reader.seek(SeekFrom::Start(self.offset))?;
        let mut stored = vec![0; usize::try_from(self.stored_size)?];
        reader.read_exact(&mut stored)?;
        if stored.starts_with(b"TLZC") {
            decode_tlzc(&stored, self.size)
        } else {
            ensure!(
                self.size == self.stored_size,
                "compressed Steam resource has no TLZC header"
            );
            Ok(stored)
        }
    }
}

#[derive(Debug)]
pub(crate) struct Directory {
    /// Physical records include resources omitted from the game's lookup table.
    pub entries: Vec<Entry>,
    pub lookup: BTreeMap<u32, usize>,
}

pub(super) fn u32(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..)
            .and_then(|b| b.get(..4))
            .context("truncated Steam word")?
            .try_into()?,
    ))
}

fn u64(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..)
            .and_then(|b| b.get(..8))
            .context("truncated Steam double word")?
            .try_into()?,
    ))
}

fn table(bytes: &[u8], pointer: usize, count: usize, stride: usize) -> Result<&[u8]> {
    let offset = pointer
        .checked_add(u32(bytes, pointer)? as usize)
        .context("Steam directory offset overflow")?;
    let size = count
        .checked_mul(stride)
        .context("Steam directory size overflow")?;
    ensure!(
        offset >= HEADER_SIZE,
        "Steam directory table overlaps its header"
    );
    bytes
        .get(offset..)
        .and_then(|b| b.get(..size))
        .context("truncated Steam directory table")
}

impl Directory {
    pub fn parse(bytes: &[u8], data_size: u64) -> Result<Self> {
        ensure!(bytes.len() >= HEADER_SIZE, "truncated Steam archive header");
        ensure!(
            u32(bytes, 16)? == 0
                && u32(bytes, 20)? == 0
                && u32(bytes, 32)? == 0
                && u32(bytes, 36)? == 0,
            "unsupported Steam virtual archive tables"
        );
        let rows = table(bytes, 24, u32(bytes, 28)? as usize, RECORD_SIZE)?;
        let mut entries = Vec::with_capacity(rows.len() / RECORD_SIZE);
        for (index, row) in rows.chunks_exact(RECORD_SIZE).enumerate() {
            let size = u64(row, 0)?;
            let stored_size = u64(row, 8)?;
            let offset = u64(row, 16)?;
            ensure!(
                offset
                    .checked_add(stored_size)
                    .is_some_and(|end| end <= data_size),
                "Steam archive record {index} exceeds data file"
            );
            ensure!(row[39] == 0, "unsupported Steam archive record flags");
            entries.push(Entry {
                name_hash: u32(row, 24)?,
                size,
                stored_size,
                offset,
                extension_bytes: row[28..38].try_into()?,
                extension_length: row[38],
                indexed: false,
            });
        }
        let hashes = table(bytes, 8, u32(bytes, 12)? as usize, 8)?;
        let mut lookup = BTreeMap::new();
        let mut previous = None;
        for row in hashes.chunks_exact(8) {
            let hash = u32(row, 0)?;
            let index = u32(row, 4)? as usize;
            ensure!(
                previous.is_none_or(|previous| previous < hash),
                "unsorted or duplicate Steam lookup hash"
            );
            previous = Some(hash);
            let entry = entries
                .get_mut(index)
                .context("Steam lookup references an absent record")?;
            ensure!(
                entry.name_hash == hash,
                "Steam lookup hash disagrees with its record"
            );
            entry.indexed = true;
            lookup.insert(hash, index);
        }
        Ok(Self { entries, lookup })
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.get(*self.lookup.get(&name_hash(name))?)
    }
}

/// The original lookup hashes an uppercase basename, including its extension.
pub fn name_hash(name: &str) -> u32 {
    name.bytes().fold(0_u32, |hash, byte| {
        hash ^ u32::from(byte.to_ascii_uppercase())
            .wrapping_add(hash.wrapping_shl(6))
            .wrapping_add(hash >> 2)
            .wrapping_add(0x9e37_79b9)
    })
}

fn decode_tlzc(bytes: &[u8], expected: u64) -> Result<Vec<u8>> {
    ensure!(bytes.len() >= 24, "truncated TLZC header");
    ensure!(
        bytes[..8] == *b"TLZC\x01\x02\0\0",
        "unsupported TLZC compression method or version"
    );
    ensure!(
        u32(bytes, 8)? as usize == bytes.len(),
        "TLZC stored length mismatch"
    );
    ensure!(u64(bytes, 16)? == 0, "unsupported TLZC flags");
    ensure!(
        u64::from(u32(bytes, 12)?) == expected && expected <= RESOURCE_LIMIT,
        "TLZC expanded length mismatch or limit exceeded"
    );
    let mut output = vec![0; usize::try_from(expected)? + 1];
    let mut decoder = flate2::Decompress::new(false);
    let status = decoder.decompress(&bytes[24..], &mut output, flate2::FlushDecompress::Finish)?;
    ensure!(
        status == flate2::Status::StreamEnd
            && decoder.total_in() == (bytes.len() - 24) as u64
            && decoder.total_out() == expected,
        "incomplete TLZC stream or incorrect expanded length"
    );
    output.truncate(expected as usize);
    Ok(output)
}

#[cfg(test)]
pub(super) mod tests;
