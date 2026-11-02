//! Read initialized PE32 image data without loading or executing the program.
use anyhow::{Context, Result, ensure};
use std::ops::Range;

pub fn half(bytes: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(at..at.checked_add(2).context("PE offset overflow")?)
            .context("truncated PE halfword")?
            .try_into()?,
    ))
}

pub fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at.checked_add(4).context("PE offset overflow")?)
            .context("truncated PE word")?
            .try_into()?,
    ))
}

pub(crate) fn string(bytes: &[u8], at: usize) -> Result<&str> {
    let bytes = bytes.get(at..).context("string starts outside resource")?;
    let end = bytes
        .iter()
        .take(4097)
        .position(|&b| b == 0)
        .context("unterminated resource string")?;
    Ok(std::str::from_utf8(&bytes[..end])?)
}

struct Section {
    memory: Range<u32>,
    raw: Range<usize>,
}

pub struct Image<'a> {
    bytes: &'a [u8],
    base: u32,
    sections: Vec<Section>,
}

impl<'a> Image<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        ensure!(bytes.get(..2) == Some(b"MZ"), "invalid DOS header");
        let pe = word(bytes, 0x3c)? as usize;
        let header = bytes.get(pe..).context("PE header offset outside file")?;
        ensure!(
            header.get(..4) == Some(b"PE\0\0") && half(header, 4)? == 0x14c,
            "expected an x86 PE image"
        );
        let count = usize::from(half(header, 6)?);
        let optional_size = usize::from(half(header, 20)?);
        ensure!(
            (1..=96).contains(&count) && optional_size >= 96,
            "invalid PE header dimensions"
        );
        let optional = header
            .get(24..24 + optional_size)
            .context("truncated PE optional header")?;
        ensure!(half(optional, 0)? == 0x10b, "expected PE32 optional header");
        let base = word(optional, 28)?;
        let image_size = word(optional, 56)?;
        ensure!(
            base.checked_add(image_size).is_some(),
            "PE image address overflow"
        );
        let headers_size = word(optional, 60)? as usize;
        let table = 24 + optional_size;
        ensure!(
            headers_size <= bytes.len() && pe + table + count * 40 <= headers_size,
            "PE sections exceed headers"
        );
        let mut sections = Vec::<Section>::with_capacity(count);
        for index in 0..count {
            let row = header
                .get(table + index * 40..table + (index + 1) * 40)
                .context("truncated PE section table")?;
            let virtual_size = word(row, 8)?;
            let rva = word(row, 12)?;
            let raw_size = word(row, 16)?;
            let raw_start = word(row, 20)? as usize;
            let raw_end = raw_start
                .checked_add(raw_size as usize)
                .context("PE file range overflow")?;
            let memory_end = rva
                .checked_add(virtual_size.max(raw_size))
                .context("PE section range overflow")?;
            ensure!(
                memory_end <= image_size && raw_end <= bytes.len(),
                "PE section exceeds image or file"
            );
            ensure!(
                raw_size == 0 || raw_start >= headers_size,
                "PE section overlaps headers"
            );
            let memory = rva..memory_end;
            let raw = raw_start..raw_end;
            for previous in &sections {
                ensure!(
                    !overlap(&memory, &previous.memory),
                    "overlapping PE virtual sections"
                );
                ensure!(
                    !overlap(&raw, &previous.raw),
                    "overlapping PE file sections"
                );
            }
            sections.push(Section { memory, raw });
        }
        Ok(Self {
            bytes,
            base,
            sections,
        })
    }

    fn tail(&self, address: u32) -> Result<&'a [u8]> {
        let rva = address
            .checked_sub(self.base)
            .context("PE address precedes image base")?;
        let section = self
            .sections
            .iter()
            .find(|s| s.memory.contains(&rva))
            .with_context(|| format!("PE address {address:#x} is outside sections"))?;
        let offset = (rva - section.memory.start) as usize;
        ensure!(
            offset < section.raw.len(),
            "PE address has no initialized file data"
        );
        Ok(&self.bytes[section.raw.start + offset..section.raw.end])
    }

    pub fn slice(&self, address: u32, length: usize) -> Result<&'a [u8]> {
        self.tail(address)?
            .get(..length)
            .context("PE read crosses initialized section boundary")
    }

    pub fn text(&self, address: u32) -> Result<&'a str> {
        let bytes = self.tail(address)?;
        let end = bytes
            .iter()
            .take(4096)
            .position(|&b| b == 0)
            .context("unterminated PE string")?;
        ensure!(bytes[..end].is_ascii(), "non-ASCII PE resource name");
        Ok(std::str::from_utf8(&bytes[..end])?)
    }
}

fn overlap<T: Ord>(a: &Range<T>, b: &Range<T>) -> bool {
    !a.is_empty() && !b.is_empty() && a.start < b.end && b.start < a.end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 0x240];
        b[..2].copy_from_slice(b"MZ");
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        for (at, v) in [
            (0x3c, 0x80u32),
            (0xb4, 0x400000),
            (0xd0, 0x2000),
            (0xd4, 0x200),
            (0x100, 0x100),
            (0x104, 0x1000),
            (0x108, 0x40),
            (0x10c, 0x200),
        ] {
            b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        for (at, v) in [(0x84, 0x14cu16), (0x86, 1), (0x94, 96), (0x98, 0x10b)] {
            b[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
        b[0x200..0x209].copy_from_slice(b"BGM.AWB\0\xff");
        b
    }

    #[test]
    fn translates_virtual_addresses_and_rejects_uninitialized_data() -> Result<()> {
        let b = fixture();
        let image = Image::parse(&b)?;
        assert_eq!(image.text(0x401000)?, "BGM.AWB");
        assert_eq!(image.slice(0x401008, 1)?, [255]);
        for (address, size) in [(0x40103f, 2), (0x401040, 1), (0x400100, 1), (0x3fffff, 1)] {
            assert!(image.slice(address, size).is_err());
        }
        assert!(image.text(0x401008).is_err());
        Ok(())
    }

    #[test]
    fn rejects_truncated_and_overlapping_sections() {
        let b = fixture();
        for size in [0, 63, 0x97, 0x120, 0x23f] {
            assert!(Image::parse(&b[..size]).is_err());
        }
        let mut bad = b.clone();
        bad[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
        bad.copy_within(0xf8..0x120, 0x120);
        assert!(Image::parse(&bad).is_err());
        let mut bad = b;
        bad[0x104..0x108].copy_from_slice(&0xfffffff0u32.to_le_bytes());
        assert!(Image::parse(&bad).is_err());
    }
}
