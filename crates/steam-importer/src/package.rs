//! Read the little-endian member directory of a Steam PAC.
use super::archive::u32;
use anyhow::{Context, Result, ensure};
use std::ops::Range;

pub struct Package<'a> {
    bytes: &'a [u8],
    sections: Vec<Option<Range<usize>>>,
}

impl<'a> Package<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let count = u32(bytes, 0)? as usize;
        ensure!(
            (1..=65536).contains(&count),
            "invalid Steam package member count"
        );
        let end = 4 + count * 4;
        let table = bytes
            .get(4..end)
            .context("truncated Steam package directory")?;
        let offsets = table
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()) as usize)
            .collect::<Vec<_>>();
        for &offset in &offsets {
            ensure!(
                offset == 0 || (end..bytes.len()).contains(&offset),
                "Steam package member overlaps directory or exceeds file"
            );
        }
        let mut boundaries = offsets.clone();
        boundaries.push(bytes.len());
        boundaries.sort_unstable();
        boundaries.dedup();
        let sections = offsets
            .iter()
            .map(|&start| {
                (start != 0)
                    .then(|| start..boundaries[boundaries.partition_point(|&at| at <= start)])
            })
            .collect();
        Ok(Self { bytes, sections })
    }

    pub fn member(&self, index: usize) -> Result<Option<&'a [u8]>> {
        Ok(self
            .sections
            .get(index)
            .context("Steam package member index exceeds directory")?
            .as_ref()
            .map(|range| &self.bytes[range.clone()]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_absent_aliased_unordered_and_inline_members() -> Result<()> {
        let mut bytes = Vec::new();
        for v in [4u32, 32, 20, 32, 0] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend(b"room.bin\0\0\xfe\xfe");
        bytes.extend([1, 0, 0, 0, 20, 0, 0, 0]);
        let p = Package::parse(&bytes)?;
        assert_eq!(p.member(1)?, Some(b"room.bin\0\0\xfe\xfe".as_slice()));
        assert_eq!(p.member(0)?, p.member(2)?);
        assert_eq!(p.member(3)?, None);
        assert!(p.member(4).is_err());
        bytes[4..8].copy_from_slice(&8u32.to_le_bytes());
        assert!(Package::parse(&bytes).is_err());
        Ok(())
    }
    #[test]
    fn rejects_invalid_package_directories() {
        for b in [
            vec![],
            vec![0; 4],
            vec![255; 4],
            [1u32.to_le_bytes(), 8u32.to_le_bytes()].concat(),
        ] {
            assert!(Package::parse(&b).is_err());
        }
    }
}
