//! Address only the executable sections used by a converter.
use anyhow::{Context, Result, bail};

pub(crate) fn u16(bytes: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(
        bytes
            .get(at..at + 2)
            .context("truncated executable halfword")?
            .try_into()?,
    ))
}

pub(crate) fn u32(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(at..at + 4)
            .context("truncated executable word")?
            .try_into()?,
    ))
}

pub(crate) fn slice(bytes: &[u8], address: u32, length: usize) -> Result<&[u8]> {
    for section in 0..18 {
        let base = u32(bytes, 0x48 + 4 * section)?;
        let size = u32(bytes, 0x90 + 4 * section)?;
        if let Some(offset) = address.checked_sub(base)
            && u64::from(offset) + length as u64 <= u64::from(size)
        {
            let file = usize::try_from(u32(bytes, 4 * section)?)?
                .checked_add(usize::try_from(offset)?)
                .context("executable offset overflow")?;
            return bytes
                .get(
                    file..file
                        .checked_add(length)
                        .context("executable range overflow")?,
                )
                .context("executable section extends beyond file");
        }
    }
    bail!("executable address {address:#x} length {length} is missing")
}
