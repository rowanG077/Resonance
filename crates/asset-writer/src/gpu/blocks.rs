//! Compress repeated flat blocks once and batch the remaining blocks for SIMD.
use intel_tex_2::{RgbaSurface, bc7};
use std::collections::HashMap;

pub(super) fn encode(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let settings = if data.chunks_exact(4).all(|p| p[3] == 255) {
        bc7::opaque_very_fast_settings()
    } else {
        bc7::alpha_very_fast_settings()
    };
    let count = (width / 4 * height / 4) as usize;
    let mut output = vec![0; count * 16];
    let mut flat = HashMap::<[u8; 4], [u8; 16]>::new();
    let mut remaining = Vec::new();
    let columns = width as usize / 4;
    for index in 0..count {
        let first = ((index / columns * 4) * width as usize + index % columns * 4) * 4;
        let color: [u8; 4] = data[first..first + 4].try_into().unwrap();
        let uniform = (0..4).all(|row| {
            data[first + row * width as usize * 4..first + row * width as usize * 4 + 16]
                .chunks_exact(4)
                .all(|p| p == color)
        });
        if uniform {
            let block = flat.get(&color).copied().or_else(|| {
                if flat.len() == 256 {
                    return None;
                }
                let rgba = color.repeat(16);
                let encoded = bc7::compress_blocks(
                    &settings,
                    &RgbaSurface {
                        data: &rgba,
                        width: 4,
                        height: 4,
                        stride: 16,
                    },
                );
                let block = encoded.try_into().unwrap();
                flat.insert(color, block);
                Some(block)
            });
            if let Some(block) = block {
                output[index * 16..index * 16 + 16].copy_from_slice(&block);
                continue;
            }
        }
        remaining.push(index);
    }
    if remaining.len() == count {
        bc7::compress_blocks_into(
            &settings,
            &RgbaSurface {
                data,
                width,
                height,
                stride: width * 4,
            },
            &mut output,
        );
    } else if !remaining.is_empty() {
        let packed_columns = remaining.len().min(32);
        let w = packed_columns * 4;
        let h = remaining.len().div_ceil(packed_columns) * 4;
        let mut pixels = vec![0; w * h * 4];
        for (index, &original) in remaining.iter().enumerate() {
            for row in 0..4 {
                let from =
                    ((original / columns * 4 + row) * width as usize + original % columns * 4) * 4;
                let to = ((index / packed_columns * 4 + row) * w + index % packed_columns * 4) * 4;
                pixels[to..to + 16].copy_from_slice(&data[from..from + 16]);
            }
        }
        let encoded = bc7::compress_blocks(
            &settings,
            &RgbaSurface {
                data: &pixels,
                width: w as u32,
                height: h as u32,
                stride: (w * 4) as u32,
            },
        );
        for (index, &original) in remaining.iter().enumerate() {
            output[original * 16..original * 16 + 16]
                .copy_from_slice(&encoded[index * 16..index * 16 + 16]);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_blocks_keep_the_same_encoding_as_the_original_surface() {
        for alpha in [false, true] {
            for (width, height) in [(4, 4), (28, 24), (132, 12)] {
                let mut pixels = vec![0; (width * height * 4) as usize];
                for y in 0..height {
                    for x in 0..width {
                        let block = y / 4 * (width / 4) + x / 4;
                        let rx = x % 8;
                        let ry = y % 8;
                        let color = if block % 3 == 0 {
                            [17, 35, 71, if alpha { 113 } else { 255 }]
                        } else {
                            [
                                (rx * 7) as u8,
                                (ry * 11) as u8,
                                (rx * ry) as u8,
                                if alpha { (rx + ry) as u8 } else { 255 },
                            ]
                        };
                        let start = ((y * width + x) * 4) as usize;
                        pixels[start..start + 4].copy_from_slice(&color);
                    }
                }
                let settings = if alpha {
                    bc7::alpha_very_fast_settings()
                } else {
                    bc7::opaque_very_fast_settings()
                };
                let expected = bc7::compress_blocks(
                    &settings,
                    &RgbaSurface {
                        data: &pixels,
                        width,
                        height,
                        stride: width * 4,
                    },
                );
                let actual = encode(&pixels, width, height);
                for (actual, expected) in actual.chunks_exact(16).zip(expected.chunks_exact(16)) {
                    let mut a = [0; 64];
                    let mut b = [0; 64];
                    bcdec_rs::bc7(actual, &mut a, 16);
                    bcdec_rs::bc7(expected, &mut b, 16);
                    assert_eq!(a, b, "{width}x{height}, alpha={alpha}");
                }
            }
        }
    }
}
