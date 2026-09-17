//! Checked scalars, byte labels and storage ranges for source asset parsers.
use anyhow::{Context, Result, ensure};
use std::ops::Range;

pub(crate) use resonance_content::source::{FloatOperand, Storage};

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

/// Fixed-width fields in the original big-endian records.
pub(crate) trait Field: Sized {
    const SIZE: usize;
    fn read(data: &[u8], offset: usize) -> Result<Self>;
}

macro_rules! scalars {
    ($($ty:ty),*) => {$(
        impl Field for $ty {
            const SIZE: usize = size_of::<Self>();
            fn read(data: &[u8], offset: usize) -> Result<Self> {
                Ok(Self::from_be_bytes(bytes(data, offset)?))
            }
        }
    )*};
}
scalars!(u8, i8, u16, i16, u32, i32);

impl Field for f32 {
    const SIZE: usize = 4;
    fn read(data: &[u8], offset: usize) -> Result<Self> {
        f32(data, offset)
    }
}

impl Field for FloatOperand {
    const SIZE: usize = 4;
    fn read(data: &[u8], offset: usize) -> Result<Self> {
        Ok(Self::from_bits(u32(data, offset)?))
    }
}

impl<T: Field, const N: usize> Field for [T; N] {
    const SIZE: usize = T::SIZE * N;
    fn read(data: &[u8], offset: usize) -> Result<Self> {
        let data = data
            .get(offset..)
            .and_then(|s| s.get(..Self::SIZE))
            .context("truncated record array")?;
        Ok((0..N)
            .map(|i| T::read(data, i * T::SIZE))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap_or_else(|_| unreachable!()))
    }
}

/// Declare fields once, using fixed offsets or expressions with a shared decode setup.
#[allow(unused_macros)] // Inspection tools include the scalar readers without records.
macro_rules! record {
    ($(#[$attr:meta])* $vis:vis struct $name:ident {
        $($(#[$field_attr:meta])* $field_vis:vis $field:ident: $ty:ty => $value:expr,)*
    } decode($($arg:ident: $arg_ty:ty),* $(,)?) { $($setup:tt)* }) => {
        #[derive(serde::Serialize, serde::Deserialize)]
        $(#[$attr])*
        $vis struct $name {
            $($(#[$field_attr])* $field_vis $field: $ty,)*
        }
        impl $name {
            $vis fn decode($($arg: $arg_ty),*) -> anyhow::Result<Self> {
                $($setup)*
                Ok(Self { $($field: $value,)* })
            }
        }
    };
    ($(#[$attr:meta])* $vis:vis struct $name:ident($size:expr) {
        $($(#[$field_attr:meta])* $field_vis:vis $field:ident: $ty:ty => $offset:expr,)*
    }) => {
        #[derive(serde::Serialize, serde::Deserialize)]
        $(#[$attr])*
        $vis struct $name {
            $($(#[$field_attr])* $field_vis $field: $ty,)*
        }
        impl $name {
            $vis fn read(bytes: &[u8]) -> anyhow::Result<Self> {
                let row = bytes.get(..$size)
                    .ok_or_else(|| anyhow::anyhow!("truncated {}", stringify!($name)))?;
                Ok(Self {
                    $($field: <$ty as $crate::read::Field>::read(row, $offset)?,)*
                })
            }
        }
        impl $crate::read::Field for $name {
            const SIZE: usize = $size;
            fn read(bytes: &[u8], offset: usize) -> anyhow::Result<Self> {
                Self::read(bytes.get(offset..).ok_or_else(|| anyhow::anyhow!("record outside buffer"))?)
            }
        }
    };
}
#[allow(unused_imports)] // Inspection examples also include this module.
pub(crate) use record;

pub(crate) fn c_string(data: &[u8], offset: usize) -> Result<&[u8]> {
    let bytes = data.get(offset..).context("string outside resource")?;
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated string")?;
    Ok(&bytes[..end])
}

/// Decode authored labels without losing byte identity or confusing escaped names.
pub(crate) fn label(bytes: &[u8]) -> String {
    let (decoded, invalid) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes);
    if invalid || encoding_rs::SHIFT_JIS.encode(&decoded).0.as_ref() != bytes {
        return bytes.escape_ascii().to_string();
    }
    let mut name = String::new();
    for character in decoded.chars() {
        if character.is_ascii() {
            name.extend([character as u8].escape_ascii().map(char::from));
        } else if character.is_control() {
            name.extend(character.escape_default());
        } else {
            name.push(character);
        }
    }
    name
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
