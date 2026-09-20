//! GameCube TPL textures decoded into ordinary RGBA images.
pub(crate) use resonance_content::texture::{Filter, Sampler, TextureLod};
use serde::{Deserialize, Serialize};
use thiserror::Error;
#[derive(Debug, Error)]
pub enum TextureError {
    #[error("{0}")]
    Tpl(String),
}
#[derive(Debug, Clone)]
pub(crate) struct TplTexture {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) format: u32,
    pub(crate) data_offset: usize,
    pub(crate) palette_offset: Option<usize>,
    pub(crate) palette_entries: usize,
    pub(crate) palette_format: u32,
    pub(crate) wrap: [u32; 2],
    pub(crate) filter: [u32; 2],
    pub(crate) lod: TextureLod,
}

pub(crate) fn filter(value: u32) -> Result<Filter, TextureError> {
    Ok(match value {
        0 => Filter::Nearest,
        1 => Filter::Linear,
        2 => Filter::NearestMipmapNearest,
        3 => Filter::LinearMipmapNearest,
        4 => Filter::NearestMipmapLinear,
        5 => Filter::LinearMipmapLinear,
        _ => return Err(TextureError::Tpl(format!("invalid texture filter {value}"))),
    })
}

pub(crate) fn wrap(value: u32) -> Result<resonance_content::TextureWrap, TextureError> {
    Ok(match value {
        0 => resonance_content::TextureWrap::Clamp,
        1 => resonance_content::TextureWrap::Repeat,
        2 => resonance_content::TextureWrap::Mirror,
        _ => return Err(TextureError::Tpl(format!("invalid texture wrap {value}"))),
    })
}

impl TplTexture {
    pub(crate) fn sampler(&self) -> Result<Sampler, TextureError> {
        if self.filter[1] > 1 || !self.lod.bias.is_finite() || self.lod.min > self.lod.max {
            return Err(TextureError::Tpl("invalid texture sampler".into()));
        }
        Ok(Sampler {
            wrap: [wrap(self.wrap[0])?, wrap(self.wrap[1])?],
            min_filter: filter(self.filter[0])?,
            mag_filter: filter(self.filter[1])?,
            lod: self.lod,
        })
    }

    /// Authored levels are contiguous GX blocks, including padding in each level.
    pub(crate) fn levels(&self, data: &[u8]) -> Result<Vec<Self>, TextureError> {
        if self.width == 0 || self.height == 0 || self.width > 4096 || self.height > 4096 {
            return Err(TextureError::Tpl("invalid texture mip dimensions".into()));
        }
        let (bw, bh, size, _) = Format::from_code(self.format)?.block();
        let mut image = self.clone();
        let mut levels = Vec::new();
        // Equal LOD bounds disable mip storage in the source loader. Active
        // chains stop at 1x1 even when the sampler permits a larger LOD.
        let last = if self.lod.min == self.lod.max {
            0
        } else {
            u32::from(self.lod.max).min(self.width.max(self.height).ilog2())
        };
        for _ in 0..=last {
            let size = usize::from(image.width).div_ceil(bw)
                * usize::from(image.height).div_ceil(bh)
                * size;
            let end = image
                .data_offset
                .checked_add(size)
                .filter(|end| *end <= data.len())
                .ok_or_else(|| TextureError::Tpl("texture mip data exceeds file".into()))?;
            levels.push(image.clone());
            image.data_offset = end;
            image.width = (image.width / 2).max(1);
            image.height = (image.height / 2).max(1);
        }
        Ok(levels)
    }
}

