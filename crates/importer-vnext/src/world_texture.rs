//! Native world TPL images directly into editable KTX2 texture levels.
use crate::{dol, world_terrain_collision::section};
use anyhow::{Context, Result, ensure};
use resonance_asset_writer::texture::Pixels;
use std::{fs, path::Path};

fn rgb565(value: u16) -> [u8; 4] {
    let five = |value: u16| ((value << 3) | (value >> 2)) as u8;
    let six = |value: u16| ((value << 2) | (value >> 4)) as u8;
    [
        five(value >> 11),
        six((value >> 5) & 63),
        five(value & 31),
        255,
    ]
}

fn rgb5a3(value: u16) -> [u8; 4] {
    let three = |value: u16| ((value << 5) | (value << 2) | (value >> 1)) as u8;
    let four = |value: u16| ((value << 4) | value) as u8;
    let five = |value: u16| ((value << 3) | (value >> 2)) as u8;
    if value & 0x8000 != 0 {
        [
            five((value >> 10) & 31),
            five((value >> 5) & 31),
            five(value & 31),
            255,
        ]
    } else {
        [
            four((value >> 8) & 15),
            four((value >> 4) & 15),
            four(value & 15),
            three(value >> 12),
        ]
    }
}

fn blend(a: [u8; 4], b: [u8; 4], na: u16, nb: u16) -> [u8; 4] {
    [
        ((u16::from(a[0]) * na + u16::from(b[0]) * nb) / (na + nb)) as u8,
        ((u16::from(a[1]) * na + u16::from(b[1]) * nb) / (na + nb)) as u8,
        ((u16::from(a[2]) * na + u16::from(b[2]) * nb) / (na + nb)) as u8,
        255,
    ]
}

fn cmpr_palette(block: &[u8]) -> Result<[[u8; 4]; 4]> {
    let c0 = dol::u16(block, 0)?;
    let c1 = dol::u16(block, 2)?;
    let a = rgb565(c0);
    let b = rgb565(c1);
    Ok(if c0 > c1 {
        [a, b, blend(a, b, 5, 3), blend(a, b, 3, 5)]
    } else {
        let average = blend(a, b, 1, 1);
        [a, b, average, [average[0], average[1], average[2], 0]]
    })
}

fn image(
    tpl: &[u8],
    width: usize,
    height: usize,
    format: u32,
    start: usize,
    palette: &[[u8; 4]],
) -> Result<(Vec<u8>, usize)> {
    let (block_width, block_height, block_bytes) = match format {
        0 | 8 | 14 => (8, 8, 32),
        1 | 2 | 9 => (8, 4, 32),
        3..=5 | 10 => (4, 4, 32),
        6 => (4, 4, 64),
        _ => anyhow::bail!("unsupported terrain texture format {format}"),
    };
    let columns = width.div_ceil(block_width);
    let rows = height.div_ceil(block_height);
    let size = columns * rows * block_bytes;
    let bytes = tpl
        .get(start..start + size)
        .context("truncated terrain texture")?;
    let mut rgba = vec![0; width * height * 4];
    for (index, block) in bytes.chunks_exact(block_bytes).enumerate() {
        let left = index % columns * block_width;
        let top = index / columns * block_height;
        let palettes = if format == 14 {
            Some(
                (0..4)
                    .map(|sub| cmpr_palette(&block[sub * 8..sub * 8 + 8]))
                    .collect::<Result<Vec<_>>>()?,
            )
        } else {
            None
        };
        for y in 0..block_height {
            for x in 0..block_width {
                if left + x >= width || top + y >= height {
                    continue;
                }
                let color = if format == 0 {
                    let packed = block[y * 4 + x / 2];
                    let intensity = if x % 2 == 0 { packed >> 4 } else { packed & 15 };
                    let value = (intensity << 4) | intensity;
                    [value; 4]
                } else if format == 1 {
                    [block[y * 8 + x]; 4]
                } else if format == 2 {
                    let value = block[y * 8 + x];
                    let intensity = (value & 15) * 17;
                    [intensity, intensity, intensity, (value >> 4) * 17]
                } else if format == 8 {
                    let packed = block[y * 4 + x / 2];
                    let index = if x % 2 == 0 { packed >> 4 } else { packed & 15 };
                    *palette
                        .get(usize::from(index))
                        .context("indexed texture exceeds its palette")?
                } else if format == 9 {
                    *palette
                        .get(usize::from(block[y * 8 + x]))
                        .context("indexed texture exceeds its palette")?
                } else if format == 10 {
                    *palette
                        .get(usize::from(dol::u16(block, (y * 4 + x) * 2)? & 0x3fff))
                        .context("CI14 texture palette index")?
                } else if format == 3 {
                    let intensity = block[(y * 4 + x) * 2 + 1];
                    [intensity, intensity, intensity, block[(y * 4 + x) * 2]]
                } else if format == 6 {
                    let pixel = (y * 4 + x) * 2;
                    [
                        block[pixel + 1],
                        block[32 + pixel],
                        block[33 + pixel],
                        block[pixel],
                    ]
                } else if let Some(palettes) = &palettes {
                    let sub = y / 4 * 2 + x / 4;
                    let bits = dol::u32(block, sub * 8 + 4)?;
                    let select = (bits >> (30 - (y % 4 * 4 + x % 4) * 2)) & 3;
                    palettes[sub][select as usize]
                } else if format == 4 {
                    rgb565(dol::u16(block, (y * 4 + x) * 2)?)
                } else {
                    rgb5a3(dol::u16(block, (y * 4 + x) * 2)?)
                };
                let at = ((top + y) * width + left + x) * 4;
                rgba[at..at + 4].copy_from_slice(&color);
            }
        }
    }
    Ok((rgba, start + size))
}

