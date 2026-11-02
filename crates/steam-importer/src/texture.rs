//! Steam's length-prefixed DDS images become the shared RGBA8 KTX2 profile.
//! Authored mip levels, BC1 transparency and BC2/BC3 straight alpha survive.
use super::archive::u32 as word;
use anyhow::{Context, Result, bail, ensure};
use resonance_asset_writer::texture::{MAX_BYTES, MAX_DIMENSION, Pixels};

mod descriptor;
pub use descriptor::Descriptor;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    Bc1,
    Bc2,
    Bc3,
}

impl Compression {
    fn block_size(self) -> usize {
        if self == Self::Bc1 { 8 } else { 16 }
    }
}

pub struct Decoded {
    pub levels: Vec<Pixels>,
}

impl Decoded {
    pub fn ktx2(&self) -> Result<Vec<u8>> {
        resonance_asset_writer::texture::validate_levels(&self.levels)?;
        Ok(resonance_asset_writer::ktx2::encode_levels(
            self.levels[0].width,
            self.levels[0].height,
            &self
                .levels
                .iter()
                .map(|level| level.rgba.as_slice())
                .collect::<Vec<_>>(),
        )?)
    }
}

/// Decode a TOTEXP_P resource, including its native length prefix. Descriptors
/// and material samplers are separate resources and cannot be inferred here.
pub fn decode(bytes: &[u8]) -> Result<Decoded> {
    ensure!(
        word(bytes, 0)? as usize == bytes.len() - 4,
        "Steam DDS length mismatch"
    );
    let bytes = &bytes[4..];
    ensure!(
        bytes.len() >= 128 && bytes.starts_with(b"DDS "),
        "invalid Steam DDS"
    );
    ensure!(
        word(bytes, 4)? == 124 && word(bytes, 76)? == 32,
        "invalid DDS header size"
    );
    let flags = word(bytes, 8)?;
    ensure!(flags & 0x1007 == 0x1007, "missing DDS texture fields");
    let width = word(bytes, 16)?;
    let height = word(bytes, 12)?;
    // Bound allocation independently of the caller's archive limits.
    ensure!(
        (1..=MAX_DIMENSION).contains(&width) && (1..=MAX_DIMENSION).contains(&height),
        "DDS dimensions exceed limits"
    );
    ensure!(
        word(bytes, 24)? == 0
            && word(bytes, 112)? == 0
            && word(bytes, 116)? == 0
            && word(bytes, 120)? == 0
            && flags & 0x800000 == 0,
        "unsupported DDS volume, cube or complex surface"
    );
    ensure!(word(bytes, 108)? & 0x1000 != 0, "DDS is not a texture");
    ensure!(
        word(bytes, 80)? == 4 && bytes[88..108].iter().all(|&b| b == 0),
        "unsupported DDS pixel format"
    );
    let compression = match &bytes[84..88] {
        b"DXT1" => Compression::Bc1,
        b"DXT3" => Compression::Bc2,
        b"DXT5" => Compression::Bc3,
        other => bail!("unsupported DDS compression {other:?}"),
    };
    let authored = word(bytes, 28)?;
    let count = if flags & 0x20000 != 0 {
        ensure!(authored != 0, "DDS mip count is zero");
        authored
    } else {
        ensure!(authored <= 1, "DDS mip count has no flag");
        1
    };
    ensure!(
        count <= width.max(height).ilog2() + 1,
        "DDS has too many mip levels"
    );
    let mut offset = 128;
    let mut decoded_bytes = 0;
    let mut levels = Vec::new();
    // Validate every source extent before allocating decoded pixels.
    let ranges = (0..count)
        .map(|level| {
            let width = (width >> level).max(1) as usize;
            let height = (height >> level).max(1) as usize;
            decoded_bytes += width * height * 4;
            ensure!(
                decoded_bytes <= MAX_BYTES,
                "DDS mip levels exceed pixel budget"
            );
            let size = width.div_ceil(4) * height.div_ceil(4) * compression.block_size();
            let start = offset;
            offset += size;
            Ok((
                width,
                height,
                bytes
                    .get(start..offset)
                    .context("truncated DDS mip level")?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        offset == bytes.len(),
        "unexpected bytes after DDS mip levels"
    );
    if flags & 0x80000 != 0 {
        ensure!(
            word(bytes, 20)? as usize == ranges[0].2.len(),
            "incorrect DDS base level length"
        );
    }
    for (width, height, source) in ranges {
        let mut rgba = vec![0; width * height * 4];
        for (index, block) in source.chunks_exact(compression.block_size()).enumerate() {
            let pixels = decode_block(block, compression);
            let x = (index % width.div_ceil(4)) * 4;
            let y = (index / width.div_ceil(4)) * 4;
            for row in 0..4.min(height - y) {
                for column in 0..4.min(width - x) {
                    let at = ((y + row) * width + x + column) * 4;
                    rgba[at..at + 4].copy_from_slice(&pixels[row * 4 + column]);
                }
            }
        }
        levels.push(Pixels {
            width: width as u32,
            height: height as u32,
            rgba,
        });
    }
    Ok(Decoded { levels })
}

fn decode_block(bytes: &[u8], compression: Compression) -> [[u8; 4]; 16] {
    let colors = &bytes[bytes.len() - 8..];
    let first = u16::from_le_bytes(colors[..2].try_into().unwrap());
    let second = u16::from_le_bytes(colors[2..4].try_into().unwrap());
    let rgb = |v: u16| {
        [
            expand5(v >> 11),
            expand6((v >> 5) & 63),
            expand5(v & 31),
            255,
        ]
    };
    let mut palette = [rgb(first), rgb(second), [0; 4], [0; 4]];
    if first > second || compression != Compression::Bc1 {
        for channel in 0..3 {
            let a = u16::from(palette[0][channel]);
            let b = u16::from(palette[1][channel]);
            palette[2][channel] = ((2 * a + b + 1) / 3) as u8;
            palette[3][channel] = ((a + 2 * b + 1) / 3) as u8;
        }
        palette[2][3] = 255;
        palette[3][3] = 255;
    } else {
        for channel in 0..3 {
            palette[2][channel] =
                ((u16::from(palette[0][channel]) + u16::from(palette[1][channel])) / 2) as u8;
        }
        palette[2][3] = 255;
        // BC1's transparent entry is transparent black, including RGB.
    }
    let indices = u32::from_le_bytes(colors[4..8].try_into().unwrap());
    let mut result = std::array::from_fn(|i| palette[((indices >> (2 * i)) & 3) as usize]);
    match compression {
        Compression::Bc1 => {}
        Compression::Bc2 => {
            let alpha = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            for (i, pixel) in result.iter_mut().enumerate() {
                pixel[3] = ((alpha >> (4 * i)) & 15) as u8 * 17;
            }
        }
        Compression::Bc3 => {
            let a = u16::from(bytes[0]);
            let b = u16::from(bytes[1]);
            let mut alpha = [bytes[0], bytes[1], 0, 0, 0, 0, 0, 255];
            let denominator = if a > b { 7 } else { 5 };
            for i in 1..denominator {
                alpha[i as usize + 1] =
                    (((denominator - i) * a + i * b + denominator / 2) / denominator) as u8;
            }
            let indices = u64::from_le_bytes(bytes[..8].try_into().unwrap()) >> 16;
            for (i, pixel) in result.iter_mut().enumerate() {
                pixel[3] = alpha[((indices >> (3 * i)) & 7) as usize];
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;

fn expand5(v: u16) -> u8 {
    ((v << 3) | (v >> 2)) as u8
}
fn expand6(v: u16) -> u8 {
    ((v << 2) | (v >> 4)) as u8
}