pub(crate) fn parse_tpl(data: &[u8]) -> Result<Vec<TplTexture>, TextureError> {
    if data.len() < 12 || read_u32(data, 0) != Some(0x0020_AF30) {
        return Err(TextureError::Tpl("missing 0x0020AF30 header".into()));
    }
    let count = read_u32(data, 4).ok_or_else(|| TextureError::Tpl("short header".into()))? as usize;
    let table = read_u32(data, 8).ok_or_else(|| TextureError::Tpl("short header".into()))? as usize;
    let table_end = table
        .checked_add(
            count
                .checked_mul(8)
                .ok_or_else(|| TextureError::Tpl("descriptor overflow".into()))?,
        )
        .ok_or_else(|| TextureError::Tpl("descriptor overflow".into()))?;
    if table_end > data.len() {
        return Err(TextureError::Tpl("descriptor table exceeds file".into()));
    }
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let d = table + index * 8;
        let texture_offset = read_u32(data, d).unwrap() as usize;
        let palette_offset = read_u32(data, d + 4).unwrap() as usize;
        if texture_offset == 0
            || texture_offset
                .checked_add(0x24)
                .is_none_or(|end| end > data.len())
        {
            return Err(TextureError::Tpl(format!(
                "texture {index} header is outside file"
            )));
        }
        let width = read_u16(data, texture_offset + 2).unwrap();
        let height = read_u16(data, texture_offset).unwrap();
        let format = read_u32(data, texture_offset + 4).unwrap();
        let data_offset = read_u32(data, texture_offset + 8).unwrap() as usize;
        if data_offset >= data.len() {
            return Err(TextureError::Tpl(format!(
                "texture {index} data is outside file"
            )));
        }
        let (palette_offset, palette_entries, palette_format) = if palette_offset != 0 {
            if palette_offset
                .checked_add(12)
                .is_none_or(|end| end > data.len())
            {
                return Err(TextureError::Tpl(format!(
                    "texture {index} palette is outside file"
                )));
            }
            (
                Some(read_u32(data, palette_offset + 8).unwrap() as usize),
                usize::from(read_u16(data, palette_offset).unwrap()),
                read_u32(data, palette_offset + 4).unwrap(),
            )
        } else {
            (None, 0, 0)
        };
        out.push(TplTexture {
            width,
            height,
            format,
            data_offset,
            palette_offset,
            palette_entries,
            palette_format,
            wrap: [
                read_u32(data, texture_offset + 12).unwrap(),
                read_u32(data, texture_offset + 16).unwrap(),
            ],
            filter: [
                read_u32(data, texture_offset + 20).unwrap(),
                read_u32(data, texture_offset + 24).unwrap(),
            ],
            lod: TextureLod {
                bias: f32::from_bits(read_u32(data, texture_offset + 28).unwrap()),
                edge: data[texture_offset + 32] != 0,
                min: data[texture_offset + 33],
                max: data[texture_offset + 34],
            },
        });
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Format {
    I4,
    I8,
    Ia4,
    Ia8,
    Rgb565,
    Rgb5a3,
    Rgba8,
    Ci4,
    Ci8,
    Ci14,
    Cmpr,
}
impl Format {
    pub(crate) fn from_code(code: u32) -> Result<Self, TextureError> {
        Ok(match code {
            0 => Self::I4,
            1 => Self::I8,
            2 => Self::Ia4,
            3 => Self::Ia8,
            4 => Self::Rgb565,
            5 => Self::Rgb5a3,
            6 => Self::Rgba8,
            8 => Self::Ci4,
            9 => Self::Ci8,
            10 => Self::Ci14,
            14 => Self::Cmpr,
            _ => {
                return Err(TextureError::Tpl(format!(
                    "texture format {code} is not implemented"
                )));
            }
        })
    }
    fn block(self) -> (usize, usize, usize, &'static str) {
        match self {
            Self::I4 => (8, 8, 32, "I4"),
            Self::Ci4 => (8, 8, 32, "CI4"),
            Self::I8 => (8, 4, 32, "I8"),
            Self::Ia4 => (8, 4, 32, "IA4"),
            Self::Ci8 => (8, 4, 32, "CI8"),
            Self::Ia8 => (4, 4, 32, "IA8"),
            Self::Rgb565 => (4, 4, 32, "RGB565"),
            Self::Rgb5a3 => (4, 4, 32, "RGB5A3"),
            Self::Rgba8 => (4, 4, 64, "RGBA8"),
            Self::Ci14 => (4, 4, 32, "CI14"),
            Self::Cmpr => (8, 8, 32, "CMPR"),
        }
    }
    fn palette_limit(self) -> Option<usize> {
        match self {
            Self::Ci4 => Some(16),
            Self::Ci8 => Some(256),
            Self::Ci14 => Some(16384),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PaletteFormat {
    Ia8,
    Rgb565,
    Rgb5a3,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Palette {
    pub format: PaletteFormat,
    pub colors: Vec<[u8; 4]>,
}

/// Preserve the complete palette, including colors unused by visible texels.
pub(crate) fn palette(data: &[u8], texture: &TplTexture) -> Result<Option<Palette>, TextureError> {
    let Some(offset) = texture.palette_offset else {
        return Ok(None);
    };
    let format = match texture.palette_format {
        0 => PaletteFormat::Ia8,
        1 => PaletteFormat::Rgb565,
        2 => PaletteFormat::Rgb5a3,
        value => {
            return Err(TextureError::Tpl(format!(
                "unsupported palette format {value}"
            )));
        }
    };
    let entries = texture.palette_entries;
    if entries == 0
        || entries > 16384
        || offset
            .checked_add(entries * 2)
            .is_none_or(|end| end > data.len())
    {
        return Err(TextureError::Tpl("invalid texture palette".into()));
    }
    Ok(Some(Palette {
        format,
        colors: (0..entries)
            .map(|index| palette_pixel(read_u16(data, offset + index * 2).unwrap(), format))
            .collect(),
    }))
}

/// Effect bindings move the palette base without reducing its entry count.
/// A window can therefore reach the following data in the same resource.
pub(crate) fn decode_palette_window(
    data: &[u8],
    texture: &TplTexture,
    first_color: usize,
) -> Result<Vec<u8>, TextureError> {
    palette(data, texture)?
        .ok_or_else(|| TextureError::Tpl("indexed texture has no palette".into()))?;
    let capacity = Format::from_code(texture.format)?
        .palette_limit()
        .ok_or_else(|| TextureError::Tpl("palette window requires indexed texture".into()))?;
    let mut window = texture.clone();
    window.palette_offset = texture.palette_offset.and_then(|base| {
        first_color
            .checked_mul(2)
            .and_then(|offset| base.checked_add(offset))
    });
    window.palette_entries = texture.palette_entries.min(capacity);
    decode_texture(data, &window)
}

pub(crate) fn decode_texture(data: &[u8], texture: &TplTexture) -> Result<Vec<u8>, TextureError> {
    let width = usize::from(texture.width);
    let height = usize::from(texture.height);
    if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
        return Err(TextureError::Tpl(
            "texture dimensions exceed import limits".into(),
        ));
    }
    let format = Format::from_code(texture.format)?;
    let (block_width, block_height, block_bytes, label) = format.block();
    let mut palette = Vec::new();
    if let Some(limit) = format.palette_limit() {
        palette = self::palette(data, texture)?
            .ok_or_else(|| TextureError::Tpl(format!("{label} texture has no palette")))?
            .colors;
        // Shared tables may contain several palettes. An image's index width
        // selects the first palette; later colors do not change its pixels.
        palette.truncate(limit);
    }
    let columns = width.div_ceil(block_width);
    let rows = height.div_ceil(block_height);
    let size = columns * rows * block_bytes;
    let start = texture.data_offset;
    let bytes = start
        .checked_add(size)
        .and_then(|end| data.get(start..end))
        .ok_or_else(|| TextureError::Tpl(format!("{label} data exceeds file")))?;
    let mut rgba = vec![0; width * height * 4];
    for (index, block) in bytes.chunks_exact(block_bytes).enumerate() {
        let left = index % columns * block_width;
        let top = index / columns * block_height;
        let compressed: Option<[[[u8; 4]; 4]; 4]> = (format == Format::Cmpr).then(|| {
            std::array::from_fn(|sub| {
                dxt_colors(
                    read_u16(block, sub * 8).unwrap(),
                    read_u16(block, sub * 8 + 2).unwrap(),
                )
            })
        });
        for pixel in 0..block_width * block_height {
            let x = pixel % block_width;
            let y = pixel / block_width;
            // Padding texels have no authored image meaning and may contain junk indices.
            if left + x >= width || top + y >= height {
                continue;
            }
            let color = match format {
                Format::I4 => [expand4(u16::from(nibble(block, pixel))); 4],
                Format::I8 => [block[pixel]; 4],
                Format::Ia4 => {
                    let intensity = expand4(u16::from(block[pixel] & 15));
                    [
                        intensity,
                        intensity,
                        intensity,
                        expand4(u16::from(block[pixel] >> 4)),
                    ]
                }
                Format::Ia8 => {
                    palette_pixel(read_u16(block, pixel * 2).unwrap(), PaletteFormat::Ia8)
                }
                Format::Rgb565 => {
                    palette_pixel(read_u16(block, pixel * 2).unwrap(), PaletteFormat::Rgb565)
                }
                Format::Rgb5a3 => {
                    palette_pixel(read_u16(block, pixel * 2).unwrap(), PaletteFormat::Rgb5a3)
                }
                // RGBA8 stores separate alpha/red and green/blue planes per tile.
                Format::Rgba8 => [
                    block[pixel * 2 + 1],
                    block[32 + pixel * 2],
                    block[33 + pixel * 2],
                    block[pixel * 2],
                ],
                Format::Ci4 | Format::Ci8 | Format::Ci14 => {
                    let index = match format {
                        Format::Ci4 => usize::from(nibble(block, pixel)),
                        Format::Ci8 => usize::from(block[pixel]),
                        _ => usize::from(read_u16(block, pixel * 2).unwrap() & 0x3fff),
                    };
                    *palette.get(index).ok_or_else(|| TextureError::Tpl(format!("{label} texel references palette index {index}, but only {} colors are declared", palette.len())))?
                }
                Format::Cmpr => {
                    // An 8×8 tile contains four compressed 4×4 subtiles.
                    let sub = y / 4 * 2 + x / 4;
                    let bits = read_u32(block, sub * 8 + 4).unwrap();
                    let index = (bits >> (30 - (y % 4 * 4 + x % 4) * 2)) & 3;
                    compressed.as_ref().unwrap()[sub][index as usize]
                }
            };
            let at = ((top + y) * width + left + x) * 4;
            rgba[at..at + 4].copy_from_slice(&color);
        }
    }
    Ok(rgba)
}
fn nibble(data: &[u8], pixel: usize) -> u8 {
    let byte = data[pixel / 2];
    if pixel.is_multiple_of(2) {
        byte >> 4
    } else {
        byte & 15
    }
}

fn dxt_colors(c0: u16, c1: u16) -> [[u8; 4]; 4] {
    let a = palette_pixel(c0, PaletteFormat::Rgb565);
    let b = palette_pixel(c1, PaletteFormat::Rgb565);
    if c0 > c1 {
        [a, b, mix(a, b, 5, 3), mix(a, b, 3, 5)]
    } else {
        let average = mix(a, b, 1, 1);
        [a, b, average, [average[0], average[1], average[2], 0]]
    }
}

fn mix(a: [u8; 4], b: [u8; 4], na: u16, nb: u16) -> [u8; 4] {
    [
        ((u16::from(a[0]) * na + u16::from(b[0]) * nb) / (na + nb)) as u8,
        ((u16::from(a[1]) * na + u16::from(b[1]) * nb) / (na + nb)) as u8,
        ((u16::from(a[2]) * na + u16::from(b[2]) * nb) / (na + nb)) as u8,
        255,
    ]
}

fn palette_pixel(value: u16, format: PaletteFormat) -> [u8; 4] {
    match format {
        PaletteFormat::Ia8 => [value as u8, value as u8, value as u8, (value >> 8) as u8],
        PaletteFormat::Rgb5a3 => {
            if value & 0x8000 != 0 {
                [
                    expand5((value >> 10) & 0x1f),
                    expand5((value >> 5) & 0x1f),
                    expand5(value & 0x1f),
                    255,
                ]
            } else {
                [
                    expand4((value >> 8) & 0x0f),
                    expand4((value >> 4) & 0x0f),
                    expand4(value & 0x0f),
                    expand3(value >> 12),
                ]
            }
        }
        PaletteFormat::Rgb565 => [
            expand5((value >> 11) & 0x1f),
            expand6((value >> 5) & 0x3f),
            expand5(value & 0x1f),
            255,
        ],
    }
}

fn expand3(v: u16) -> u8 {
    ((v << 5) | (v << 2) | (v >> 1)) as u8
}
pub(crate) fn expand4(v: u16) -> u8 {
    ((v << 4) | v) as u8
}
pub(crate) fn expand5(v: u16) -> u8 {
    ((v << 3) | (v >> 2)) as u8
}
pub(crate) fn expand6(v: u16) -> u8 {
    ((v << 2) | (v >> 4)) as u8
}
pub(crate) fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)
        .map(|v| u16::from_be_bytes(v.try_into().unwrap()))
}
pub(crate) fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|v| u32::from_be_bytes(v.try_into().unwrap()))
}

