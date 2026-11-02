//! AFS2 wave-bank indexing. Wave IDs are authored keys, not physical row numbers.
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeSet,
    io::{Read, Seek, SeekFrom},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u16,
    pub offset: u64,
    pub size: usize,
}

impl Entry {
    pub fn read(&self, input: &mut (impl Read + Seek)) -> Result<Vec<u8>> {
        ensure!(
            self.size <= 128 * 1024 * 1024,
            "AWB stream exceeds read budget"
        );
        input.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = vec![0; self.size];
        input.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

pub fn index(input: &mut (impl Read + Seek)) -> Result<Vec<Entry>> {
    let size = input.seek(SeekFrom::End(0))?;
    input.seek(SeekFrom::Start(0))?;
    let mut header = [0; 16];
    input.read_exact(&mut header)?;
    ensure!(
        &header[..4] == b"AFS2" && header[4] == 1,
        "unsupported AWB header"
    );
    let width = usize::from(header[5]);
    let id_width = usize::from(u16::from_le_bytes(header[6..8].try_into()?));
    let count = u32::from_le_bytes(header[8..12].try_into()?) as usize;
    let alignment = u64::from(u16::from_le_bytes(header[12..14].try_into()?));
    ensure!(
        matches!(width, 2 | 4) && id_width == 2 && count <= 65536 && alignment > 0,
        "unsupported AWB table dimensions"
    );
    ensure!(
        header[14..16] == [0, 0],
        "encrypted AWB subkeys are unsupported"
    );
    let table_size = count * id_width + (count + 1) * width;
    ensure!(16 + table_size as u64 <= size, "truncated AWB index");
    let mut table = vec![0; table_size];
    input.read_exact(&mut table)?;
    let mut offsets = Vec::with_capacity(count + 1);
    for bytes in table[count * id_width..].chunks_exact(width) {
        let mut word = [0; 4];
        word[..width].copy_from_slice(bytes);
        offsets.push(u64::from(u32::from_le_bytes(word)));
    }
    ensure!(
        offsets[0] >= 16 + table_size as u64,
        "AWB data overlaps index"
    );
    ensure!(
        offsets[count] == size,
        "AWB final offset differs from file length"
    );
    ensure!(
        offsets.windows(2).all(|pair| pair[0] <= pair[1]),
        "reversed AWB offsets"
    );
    let align = |offset: u64| -> Result<u64> {
        let aligned = offset.div_ceil(alignment) * alignment;
        ensure!(aligned <= size, "AWB aligned offset exceeds file length");
        Ok(aligned)
    };
    let mut ids = BTreeSet::new();
    table[..count * id_width]
        .chunks_exact(2)
        .enumerate()
        .map(|(i, bytes)| {
            let id = u16::from_le_bytes(bytes.try_into()?);
            ensure!(ids.insert(id), "duplicate AWB wave ID {id}");
            // An offset before EOF describes the next aligned start. EOF itself
            // must not be rounded beyond the file. Keep genuine empty entries.
            let offset = if offsets[i] == size {
                size
            } else {
                align(offsets[i])?
            };
            let end = if offsets[i + 1] == size {
                size
            } else {
                align(offsets[i + 1])?
            };
            let length = end
                .checked_sub(offset)
                .context("invalid AWB stream range")?;
            ensure!(
                length <= 128 * 1024 * 1024,
                "AWB stream exceeds read budget"
            );
            Ok(Entry {
                id,
                offset,
                size: usize::try_from(length)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn fixture(width: u8) -> Vec<u8> {
        let mut b = b"AFS2\x01\x04\x02\x00".to_vec();
        b[5] = width;
        b.extend(3u32.to_le_bytes());
        b.extend([16, 0, 0, 0]);
        for id in [9u16, 2, 44] {
            b.extend(id.to_le_bytes());
        }
        for offset in [38u32, 51, 51, 69] {
            b.extend(&offset.to_le_bytes()[..usize::from(width)]);
        }
        b.resize(69, 0);
        b[48..51].copy_from_slice(b"one");
        b[64..69].copy_from_slice(b"three");
        b
    }

    #[test]
    fn respects_authored_ids_alignment_empty_members_and_unaligned_eof() -> Result<()> {
        for width in [2, 4] {
            let mut input = Cursor::new(fixture(width));
            let entries = index(&mut input)?;
            assert_eq!(entries.iter().map(|e| e.id).collect::<Vec<_>>(), [9, 2, 44]);
            assert_eq!((entries[0].offset, entries[0].size), (48, 16));
            assert_eq!(entries[1].size, 0);
            assert_eq!(entries[2].read(&mut input)?, b"three");
        }
        Ok(())
    }

    #[test]
    fn rejects_corrupt_tables_before_reading_streams() {
        for (at, replacement) in [
            (5, vec![8]),
            (6, vec![4]),
            (12, vec![0, 0]),
            (14, vec![1]),
            (18, vec![9, 0]),
            (22, 16u32.to_le_bytes().to_vec()),
            (26, 70u32.to_le_bytes().to_vec()),
            (34, 70u32.to_le_bytes().to_vec()),
        ] {
            let mut b = fixture(4);
            b[at..at + replacement.len()].copy_from_slice(&replacement);
            assert!(
                index(&mut Cursor::new(b)).is_err(),
                "accepted mutation at {at}"
            );
        }
        let b = fixture(4);
        for length in [0, 15, 30, 68] {
            assert!(index(&mut Cursor::new(&b[..length])).is_err());
        }
    }
}
