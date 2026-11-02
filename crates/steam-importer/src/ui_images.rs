//! UI categories share the same DDS decoder and final KTX2 writer.
use crate::{output, source::Source, texture};
use anyhow::{Context, Result};
use image::{RgbaImage, imageops};
use resonance_content::font::UiTexture;
use std::path::Path;

pub(crate) fn read(source: &Source, name: &str) -> Result<RgbaImage> {
    let descriptor = texture::Descriptor::read(&source.require(name)?)
        .with_context(|| format!("UI image {name}"))?;
    let pixels = descriptor
        .decode(&source.require(&descriptor.payload)?)?
        .levels
        .remove(0);
    RgbaImage::from_raw(pixels.width, pixels.height, pixels.rgba)
        .context("invalid UI image dimensions")
}

pub(crate) fn write(output: &Path, path: &str, pixels: &RgbaImage) -> Result<UiTexture> {
    let (width, height) = pixels.dimensions();
    output::write(
        &output.join(path),
        &resonance_asset_writer::ktx2::encode_rgba8(width, height, pixels.as_raw())?,
    )?;
    Ok(UiTexture {
        path: path.into(),
        width,
        height,
    })
}

pub(crate) fn cook(
    source: &Source,
    output: &Path,
    name: &str,
    path: &str,
    half_size: bool,
) -> Result<UiTexture> {
    let pixels = read(source, name)?;
    let pixels = if half_size {
        imageops::resize(
            &pixels,
            (pixels.width() / 2).max(1),
            (pixels.height() / 2).max(1),
            imageops::FilterType::Triangle,
        )
    } else {
        pixels
    };
    write(output, path, &pixels)
}
