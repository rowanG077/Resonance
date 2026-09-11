//! Checked offline access to original executable constants.
use anyhow::{Context, Result, bail, ensure};

pub(crate) fn text(data: &[u8], address: u32) -> Result<String> {
    if address == 0 {
        return Ok(String::new());
    }
    let end = (0..4096)
        .find_map(|offset| match slice(data, address + offset, 1) {
            Ok([0]) => Some(Ok(offset as usize)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .context("unterminated executable string")??;
    let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(slice(data, address, end)?);
    ensure!(!invalid, "invalid executable string encoding");
    Ok(text.into_owned())
}

pub(crate) fn slice(data: &[u8], address: u32, size: usize) -> Result<&[u8]> {
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
                .checked_add(size as u64)
                .is_some_and(|end| end <= base + length)
        {
            let start = usize::try_from(offset + address - base)?;
            return data
                .get(start..start.checked_add(size).context("DOL range overflow")?)
                .context("DOL section extends beyond file");
        }
    }
    bail!("DOL range {address:#x}+{size:#x} is missing")
}
