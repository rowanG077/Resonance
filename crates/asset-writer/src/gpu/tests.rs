use super::*;
use crate::texture::Pixels;

#[test]
fn native_bc7_keeps_authored_mips_and_rgba_fallback() -> Result<()> {
    // Distinct quadrants per level expose a swapped axis or a wrong crop.
    let levels = [(12, 8), (6, 4), (3, 2), (1, 1)]
        .into_iter()
        .enumerate()
        .map(|(level, (width, height))| Pixels {
            width,
            height,
            rgba: (0..height)
                .flat_map(|y| {
                    (0..width).flat_map(move |x| {
                        let right = u8::from(x * 2 >= width);
                        let lower = u8::from(y * 2 >= height);
                        [
                            right * 200,
                            lower * 200,
                            level as u8 * 60,
                            255 - lower * 255,
                        ]
                    })
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let bytes = encode(&levels)?;
    let reader = Reader::new(bytes.as_slice())?;
    assert_eq!(reader.header().format, Some(Format::BC7_UNORM_BLOCK));
    assert_eq!(reader.header().level_count, 4);
    let decoded = crate::texture::ktx2(&bytes)?;
    for (decoded, level) in decoded.iter().zip(&levels) {
        assert_eq!((decoded.width, decoded.height), (level.width, level.height));
        for (actual, expected) in decoded.rgba.iter().zip(&level.rgba) {
            assert!(actual.abs_diff(*expected) <= 8, "{actual} != {expected}");
        }
    }
    assert_eq!(crate::texture::ktx2(&rgba8(&bytes)?)?, decoded);
    Ok(())
}

#[test]
fn other_sizes_are_padded_or_wrapped_as_the_console_does_without_changing_pixels() -> Result<()> {
    let (width, height) = (6u32, 5u32);
    let rgba = (0..width * height * 4)
        .map(|i| (i * 37 % 251) as u8 | 1)
        .collect::<Vec<_>>();
    assert!(encode_rgba8(width, height, &rgba).is_err());
    let source = |x: u32, y: u32| &rgba[((y * width + x) * 4) as usize..][..4];
    let original = Pixels {
        width,
        height,
        rgba: rgba.clone(),
    };
    let pixel = |pixels: &Pixels, x: u32, y: u32| {
        pixels.rgba[((y * pixels.width + x) * 4) as usize..][..4].to_vec()
    };

    let mut padded = original.clone();
    padded.pad_to_blocks();
    assert_eq!((padded.width, padded.height), (8, 8));
    for (x, y) in (0..8).flat_map(|y| (0..8).map(move |x| (x, y))) {
        let edge = source(x.min(width - 1), y.min(height - 1));
        let outside = x >= width || y >= height;
        assert_eq!(pixel(&padded, x, y)[..3], edge[..3]);
        assert_eq!(pixel(&padded, x, y)[3], if outside { 0 } else { edge[3] });
    }

    // Width 6 masks with 5 (columns 0, 1, 4, 5); height 5 masks with 4 (rows 0
    // and 4), each over the next power of two. An axis that clamps is kept.
    let wrapped = crate::texture::console_wrapped(std::slice::from_ref(&original), [true, true]);
    assert_eq!((wrapped[0].width, wrapped[0].height), (8, 8));
    for (x, y) in (0..8).flat_map(|y| (0..8).map(move |x| (x, y))) {
        assert_eq!(pixel(&wrapped[0], x, y), source(x & 5, y & 4));
    }
    let kept = crate::texture::console_wrapped(std::slice::from_ref(&original), [true, false]);
    assert_eq!((kept[0].width, kept[0].height), (8, 5));
    assert!(
        is_native_compressed(&encode(&[padded])?)? && is_native_compressed(&encode(&wrapped)?)?
    );
    Ok(())
}