fn tpl(tile: &[u8], layer: usize) -> Result<&[u8]> {
    ensure!(
        matches!(layer, 0 | 2),
        "terrain texture layer must be 0 or 2"
    );
    let layer = section(tile, layer)?;
    let start = dol::u32(layer, 0)? as usize;
    let end = dol::u32(layer, 4)? as usize;
    let tpl = layer.get(start..end).context("terrain TPL exceeds model")?;
    ensure!(
        dol::u32(tpl, 0)? == 0x0020_af30,
        "missing terrain TPL header"
    );
    Ok(tpl)
}

pub(crate) fn texture_count(tile: &[u8], layer: usize) -> Result<usize> {
    Ok(dol::u32(tpl(tile, layer)?, 4)? as usize)
}

pub(crate) fn tpl_texture(tpl: &[u8], index: usize) -> Result<(Vec<u8>, bool)> {
    tpl_texture_palette(tpl, index, 0)
}

/// Decode an indexed TPL image using a window within its native palette.
/// Effect atlases reuse the same pixel indices with several color windows.
pub(crate) fn tpl_texture_palette(
    tpl: &[u8],
    index: usize,
    palette_offset: usize,
) -> Result<(Vec<u8>, bool)> {
    tpl_texture_palette_window(tpl, index, palette_offset, false)
}

/// Some effect actors explicitly select a full palette window beginning near
/// the declared palette tail. Only that explicit binding may read the trailing
/// bytes; ordinary atlas enumeration stays within the declared entries.
pub(crate) fn tpl_texture_palette_explicit(
    tpl: &[u8],
    index: usize,
    palette_offset: usize,
) -> Result<(Vec<u8>, bool)> {
    tpl_texture_palette_window(tpl, index, palette_offset, true)
}

fn tpl_texture_palette_window(
    tpl: &[u8],
    index: usize,
    palette_offset: usize,
    explicit_tail: bool,
) -> Result<(Vec<u8>, bool)> {
    let levels = tpl_pixels(tpl, index, palette_offset, explicit_tail, true)?;
    let first = &levels[0];
    let alpha = first.rgba.chunks_exact(4).any(|pixel| pixel[3] < 255);
    let references = levels
        .iter()
        .map(|level| level.rgba.as_slice())
        .collect::<Vec<_>>();
    Ok((
        resonance_asset_writer::ktx2::encode_levels(first.width, first.height, &references)?,
        alpha,
    ))
}

/// Menu atlases use only the native base image; do not encode and decode KTX.
pub(crate) fn tpl_base_image(tpl: &[u8], index: usize) -> Result<Pixels> {
    Ok(tpl_pixels(tpl, index, 0, false, false)?.remove(0))
}

