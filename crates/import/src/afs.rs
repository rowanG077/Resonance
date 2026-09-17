//! AFS archive directory and checked payload ranges.
use anyhow::{Context, Result, ensure};
use std::io::{Read, Seek, SeekFrom};

pub(crate) struct Entry {
    pub name: String,
    pub offset: u64,
    pub size: usize,
}

pub(crate) struct Member<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
}
pub(crate) fn index(reader: &mut (impl Read + Seek)) -> Result<Vec<Entry>> {
    let length = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut header = [0; 8];
    reader.read_exact(&mut header)?;
    ensure!(header.starts_with(b"AFS\0"), "invalid AFS header");
    let count = u32::from_le_bytes(header[4..].try_into()?) as usize;
    ensure!((1..=65536).contains(&count), "invalid AFS member count");
    let mut table = vec![0; count * 8 + 8];
    reader.read_exact(&mut table)?;
    let word = |at: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(
            table
                .get(at..at + 4)
                .context("truncated AFS table")?
                .try_into()?,
        ) as usize)
    };
    let end = 8 + count * 8;
    let names = word(count * 8)?;
    let names_size = word(count * 8 + 4)?;
    ensure!(
        names >= end + 8
            && names_size >= count * 48
            && names
                .checked_add(names_size)
                .is_some_and(|n| n as u64 <= length),
        "invalid AFS name table"
    );
    let mut ranges = vec![(0, end + 8), (names, names + names_size)];
    reader.seek(SeekFrom::Start(names as u64))?;
    let mut name_bytes = vec![0; count * 48];
    reader.read_exact(&mut name_bytes)?;
    let mut members = Vec::with_capacity(count);
    for index in 0..count {
        let offset = word(index * 8)?;
        let size = word(4 + index * 8)?;
        let end = offset
            .checked_add(size)
            .context("AFS member range overflow")?;
        ensure!(size > 0 && end as u64 <= length, "invalid AFS member range");
        ranges.push((offset, end));
        let name = &name_bytes[index * 48..index * 48 + 32];
        let end = name
            .iter()
            .position(|b| *b == 0)
            .context("unterminated AFS name")?;
        let name = std::str::from_utf8(&name[..end])?;
        ensure!(
            !name.is_empty() && !name.contains(['/', '\\']),
            "invalid AFS member name"
        );
        members.push(Entry {
            name: name.into(),
            offset: offset as u64,
            size,
        });
    }
    ranges.sort_unstable();
    ensure!(
        ranges.windows(2).all(|r| r[0].1 <= r[1].0),
        "overlapping AFS members or tables"
    );
    Ok(members)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0u8; 112];
        bytes[..4].copy_from_slice(b"AFS\0");
        for (offset, value) in [(4, 1u32), (8, 32), (12, 4), (16, 64), (20, 48)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[32..36].copy_from_slice(b"AHX!");
        bytes[64..72].copy_from_slice(b"line.ahx");
        bytes
    }
    #[test]
    fn member_names_and_ranges_are_checked() {
        let mut bytes = fixture();
        let parsed = index(&mut Cursor::new(&bytes)).unwrap();
        assert_eq!(parsed[0].name, "line.ahx");
        assert_eq!((parsed[0].offset, parsed[0].size), (32, 4));
        bytes[8..12].copy_from_slice(&16u32.to_le_bytes());
        assert!(index(&mut Cursor::new(&bytes)).is_err());
        let mut bytes = fixture();
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(index(&mut Cursor::new(&bytes)).is_err());
    }
}
