//! GameCube TPL textures decoded into ordinary RGBA images.
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
        });
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
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
    fn from_code(code: u32) -> Result<Self, TextureError> {
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
                    "texture format {code} is not implemented (raw TPL is still preserved)"
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
            Self::Ci14 => Some(usize::MAX),
            _ => None,
        }
    }
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
        let offset = texture
            .palette_offset
            .ok_or_else(|| TextureError::Tpl(format!("{label} texture has no palette")))?;
        let entries = texture.palette_entries;
        if entries == 0
            || entries > 16384
            || entries
                .checked_mul(2)
                .and_then(|size| offset.checked_add(size))
                .is_none_or(|end| end > data.len())
        {
            return Err(TextureError::Tpl(format!("invalid {label} palette")));
        }
        // Shared tables may contain several palettes. An image's index width
        // selects the first palette; later colors do not change its pixels.
        palette = (0..entries.min(limit))
            .map(|index| {
                palette_pixel(
                    read_u16(data, offset + index * 2).unwrap(),
                    texture.palette_format,
                )
            })
            .collect();
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
                Format::Ia8 => palette_pixel(read_u16(block, pixel * 2).unwrap(), 0),
                Format::Rgb565 => palette_pixel(read_u16(block, pixel * 2).unwrap(), 1),
                Format::Rgb5a3 => palette_pixel(read_u16(block, pixel * 2).unwrap(), 2),
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
                    palette.get(index).copied().unwrap_or([0; 4])
                }
                Format::Cmpr => {
                    // An 8×8 tile contains four compressed 4×4 subtiles.
                    let sub = y / 4 * 2 + x / 4;
                    let bits = read_u32(block, sub * 8 + 4).unwrap();
                    let index = (bits >> (30 - (y % 4 * 4 + x % 4) * 2)) & 3;
                    compressed.as_ref().unwrap()[sub][index as usize]
                }
            };
            put_pixel(&mut rgba, width, height, left + x, top + y, color);
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
    let a = palette_pixel(c0, 1);
    let b = palette_pixel(c1, 1);
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

fn palette_pixel(value: u16, format: u32) -> [u8; 4] {
    match format {
        0 => [value as u8, value as u8, value as u8, (value >> 8) as u8],
        2 => {
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
        _ => [
            expand5((value >> 11) & 0x1f),
            expand6((value >> 5) & 0x3f),
            expand5(value & 0x1f),
            255,
        ],
    }
}

fn put_pixel(out: &mut [u8], width: usize, height: usize, x: usize, y: usize, pixel: [u8; 4]) {
    if x < width && y < height {
        out[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(&pixel);
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
        }
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
