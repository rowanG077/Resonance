//! Bounded PNG pixels and explicit mip generation, shared by authoring and GPU preparation.
use anyhow::{Context, Result, ensure};
use image::{ImageDecoder, ImageEncoder};
use std::io::{Cursor, Read};

#[cfg(test)]
mod tests;

pub const MAX_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 16384;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl Pixels {
    pub fn validate(&self) -> Result<()> {
        let bytes = byte_count(self.width, self.height)?;
        ensure!(
            self.rgba.len() == bytes,
            "texture pixel count differs from its dimensions"
        );
        Ok(())
    }
    pub fn png(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes).write_image(
            &self.rgba,
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
        )?;
        Ok(bytes)
    }
}
fn byte_count(width: u32, height: u32) -> Result<usize> {
    ensure!(
        (1..=MAX_DIMENSION).contains(&width) && (1..=MAX_DIMENSION).contains(&height),
        "texture dimensions must be 1–16384 pixels"
    );
    let bytes = u64::from(width) * u64::from(height) * 4;
    ensure!(
        bytes <= MAX_BYTES as u64,
        "decoded texture exceeds pixel budget"
    );
    Ok(bytes as usize)
}

/// Decode the actual stored channels; color interpretation is explicit at the
/// texture resource, so embedded display profiles never rewrite native texels.
pub fn png(bytes: &[u8]) -> Result<Pixels> {
    ensure!(bytes.len() <= MAX_BYTES, "PNG exceeds encoded byte budget");
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_BYTES as u64);
    let decoder = image::codecs::png::PngDecoder::with_limits(Cursor::new(bytes), limits)?;
    let (width, height) = decoder.dimensions();
    byte_count(width, height)?;
    // Authoring is RGBA8, including exact imported pixels. Reject precision loss
    // instead of silently truncating a replacement with sixteen-bit channels.
    ensure!(
        decoder.color_type().bits_per_pixel() / u16::from(decoder.color_type().channel_count())
            <= 8,
        "texture PNGs require at most 8 bits per channel"
    );
    let rgba = image::DynamicImage::from_decoder(decoder)?
        .into_rgba8()
        .into_raw();
    Ok(Pixels {
        width,
        height,
        rgba,
    })
}

pub fn validate_levels(levels: &[Pixels]) -> Result<()> {
    let first = levels.first().context("texture has no image levels")?;
    ensure!(
        levels.len()
            <= (first
                .width
                .max(first.height)
                .checked_ilog2()
                .context("empty texture")?
                + 1) as usize,
        "texture has too many mip levels"
    );
    let mut total = 0;
    for (index, level) in levels.iter().enumerate() {
        level.validate()?;
        ensure!(
            (level.width, level.height)
                == (
                    (first.width >> index).max(1),
                    (first.height >> index).max(1)
                ),
            "mip {index} dimensions must be {}×{}",
            (first.width >> index).max(1),
            (first.height >> index).max(1)
        );
        total += level.rgba.len();
        ensure!(
            total <= MAX_BYTES,
            "complete mip chain exceeds decoded budget"
        );
    }
    Ok(())
}

