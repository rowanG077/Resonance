//! Checked scalars, byte labels and storage ranges for source asset parsers.
use anyhow::{Context, Result, ensure};
use std::ops::Range;

/// Editable finite operands; inactive NaN/Infinity payloads retain their exact bits.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum FloatOperand {
    Value(f32),
    Bits { bits: u32 },
}

impl FloatOperand {
    pub(crate) fn from_bits(bits: u32) -> Self {
        let value = f32::from_bits(bits);
        if value.is_finite() {
            Self::Value(value)
        } else {
            Self::Bits { bits }
        }
    }

    pub(crate) fn read(bytes: &[u8], offset: usize) -> Result<Self> {
        Ok(Self::from_bits(u32(bytes, offset)?))
    }

    pub(crate) fn bits(self) -> u32 {
        match self {
            Self::Value(value) => value.to_bits(),
            Self::Bits { bits } => bits,
        }
    }

    pub(crate) fn finite(self) -> Result<f32> {
        let value = f32::from_bits(self.bits());
        ensure!(value.is_finite(), "non-finite active float operand");
        Ok(value)
    }
}

/// Source-relative bytes outside decoded records; their meaning is not inferred.
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Storage {
    pub offset: usize,
    pub bytes: Vec<u8>,
}

pub(crate) fn unreferenced_storage(bytes: &[u8], covered: Vec<Range<usize>>) -> Vec<Storage> {
    unreferenced_ranges(bytes, covered)
        .into_iter()
        .map(|range| Storage {
            offset: range.start,
            bytes: bytes[range].to_vec(),
        })
        .collect()
}

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

pub(crate) fn c_string(data: &[u8], offset: usize) -> Result<&[u8]> {
    let bytes = data.get(offset..).context("string outside resource")?;
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated string")?;
    Ok(&bytes[..end])
}

/// Callers validate consumed spans first. Shared/overlapping storage is allowed.
// Also compiled directly by inspection examples; the sentinel is a byte span.
#[allow(clippy::single_range_in_vec_init)]
pub(crate) fn unreferenced_ranges(
    bytes: &[u8],
    mut covered: Vec<Range<usize>>,
) -> Vec<Range<usize>> {
    covered.sort_unstable_by_key(|range| range.start);
    let mut end = 0;
    let mut unused = Vec::new();
    for range in covered.into_iter().chain([bytes.len()..bytes.len()]) {
        if range.start > end && bytes[end..range.start].iter().any(|&byte| byte != 0) {
            unused.push(end..range.start);
        }
        end = end.max(range.end);
    }
    unused
}