fn tpl_pixels(
    tpl: &[u8],
    index: usize,
    palette_offset: usize,
    explicit_tail: bool,
    all_levels: bool,
) -> Result<Vec<Pixels>> {
    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "invalid TPL header");
    let count = dol::u32(tpl, 4)? as usize;
    ensure!(index < count, "terrain texture index exceeds TPL");
    let descriptor = dol::u32(tpl, dol::u32(tpl, 8)? as usize + index * 8)? as usize;
    let width = usize::from(dol::u16(tpl, descriptor + 2)?);
    let height = usize::from(dol::u16(tpl, descriptor)?);
    ensure!(
        (1..=4096).contains(&width) && (1..=4096).contains(&height),
        "invalid terrain texture dimensions"
    );
    let format = dol::u32(tpl, descriptor + 4)?;
    let palette = if matches!(format, 8..=10) {
        let table = dol::u32(tpl, 8)? as usize;
        let header = dol::u32(tpl, table + index * 8 + 4)? as usize;
        ensure!(header != 0, "indexed texture has no palette");
        let entries = usize::from(dol::u16(tpl, header)?);
        ensure!(
            explicit_tail || palette_offset < entries,
            "indexed texture palette window exceeds its table"
        );
        let capacity = match format {
            8 => 16,
            9 => 256,
            _ => 16384,
        };
        let count = if explicit_tail {
            capacity
        } else {
            capacity.min(entries - palette_offset)
        };
        let kind = dol::u32(tpl, header + 4)?;
        let start = dol::u32(tpl, header + 8)? as usize;
        if explicit_tail {
            let bytes = palette_offset
                .checked_add(capacity)
                .and_then(|entries| entries.checked_mul(2))
                .context("indexed palette offset overflows")?;
            let end = start
                .checked_add(bytes)
                .context("indexed palette window overflows")?;
            ensure!(end <= tpl.len(), "indexed palette window exceeds TPL bytes");
        }
        (0..count)
            .map(|index| {
                let value = dol::u16(tpl, start + (palette_offset + index) * 2)?;
                Ok(match kind {
                    0 => [value as u8, value as u8, value as u8, (value >> 8) as u8],
                    1 => rgb565(value),
                    2 => rgb5a3(value),
                    _ => anyhow::bail!("unsupported indexed texture palette {kind}"),
                })
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        ensure!(
            palette_offset == 0,
            "unindexed texture has no palette windows"
        );
        Vec::new()
    };
    let mut offset = dol::u32(tpl, descriptor + 8)? as usize;
    let min_lod = *tpl.get(descriptor + 33).context("missing texture LOD")?;
    let max_lod = *tpl.get(descriptor + 34).context("missing texture LOD")?;
    let last = if !all_levels || min_lod == max_lod {
        0
    } else {
        u32::from(max_lod).min(width.max(height).ilog2())
    };
    let mut levels = Vec::new();
    for level in 0..=last {
        let w = (width >> level).max(1);
        let h = (height >> level).max(1);
        let (rgba, end) = image(tpl, w, h, format, offset, &palette)?;
        levels.push(Pixels {
            width: w as u32,
            height: h as u32,
            rgba,
        });
        offset = end;
    }
    Ok(levels)
}

pub(crate) fn tpl_palette_entries(tpl: &[u8], index: usize) -> Result<usize> {
    let count = dol::u32(tpl, 4)? as usize;
    ensure!(index < count, "texture index exceeds TPL");
    let table = dol::u32(tpl, 8)? as usize;
    let header = dol::u32(tpl, table + index * 8 + 4)? as usize;
    if header == 0 {
        return Ok(0);
    }
    Ok(usize::from(dol::u16(tpl, header)?))
}

pub(crate) fn tpl_palette_capacity(tpl: &[u8], index: usize) -> Result<usize> {
    let count = dol::u32(tpl, 4)? as usize;
    ensure!(index < count, "texture index exceeds TPL");
    let table = dol::u32(tpl, 8)? as usize;
    let descriptor = dol::u32(tpl, table + index * 8)? as usize;
    Ok(match dol::u32(tpl, descriptor + 4)? {
        8 => 16,
        9 => 256,
        _ => 0,
    })
}

pub(crate) fn texture(tile: &[u8], layer: usize, index: usize) -> Result<(Vec<u8>, bool)> {
    tpl_texture(tpl(tile, layer)?, index)
}

pub fn cook_world_terrain_texture(
    source: &Path,
    output: &Path,
    layer: usize,
    index: usize,
) -> Result<()> {
    ensure!(!output.exists(), "world texture output already exists");
    let (bytes, _) = texture(&fs::read(source)?, layer, index)?;
    crate::output::write(&output, &bytes)?;
    Ok(())
}

pub(crate) fn sampler(
    tpl: &[u8],
    descriptor: usize,
) -> Result<resonance_content::texture::Sampler> {
    use resonance_content::{
        TextureWrap,
        texture::{Filter, Sampler, TextureLod},
    };
    let wrap = |value| -> Result<TextureWrap> {
        Ok(match value {
            0 => TextureWrap::Clamp,
            1 => TextureWrap::Repeat,
            2 => TextureWrap::Mirror,
            _ => anyhow::bail!("invalid texture wrap {value}"),
        })
    };
    let filter = |value| -> Result<Filter> {
        Ok(match value {
            0 => Filter::Nearest,
            1 => Filter::Linear,
            2 => Filter::NearestMipmapNearest,
            3 => Filter::LinearMipmapNearest,
            4 => Filter::NearestMipmapLinear,
            5 => Filter::LinearMipmapLinear,
            _ => anyhow::bail!("invalid texture filter {value}"),
        })
    };
    let sampler = Sampler {
        wrap: [
            wrap(dol::u32(tpl, descriptor + 12)?)?,
            wrap(dol::u32(tpl, descriptor + 16)?)?,
        ],
        min_filter: filter(dol::u32(tpl, descriptor + 20)?)?,
        mag_filter: filter(dol::u32(tpl, descriptor + 24)?)?,
        lod: TextureLod {
            bias: f32::from_bits(dol::u32(tpl, descriptor + 28)?),
            edge: *tpl
                .get(descriptor + 32)
                .context("texture LOD is truncated")?
                != 0,
            min: *tpl
                .get(descriptor + 33)
                .context("texture LOD is truncated")?,
            max: *tpl
                .get(descriptor + 34)
                .context("texture LOD is truncated")?,
        },
    };
    sampler.validate()?;
    Ok(sampler)
}
