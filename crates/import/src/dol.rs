//! Checked offline access to original executable constants.
use anyhow::{Context, Result, bail, ensure};

pub(crate) fn optional_text(data: &[u8], address: u32) -> Result<Option<String>> {
    (address != 0).then(|| text(data, address)).transpose()
}

pub(crate) fn text(data: &[u8], address: u32) -> Result<String> {
    if address == 0 {
        return Ok(String::new());
    }
    let bytes = crate::read::c_string(mapped(data, address, None)?, 0)?;
    let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(bytes);
    ensure!(!invalid, "invalid executable string encoding");
    Ok(text.into_owned())
}

pub(crate) fn slice(data: &[u8], address: u32, size: usize) -> Result<&[u8]> {
    mapped(data, address, Some(size))
}

/// Match an embedded texture declaration to the generic data-section publication.
pub(crate) fn texture_bank(data: &[u8], address: u32) -> Result<String> {
    for section in 7..18 {
        let base = crate::read::u32(data, 0x48 + section * 4)?;
        let length = crate::read::u32(data, 0x90 + section * 4)?;
        if let Some(offset) = address.checked_sub(base).filter(|offset| *offset < length) {
            return Ok(format!("embedded/dol/section-{section}/tpl-{offset:x}"));
        }
    }
    bail!("texture address {address:#x} outside DOL data sections")
}

/// Without a length, return only the remainder of the containing section.
fn mapped(data: &[u8], address: u32, size: Option<usize>) -> Result<&[u8]> {
    let word = |at| -> Result<u64> {
        Ok(u64::from(u32::from_be_bytes(
            data.get(at..at + 4)
                .context("truncated DOL header")?
                .try_into()?,
        )))
    };
    for index in 0..18 {
        let offset = word(index * 4)?;
        let base = word(0x48 + index * 4)?;
        let length = word(0x90 + index * 4)?;
        let address = u64::from(address);
        if base <= address
            && address
                .checked_add(size.map_or(1, |size| size as u64))
                .is_some_and(|end| end <= base + length)
        {
            let start = usize::try_from(offset + address - base)?;
            let size = size.unwrap_or(usize::try_from(base + length - address)?);
            return data
                .get(start..start.checked_add(size).context("DOL range overflow")?)
                .context("DOL section extends beyond file");
        }
    }
    bail!("DOL range {address:#x} with length {size:?} is missing")
}

#[test]
fn executable_strings_are_bounded_by_their_section() -> Result<()> {
    let base = 0x80001000_u32;
    let mut bytes = vec![b'a'; 0x100 + 5001];
    bytes[..0x100].fill(0);
    for (at, value) in [(0, 0x100_u32), (0x48, base), (0x90, 5001)] {
        bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    bytes[0x100 + 5000] = 0;
    assert_eq!(text(&bytes, base)?.len(), 5000);
    bytes[0x90..0x94].copy_from_slice(&5000_u32.to_be_bytes());
    assert!(text(&bytes, base).is_err());
    assert!(text(&bytes, u32::MAX).is_err());
    assert!(slice(&bytes, base + 4999, 2).is_err());
    Ok(())
}