/// This is opt-in replacement-art filtering. Imported authored levels never
/// pass through it. Average premultiplied linear color over each source footprint.
pub fn generate(base: Pixels, srgb: bool) -> Result<Vec<Pixels>> {
    base.validate()?;
    let mut levels = vec![base];
    let mut total = levels[0].rgba.len();
    while levels.last().unwrap().width > 1 || levels.last().unwrap().height > 1 {
        let source = levels.last().unwrap();
        let width = (source.width / 2).max(1);
        let height = (source.height / 2).max(1);
        let bytes = byte_count(width, height)?;
        total += bytes;
        ensure!(
            total <= MAX_BYTES,
            "generated mip chain exceeds decoded budget"
        );
        let mut rgba = Vec::with_capacity(bytes);
        for y in 0..height {
            for x in 0..width {
                let bounds = [
                    f64::from(x) * f64::from(source.width) / f64::from(width),
                    f64::from(x + 1) * f64::from(source.width) / f64::from(width),
                    f64::from(y) * f64::from(source.height) / f64::from(height),
                    f64::from(y + 1) * f64::from(source.height) / f64::from(height),
                ];
                let mut sums = [0.; 4];
                let area = (bounds[1] - bounds[0]) * (bounds[3] - bounds[2]);
                for sy in bounds[2].floor() as u32..(bounds[3].ceil() as u32).min(source.height) {
                    for sx in bounds[0].floor() as u32..(bounds[1].ceil() as u32).min(source.width)
                    {
                        let weight = (bounds[1].min(f64::from(sx + 1))
                            - bounds[0].max(f64::from(sx)))
                            * (bounds[3].min(f64::from(sy + 1)) - bounds[2].max(f64::from(sy)));
                        let offset = ((sy * source.width + sx) * 4) as usize;
                        let alpha = f64::from(source.rgba[offset + 3]) / 255.;
                        for channel in 0..3 {
                            let value = f64::from(source.rgba[offset + channel]) / 255.;
                            let linear = if srgb { to_linear(value) } else { value };
                            sums[channel] += linear * alpha * weight;
                        }
                        sums[3] += alpha * weight;
                    }
                }
                for value in &sums[..3] {
                    let value = if sums[3] > 0. { value / sums[3] } else { 0. };
                    let encoded = if srgb { from_linear(value) } else { value };
                    rgba.push((encoded.clamp(0., 1.) * 255.).round() as u8);
                }
                rgba.push((sums[3] / area * 255.).round() as u8);
            }
        }
        levels.push(Pixels {
            width,
            height,
            rgba,
        });
    }
    Ok(levels)
}
fn to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn from_linear(value: f64) -> f64 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    }
}

/// Recover current converter scratch pixels for terminal PNG publication.
/// This accepts only the lossless RGBA8 profile emitted by our KTX2 writer.
pub fn ktx2(bytes: &[u8]) -> Result<Vec<Pixels>> {
    use ktx2::{Format, SupercompressionScheme};
    let reader = ktx2::Reader::new(bytes)?;
    let header = reader.header();
    ensure!(
        header.format == Some(Format::R8G8B8A8_UNORM)
            && header.type_size == 1
            && header.pixel_depth == 0
            && header.layer_count == 0
            && header.face_count == 1,
        "expected converter RGBA8 KTX2 profile"
    );
    byte_count(header.pixel_width, header.pixel_height)?;
    let (basic, _) = ktx2::dfd::Basic::from_format(Format::R8G8B8A8_UNORM)?;
    ensure!(
        reader.dfd_blocks() == [ktx2::dfd::Block::Basic(basic)]
            && reader.supercompression_global_data().is_empty(),
        "unsupported converter texture color metadata"
    );
    let mut metadata_length = 0;
    for (key, value) in reader.key_value_data() {
        metadata_length += (4 + key.len() + 1 + value.len()).next_multiple_of(4);
        ensure!(
            match key {
                "KTXwriter" | "KTXwriterScParams" => true,
                "KTXorientation" => value == b"rd" || value == b"rd\0",
                "KTXswizzle" => value == b"rgba" || value == b"rgba\0",
                _ => false,
            },
            "unsupported converter texture metadata {key}"
        );
    }
    ensure!(
        metadata_length == header.index.kvd_byte_length as usize,
        "malformed converter texture metadata"
    );
    ensure!(
        (1..=header.pixel_width.max(header.pixel_height).ilog2() + 1).contains(&header.level_count),
        "invalid converter mip count"
    );
    let mut total = 0;
    let mut levels = Vec::new();
    for (index, level) in reader.levels().enumerate() {
        let width = (header.pixel_width >> index).max(1);
        let height = (header.pixel_height >> index).max(1);
        let size = byte_count(width, height)?;
        total += size;
        ensure!(
            total <= MAX_BYTES && level.uncompressed_byte_length == size as u64,
            "invalid converter mip length"
        );
        let rgba = match header.supercompression_scheme {
            None => level.data.to_vec(),
            Some(SupercompressionScheme::Zstandard) => {
                let mut decoder = ruzstd::decoding::StreamingDecoder::new(level.data)?;
                let mut rgba = Vec::new();
                (&mut decoder)
                    .take(size as u64 + 1)
                    .read_to_end(&mut rgba)?;
                ensure!(
                    decoder.into_inner().is_empty(),
                    "trailing converter compressed data"
                );
                rgba
            }
            _ => anyhow::bail!("unsupported converter texture compression"),
        };
        ensure!(rgba.len() == size, "truncated converter mip");
        levels.push(Pixels {
            width,
            height,
            rgba,
        });
    }
    validate_levels(&levels)?;
    Ok(levels)
}
