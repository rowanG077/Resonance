//! Byte-only texture encoding. No filesystem, process, clock, or thread services.
extern crate alloc;

use alloc::vec::Vec;
use ktx2::{Format, Header, Index, LevelIndex, SupercompressionScheme, dfd};
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
    if width == 0 || height == 0 {
        return Err(EncodeError::EmptyDimensions);
    }
    if pixels.is_empty() || pixels.len() > (width.max(height).ilog2() + 1) as usize {
        return Err(EncodeError::LevelCount);
    }
    let mut compressed = Vec::with_capacity(pixels.len());
    for (level, pixels) in pixels.iter().enumerate() {
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

        compressed.push(compress_slice_to_vec(
            pixels,
            CompressionLevel::from_level(9),
        ));
    }
    let (basic, _) = dfd::Basic::from_format(Format::R8G8B8A8_UNORM)
        .expect("RGBA8 has a standard data format descriptor");
    let descriptor = dfd::Block::Basic(basic).to_vec();
    // KTX2's DFD totalSize includes its own four bytes, unlike descriptorBlockSize.
    let descriptor_length = 4 + descriptor.len();
    let descriptor_offset = Header::LENGTH + pixels.len() * LevelIndex::LENGTH;
    let data_offset = descriptor_offset + descriptor_length;
    let header = Header {
        format: Some(Format::R8G8B8A8_UNORM),
        type_size: 1,
        pixel_width: width,
        pixel_height: height,
        pixel_depth: 0,
        layer_count: 0,
        face_count: 1,
        level_count: pixels.len() as u32,
        supercompression_scheme: Some(SupercompressionScheme::Zstandard),
        index: Index {
            dfd_byte_offset: descriptor_offset as u32,
            dfd_byte_length: descriptor_length as u32,
            kvd_byte_offset: 0,
            kvd_byte_length: 0,
            sgd_byte_offset: 0,
            sgd_byte_length: 0,
        },
    };
    // KTX2 stores the smallest level first while its index stays base-first.
    let mut levels = Vec::with_capacity(pixels.len());
    let mut capacity = data_offset;
    for (pixels, bytes) in pixels.iter().zip(&compressed).rev() {
        levels.push(LevelIndex {
            byte_offset: capacity as u64,
            byte_length: bytes.len() as u64,
            uncompressed_byte_length: pixels.len() as u64,
        });
        capacity = capacity
            .checked_add(bytes.len())
            .filter(|&size| size <= isize::MAX as usize)
            .ok_or(EncodeError::SizeOverflow)?;
    }
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(&header.as_bytes());
    for level in levels.iter().rev() {
        bytes.extend_from_slice(&level.as_bytes());
    }
    bytes.extend_from_slice(&(descriptor_length as u32).to_le_bytes());
    bytes.extend_from_slice(&descriptor);
    for level in compressed.iter().rev() {
        bytes.extend_from_slice(level);
    }
    Ok(bytes)
}
