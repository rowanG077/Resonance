//! Checked big-endian scalars for source asset parsers.
use anyhow::{Context, Result, ensure};

fn bytes<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N]> {
    offset
        .checked_add(N)
        .and_then(|end| data.get(offset..end))
        .with_context(|| format!("truncated {N}-byte value at {offset:#x}"))?
        .try_into()
        .map_err(Into::into)
}
pub(crate) fn u16(data: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(bytes(data, offset)?))
}
pub(crate) fn u32(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(bytes(data, offset)?))
}
pub(crate) fn f32(data: &[u8], offset: usize) -> Result<f32> {
    let value = f32::from_bits(u32(data, offset)?);
    ensure!(value.is_finite(), "non-finite float at {offset:#x}");
    Ok(value)
}
