//! Byte-only texture encoding. No filesystem, process, clock, or thread services.
extern crate alloc;

use alloc::vec::Vec;
pub use ktx2::Format;
use ktx2::{Header, Index, LevelIndex, SupercompressionScheme, dfd};
use structured_zstd::encoding::{CompressionLevel, compress_slice_to_vec};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EncodeError {
    #[error("texture dimensions must be nonzero")]
    EmptyDimensions,
    #[error("texture dimensions exceed the addressable byte length")]
    SizeOverflow,
    #[error("expected {expected} RGBA8 bytes, got {actual}")]
    PixelLength { expected: usize, actual: usize },
    #[error("invalid original mip level count")]
    LevelCount,
    #[error("BC7 needs block-sized dimensions and complete block payloads")]
    BlockFormat,
}

/// Encode tightly packed, top-left-first RGBA8 pixels without changing any channel.
///
/// The profile is a single 2D mip, linear transfer, BT.709 primaries, straight alpha,
/// and Zstandard supercompression. KTX2's default orientation is right and down.
/// There is no variable metadata, so identical inputs produce identical bytes.
pub fn encode_rgba8(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, EncodeError> {
    encode_levels(width, height, &[pixels])
}

/// Preserve supplied mip pixels exactly, without filtering or generating levels.
pub fn encode_levels(width: u32, height: u32, pixels: &[&[u8]]) -> Result<Vec<u8>, EncodeError> {
    validate_rgba8(width, height, pixels)?;
    let compressed = pixels
        .iter()
        .map(|pixels| compress_slice_to_vec(pixels, CompressionLevel::from_level(9)))
        .collect::<Vec<_>>();
    let refs = compressed.iter().map(Vec::as_slice).collect::<Vec<_>>();
    encode_data(
        Format::R8G8B8A8_UNORM,
        width,
        height,
        &refs,
        &lengths(pixels),
        Some(SupercompressionScheme::Zstandard),
    )
}

/// Uncompressed RGBA for an adapter without BC7, including the original mip chain.
pub fn encode_raw_levels(
    width: u32,
    height: u32,
    levels: &[&[u8]],
) -> Result<Vec<u8>, EncodeError> {
    validate_rgba8(width, height, levels)?;
    encode_data(
        Format::R8G8B8A8_UNORM,
        width,
        height,
        levels,
        &lengths(levels),
        None,
    )
}

/// Store GPU-ready BC7 blocks directly, without CPU supercompression.
pub fn encode_bc7(width: u32, height: u32, levels: &[&[u8]]) -> Result<Vec<u8>, EncodeError> {
    if width == 0 || height == 0 || !width.is_multiple_of(4) || !height.is_multiple_of(4) {
        return Err(EncodeError::BlockFormat);
    }
    validate_level_count(width, height, levels)?;
    for (index, data) in levels.iter().enumerate() {
        let expected = ((width >> index).max(1).div_ceil(4) as u64)
            * ((height >> index).max(1).div_ceil(4) as u64)
            * crate::gpu::BLOCK_BYTES as u64;
        if data.len() as u64 != expected {
            return Err(EncodeError::BlockFormat);
        }
    }
    encode_data(
        Format::BC7_UNORM_BLOCK,
        width,
        height,
        levels,
        &lengths(levels),
        None,
    )
}

fn lengths(levels: &[&[u8]]) -> Vec<u64> {
    levels.iter().map(|level| level.len() as u64).collect()
}

fn validate_level_count(width: u32, height: u32, levels: &[&[u8]]) -> Result<(), EncodeError> {
    if width == 0 || height == 0 {
        return Err(EncodeError::EmptyDimensions);
    }
    if levels.is_empty() || levels.len() > (width.max(height).ilog2() + 1) as usize {
        return Err(EncodeError::LevelCount);
    }
    Ok(())
}

fn validate_rgba8(width: u32, height: u32, levels: &[&[u8]]) -> Result<(), EncodeError> {
    validate_level_count(width, height, levels)?;
    for (level, pixels) in levels.iter().enumerate() {
        let expected = u64::from((width >> level).max(1))
            .checked_mul(u64::from((height >> level).max(1)))
            .and_then(|size| size.checked_mul(4))
            .and_then(|size| usize::try_from(size).ok())
            .filter(|&size| size <= isize::MAX as usize)
            .ok_or(EncodeError::SizeOverflow)?;
        if pixels.len() != expected {
            return Err(EncodeError::PixelLength {
                expected,
                actual: pixels.len(),
            });
        }
    }
    Ok(())
}

fn encode_data(
    format: Format,
    width: u32,
    height: u32,
    levels: &[&[u8]],
    raw_lengths: &[u64],
    scheme: Option<SupercompressionScheme>,
) -> Result<Vec<u8>, EncodeError> {
    let (basic, _) = dfd::Basic::from_format(format).map_err(|_| EncodeError::BlockFormat)?;
    let descriptor = dfd::Block::Basic(basic).to_vec();
    let prefix = Header::LENGTH + levels.len() * LevelIndex::LENGTH;
    let mut bytes = vec![0; prefix];
    bytes.extend_from_slice(&((descriptor.len() + 4) as u32).to_le_bytes());
    bytes.extend_from_slice(&descriptor);
    let header = Header {
        format: Some(format),
        type_size: 1,
        pixel_width: width,
        pixel_height: height,
        pixel_depth: 0,
        layer_count: 0,
        face_count: 1,
        level_count: levels.len() as u32,
        supercompression_scheme: scheme,
        index: Index {
            dfd_byte_offset: prefix as u32,
            dfd_byte_length: (descriptor.len() + 4) as u32,
            kvd_byte_offset: 0,
            kvd_byte_length: 0,
            sgd_byte_offset: 0,
            sgd_byte_length: 0,
        },
    };
    bytes[..Header::LENGTH].copy_from_slice(&header.as_bytes());
    let alignment = match scheme {
        Some(_) => 1,
        None if crate::gpu::is_bc7(format) => crate::gpu::BLOCK_BYTES,
        None => 4,
    };
    for index in (0..levels.len()).rev() {
        bytes.resize(bytes.len().next_multiple_of(alignment), 0);
        let level = LevelIndex {
            byte_offset: bytes.len() as u64,
            byte_length: levels[index].len() as u64,
            uncompressed_byte_length: raw_lengths[index],
        };
        let offset = Header::LENGTH + index * LevelIndex::LENGTH;
        bytes[offset..offset + LevelIndex::LENGTH].copy_from_slice(&level.as_bytes());
        bytes.extend_from_slice(levels[index]);
    }
    Ok(bytes)
}
