//! Native BC7 cooking, and RGBA decoding for GPUs without it.
use anyhow::{Result, ensure};
use ktx2::{Format, Reader};

#[cfg(feature = "encode")]
mod cache;
#[cfg(all(test, feature = "encode"))]
mod tests;

/// Bytes in one BC7 block of four by four pixels.
pub(crate) const BLOCK_BYTES: usize = 16;

/// BC7 is the only GPU block format in a library.
pub(crate) fn is_bc7(format: Format) -> bool {
    matches!(format, Format::BC7_UNORM_BLOCK | Format::BC7_SRGB_BLOCK)
}

/// Whether a library texture stores BC7 blocks rather than RGBA8 pixels.
/// Any other format, or blocks that do not cover the image exactly, is an error.
pub fn is_native_compressed(bytes: &[u8]) -> Result<bool> {
    let header = Reader::new(bytes)?.header();
    match header.format {
        Some(format) if is_bc7(format) => {
            ensure!(
                header.pixel_width.is_multiple_of(4) && header.pixel_height.is_multiple_of(4),
                "BC7 texture dimensions must be multiples of four"
            );
            Ok(true)
        }
        Some(Format::R8G8B8A8_UNORM | Format::R8G8B8A8_SRGB) => Ok(false),
        format => anyhow::bail!("textures must be BC7 or RGBA8, not {format:?}"),
    }
}

/// Encode directly from final RGBA pixels, without intermediate image files.
#[cfg(feature = "encode")]
pub fn encode_rgba8(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    encode(&[crate::texture::Pixels {
        width,
        height,
        rgba: rgba.to_vec(),
    }])
}

/// Preserve authored mip artwork in a BC7 mip chain. Blocks cover four pixels;
/// an image of any other size is first padded or repeated by its converter
/// (see `Pixels`), which knows how its coordinates are addressed.
#[cfg(feature = "encode")]
pub fn encode(levels: &[crate::texture::Pixels]) -> Result<Vec<u8>> {
    crate::texture::validate_levels(levels)?;
    let first = &levels[0];
    ensure!(
        first.width.is_multiple_of(4) && first.height.is_multiple_of(4),
        "BC7 needs dimensions in multiples of four, not {}×{}; pad or repeat the image first",
        first.width,
        first.height
    );
    // Encoded blocks plus container metadata; cached storage stays bounded.
    let bytes = levels
        .iter()
        .map(|level| (level.width.div_ceil(4) * level.height.div_ceil(4) * 16) as usize)
        .sum::<usize>()
        + 1024;
    cache::get(crate::texture::fingerprint(levels), bytes, || {
        encode_uncached(levels)
    })
}

#[cfg(feature = "encode")]
fn encode_uncached(levels: &[crate::texture::Pixels]) -> Result<Vec<u8>> {
    use texture_compressor::{AlphaPolicy, Effort, Format, ImageView, Settings};
    let mut blocks = Vec::with_capacity(levels.len());
    for level in levels {
        let mut settings = Settings::new(Format::Bc7, Effort::Quality);
        if level.rgba.chunks_exact(4).any(|pixel| pixel[3] == 0) {
            settings = settings.with_alpha(AlphaPolicy::PreserveZeroAlpha);
        }
        let (w, h) = (level.width, level.height);
        // The encoder repeats edge pixels for partial mip blocks.
        let image = ImageView::rgba8(&level.rgba, w, h, w as usize * 4)?;
        blocks.push(texture_compressor::encode(image, &settings)?);
    }
    let refs = blocks.iter().map(Vec::as_slice).collect::<Vec<_>>();
    Ok(crate::ktx2::encode_bc7(
        levels[0].width,
        levels[0].height,
        &refs,
    )?)
}

/// Expand for an adapter without BC7.
pub fn rgba8(bytes: &[u8]) -> Result<Vec<u8>> {
    let levels = crate::texture::ktx2(bytes)?;
    let first = &levels[0];
    let pixels = levels.iter().map(|p| p.rgba.as_slice()).collect::<Vec<_>>();
    Ok(crate::ktx2::encode_raw_levels(
        first.width,
        first.height,
        &pixels,
    )?)
}

pub(crate) fn decode_level(width: u32, height: u32, bytes: &[u8]) -> Result<Vec<u8>> {
    let columns = width.div_ceil(4);
    ensure!(
        bytes.len() == (columns * height.div_ceil(4)) as usize * BLOCK_BYTES,
        "invalid GPU block payload"
    );
    let mut pixels = vec![0; (width * height * 4) as usize];
    for (index, block) in bytes.chunks_exact(BLOCK_BYTES).enumerate() {
        let (bx, by) = (index as u32 % columns * 4, index as u32 / columns * 4);
        let mut decoded = [0; 64];
        bcdec_rs::bc7(block, &mut decoded, 16);
        // Mips below one block keep only their own pixels.
        let length = 4.min(width - bx) as usize * 4;
        for y in 0..4.min(height - by) {
            let to = (((by + y) * width + bx) * 4) as usize;
            let from = y as usize * 16;
            pixels[to..to + length].copy_from_slice(&decoded[from..from + length]);
        }
    }
    Ok(pixels)
}
