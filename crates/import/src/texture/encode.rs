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
}

/// Encode tightly packed, top-left-first RGBA8 pixels without changing any channel.
///
/// The profile is a single 2D mip, linear transfer, BT.709 primaries, straight alpha,
/// and Zstandard supercompression. KTX2's default orientation is right and down.
/// There is no variable metadata, so identical inputs produce identical bytes.
pub fn encode_rgba8(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if width == 0 || height == 0 {
        return Err(EncodeError::EmptyDimensions);
    }
    let expected = u64::from(width)
        .checked_mul(u64::from(height))
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

    let compressed = compress_slice_to_vec(pixels, CompressionLevel::from_level(9));
    let (basic, _) = dfd::Basic::from_format(Format::R8G8B8A8_UNORM)
        .expect("RGBA8 has a standard data format descriptor");
    let descriptor = dfd::Block::Basic(basic).to_vec();
    // KTX2's DFD totalSize includes its own four bytes, unlike descriptorBlockSize.
    let descriptor_length = 4 + descriptor.len();
    let descriptor_offset = Header::LENGTH + LevelIndex::LENGTH;
    let data_offset = descriptor_offset + descriptor_length;
    let header = Header {
        format: Some(Format::R8G8B8A8_UNORM),
        type_size: 1,
        pixel_width: width,
        pixel_height: height,
        pixel_depth: 0,
        layer_count: 0,
        face_count: 1,
        level_count: 1,
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
    let level = LevelIndex {
        byte_offset: data_offset as u64,
        byte_length: compressed.len() as u64,
        uncompressed_byte_length: pixels.len() as u64,
    };
    let capacity = data_offset
        .checked_add(compressed.len())
        .filter(|&size| size <= isize::MAX as usize)
        .ok_or(EncodeError::SizeOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(&header.as_bytes());
    bytes.extend_from_slice(&level.as_bytes());
    bytes.extend_from_slice(&(descriptor_length as u32).to_le_bytes());
    bytes.extend_from_slice(&descriptor);
    bytes.extend_from_slice(&compressed);
    Ok(bytes)
}