/// Decode the images in a GameCube TPL into ordinary RGBA buffers.
pub fn decode(data: &[u8]) -> Result<Vec<(u32, u32, Vec<u8>)>, TextureError> {
    parse_tpl(data)?
        .iter()
        .map(|texture| {
            if texture.width == 0
                || texture.height == 0
                || texture.width > 4096
                || texture.height > 4096
            {
                return Err(TextureError::Tpl("invalid texture dimensions".into()));
            }
            Ok((
                u32::from(texture.width),
                u32::from(texture.height),
                decode_texture(data, texture)?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(format: u32) -> TplTexture {
        TplTexture {
            width: 1,
            height: 1,
            format,
            data_offset: 0,
            palette_offset: None,
            palette_entries: 0,
            palette_format: 0,
            wrap: [0; 2],
            filter: [1; 2],
            lod: TextureLod::default(),
        }
    }

    #[test]
    fn authored_mips_keep_block_padding_and_independent_pixels() {
        let mut descriptor = texture(1);
        descriptor.width = 16;
        descriptor.height = 4;
        descriptor.lod.max = 3;
        let mut data = vec![1; 64];
        for value in 2..=4 {
            data.extend([value; 32]);
        }
        let levels = descriptor.levels(&data).unwrap();
        assert_eq!(
            levels
                .iter()
                .map(|level| (level.width, level.height, level.data_offset))
                .collect::<Vec<_>>(),
            [(16, 4, 0), (8, 2, 64), (4, 1, 96), (2, 1, 128)]
        );
        for (index, level) in levels.iter().enumerate() {
            assert!(
                decode_texture(&data, level)
                    .unwrap()
                    .iter()
                    .all(|value| *value == index as u8 + 1)
            );
        }
        data.pop();
        assert!(descriptor.levels(&data).is_err());
        assert!(decode_texture(&data, &descriptor).is_ok());
        descriptor.lod.min = 3;
        assert_eq!(descriptor.levels(&data[..64]).unwrap().len(), 1);
        descriptor.width = 2;
        descriptor.height = 1;
        descriptor.lod.min = 0;
        descriptor.lod.max = 10;
        let levels = descriptor.levels(&data[..64]).unwrap();
        assert_eq!(levels.len(), 2);
        assert_eq!((levels[1].width, levels[1].height), (1, 1));
        assert_eq!(descriptor.sampler().unwrap().lod.max, 10);
        assert!(descriptor.levels(&data[..63]).is_err());
    }

    #[test]
    fn tpl_sampler_keeps_authored_lod_and_filter_modes() {
        let mut bytes = vec![0; 96];
        for (offset, value) in [
            (0, 0x20_af30_u32),
            (4, 1),
            (8, 12),
            (12, 20),
            (20, 0x0008_0008),
            (28, 64),
            (32, 2),
            (36, 1),
            (40, 5),
            (44, 1),
            (48, (-1.25_f32).to_bits()),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[52..55].copy_from_slice(&[1, 1, 3]);
        let descriptor = parse_tpl(&bytes).unwrap().remove(0);
        let sampler = serde_json::to_value(descriptor.sampler().unwrap()).unwrap();
        assert_eq!(sampler["wrap"], serde_json::json!(["mirror", "repeat"]));
        assert_eq!(sampler["min_filter"], "linear_mipmap_linear");
        assert_eq!(
            sampler["lod"],
            serde_json::json!({"bias": -1.25, "min": 1, "max": 3, "edge": true})
        );
        bytes[4..8].fill(0);
        assert!(parse_tpl(&bytes[..12]).unwrap().is_empty());
    }

    #[test]
    fn palette_checks_visible_indices_and_preserves_unused_colors() {
        let mut descriptor = texture(8);
        descriptor.palette_offset = Some(32);
        descriptor.palette_entries = 2;
        descriptor.palette_format = 2;
        let mut data = vec![255; 32];
        data.extend([255, 255, 252, 0]);
        assert!(decode_texture(&data, &descriptor).is_err());
        data[0] = 0x0f; // Only the first texel is visible; tile padding is ignored.
        assert_eq!(decode_texture(&data, &descriptor).unwrap(), [255; 4]);
        assert_eq!(
            palette(&data, &descriptor).unwrap().unwrap().colors,
            [[255; 4], [255, 0, 0, 255]]
        );
        descriptor.palette_format = 3;
        assert!(decode_texture(&data, &descriptor).is_err());
        descriptor.palette_format = 2;
        data.pop();
        assert!(palette(&data, &descriptor).is_err());
    }

    #[test]
    fn shifted_palette_keeps_index_capacity_and_reads_actual_following_colors() {
        let mut descriptor = texture(9);
        descriptor.width = 2;
        descriptor.palette_offset = Some(32);
        descriptor.palette_entries = 256;
        descriptor.palette_format = 1;
        let mut data = vec![0; 32 + (256 + 32) * 2];
        data[..2].copy_from_slice(&[0, 255]);
        data[32 + 32 * 2..34 + 32 * 2].copy_from_slice(&0xf800_u16.to_be_bytes());
        data[32 + 287 * 2..34 + 287 * 2].copy_from_slice(&0x07e0_u16.to_be_bytes());
        assert_eq!(
            decode_palette_window(&data, &descriptor, 32).unwrap(),
            [255, 0, 0, 255, 0, 255, 0, 255]
        );
        assert!(decode_palette_window(&data, &descriptor, 256).is_err());
        data.pop();
        assert!(decode_texture(&data, &descriptor).is_ok());
        assert!(decode_palette_window(&data, &descriptor, 32).is_err());
        // The native binding shifts the base without bounding it by the first
        // palette's count. A complete following palette is independently valid.
        data.resize(32 + 512 * 2, 0);
        data[32 + 256 * 2..34 + 256 * 2].copy_from_slice(&0xf800_u16.to_be_bytes());
        data[32 + 511 * 2..34 + 511 * 2].copy_from_slice(&0x07e0_u16.to_be_bytes());
        assert_eq!(
            decode_palette_window(&data, &descriptor, 256).unwrap(),
            [255, 0, 0, 255, 0, 255, 0, 255]
        );
        assert!(decode_palette_window(&data, &descriptor, 512).is_err());
        assert!(decode_palette_window(&data, &descriptor, usize::MAX).is_err());
    }

    #[test]
    fn intensity_and_alpha_channels_follow_gamecube_storage() {
        let block = [0xA3; 32];
        assert_eq!(decode_texture(&block, &texture(0)).unwrap(), [170; 4]);
        assert_eq!(decode_texture(&block, &texture(1)).unwrap(), [163; 4]);
        assert_eq!(
            decode_texture(&block, &texture(2)).unwrap(),
            [51, 51, 51, 170]
        );
    }

    #[test]
    fn compressed_transparent_pixels_keep_their_interpolated_rgb() {
        let colors = dxt_colors(0, 0xffff);
        assert_eq!(colors[2], [127, 127, 127, 255]);
        assert_eq!(colors[3], [127, 127, 127, 0]);
    }

    #[test]
    fn ci4_decodes_its_indices_from_a_larger_shared_palette() {
        let mut descriptor = texture(8);
        descriptor.width = 2;
        descriptor.palette_offset = Some(32);
        descriptor.palette_entries = 48;
        descriptor.palette_format = 1;
        let mut data = vec![0; 32 + 48 * 2];
        data[0] = 0x0f;
        data[32..34].copy_from_slice(&0xf800_u16.to_be_bytes());
        data[62..64].copy_from_slice(&0x07e0_u16.to_be_bytes());
        data[64..].fill(255);
        assert_eq!(
            decode_texture(&data, &descriptor).unwrap(),
            [255, 0, 0, 255, 0, 255, 0, 255]
        );
        // The complete declared table must exist, including unused palettes.
        data.pop();
        assert!(decode_texture(&data, &descriptor).is_err());
    }

    #[test]
    fn rgb5a3_opaque_green_uses_five_bits_without_red_leakage() {
        for (word, expected) in [
            (0x83E0u16, [0, 255, 0, 255]),
            (0xFC00, [255, 0, 0, 255]),
            (0x8400, [8, 0, 0, 255]),
            (0x3F12, [255, 17, 34, 109]),
        ] {
            let block = word.to_be_bytes().repeat(16);
            assert_eq!(decode_texture(&block, &texture(5)).unwrap(), expected);
        }
    }

    #[test]
    fn packed_channels_expand_by_bit_replication() {
        // Values where normalized rounding differs from the original decode.
        assert_eq!(expand5(3), 24);
        assert_eq!(expand5(28), 231);
        assert_eq!(expand6(11), 44);
        assert_eq!(expand6(52), 211);
    }

    #[test]
    fn malformed_textures_fail_before_large_allocations_or_indexing() {
        let mut descriptor = texture(0);
        descriptor.width = u16::MAX;
        descriptor.height = u16::MAX;
        assert!(decode_texture(&[], &descriptor).is_err());
        assert!(decode_texture(&[0; 31], &texture(0)).is_err());
        for length in 0..12 {
            assert!(parse_tpl(&vec![0; length]).is_err());
        }
    }
}
