//! KTX2 files carry their dimensions, color space and mip levels together.
use anyhow::{Result, ensure};
use bevy::{
    image::{CompressedImageFormats, ImageLoaderSettings, ImageType},
    prelude::*,
};
#[cfg(test)]
mod tests;

pub(super) fn decode(
    path: &str,
    settings: &ImageLoaderSettings,
    read: &impl Fn(&str) -> Result<Vec<u8>>,
) -> Result<Image> {
    let bytes = read(path)?;
    ensure!(path.ends_with(".ktx2"), "textures must use KTX2: {path}");
    Ok(Image::from_buffer(
        &bytes,
        ImageType::Extension("ktx2"),
        CompressedImageFormats::NONE,
        settings.is_srgb,
        settings.sampler.clone(),
        settings.asset_usage,
    )?)
}
