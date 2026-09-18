//! Shared lossless KTX2 profile for decoded GameCube textures.
mod encode;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

// Bump when the encoding profile, encoder version, or compression settings change.
pub(crate) const RECIPE: &str = "rgba8-linear-ktx2-structured-zstd-0.0.54-level9-v1";

pub(crate) fn cook(width: u32, height: u32, pixels: &[u8], output: &Path) -> Result<()> {
    let bytes = encode::encode_rgba8(width, height, pixels)
        .with_context(|| format!("encode texture {}", output.display()))?;
    crate::write_atomic(output, &bytes)
}

/// Scene exports keep editable PNGs alongside their glTF files.
pub(crate) fn cook_png(png: &Path, output: &Path) -> Result<()> {
    let image = image::open(png)
        .with_context(|| format!("read texture {}", png.display()))?
        .into_rgba8();
    cook(image.width(), image.height(), image.as_raw(), output)
}

pub(crate) fn fingerprint(width: u32, height: u32, pixels: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(RECIPE.as_bytes());
    hash.update(width.to_le_bytes());
    hash.update(height.to_le_bytes());
    hash.update(pixels);
    format!("{:x}", hash.finalize())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
