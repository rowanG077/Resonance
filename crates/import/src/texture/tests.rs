use super::{cook, cook_png, encode::*, fingerprint};
use bevy_image::{CompressedImageFormats, ktx2_buffer_to_image};
use ktx2::{ColorModel, ColorPrimaries, Format, Reader, SupercompressionScheme, TransferFunction};
use std::{fs, io::Read, time::SystemTime};

fn verify(width: u32, height: u32, pixels: &[u8]) {
    let encoded = encode_rgba8(width, height, pixels).unwrap();
    assert_eq!(encoded, encode_rgba8(width, height, pixels).unwrap());
    let reader = Reader::new(&encoded).unwrap();
    let header = reader.header();
    assert_eq!(header.format, Some(Format::R8G8B8A8_UNORM));
    assert_eq!(header.type_size, 1);
    assert_eq!((header.pixel_width, header.pixel_height), (width, height));
    assert_eq!((header.pixel_depth, header.layer_count), (0, 0));
    assert_eq!((header.face_count, header.level_count), (1, 1));
    assert_eq!(
        header.supercompression_scheme,
        Some(SupercompressionScheme::Zstandard)
    );
    assert_eq!(reader.color_model(), Some(ColorModel::RGBSDA));
    assert_eq!(reader.color_primaries(), Some(ColorPrimaries::BT709));
    assert_eq!(reader.transfer_function(), Some(TransferFunction::Linear));
    assert_eq!(reader.is_alpha_premultiplied(), Some(false));
    let dfd = reader.basic_dfd().unwrap();
    assert_eq!(dfd.texel_block_dimensions.map(|n| n.get()), [1; 4]);
    assert_eq!(dfd.bytes_planes, [4, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(dfd.sample_information.len(), 4);
    for (index, sample) in dfd.sample_information.iter().enumerate() {
        assert_eq!(sample.bit_offset, index as u16 * 8);
        assert_eq!(sample.bit_length.get(), 8);
        assert_eq!(sample.channel_type, [0, 1, 2, 15][index]);
        assert_eq!(sample.sample_positions, [0; 4]);
        assert_eq!((sample.lower, sample.upper), (0, 255));
    }
    // No orientation or swizzle override: KTX2 defaults to right/down and RGBA.
    assert_eq!(reader.key_value_data().count(), 0);
    assert!(reader.supercompression_global_data().is_empty());
    let level = reader.levels().next().unwrap();
    assert_eq!(level.uncompressed_byte_length, pixels.len() as u64);
    let mut decoded = Vec::new();
    ruzstd::decoding::StreamingDecoder::new(level.data)
        .unwrap()
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, pixels);

    // Exercise the player's actual image loader, including decompression and
    // GPU format selection. This catches more than parsing our own header.
    let image = ktx2_buffer_to_image(&encoded, CompressedImageFormats::NONE, false).unwrap();
    assert_eq!(image.data.as_deref(), Some(pixels));
    let descriptor = image.texture_descriptor;
    assert_eq!(
        (descriptor.size.width, descriptor.size.height),
        (width, height)
    );
    assert_eq!(descriptor.size.depth_or_array_layers, 1);
    assert_eq!(descriptor.mip_level_count, 1);
    assert_eq!(format!("{:?}", descriptor.format), "Rgba8Unorm");
}

#[test]
fn preserves_channels_alpha_and_orientation() {
    verify(1, 1, &[31, 73, 127, 0]); // Invisible RGB must survive.
    verify(
        2,
        3,
        &[
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 91, 27, 41, 1, 11, 13, 17, 99, 19, 23,
            29, 254,
        ],
    );
    let gradient: Vec<_> = (0..17 * 9)
        .flat_map(|i| {
            [
                (i % 17 * 15) as u8,
                (i / 17 * 31) as u8,
                i as u8,
                (i * 7) as u8,
            ]
        })
        .collect();
    verify(17, 9, &gradient);
    verify(1, 153, &gradient);
    verify(153, 1, &gradient);
}

#[test]
fn handles_zstd_block_boundaries_and_incompressible_pixels() {
    for width in [32_767, 32_768, 32_769, 262_144] {
        let mut seed = 0x1234_5678u32;
        let pixels: Vec<_> = (0..width * 4)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        verify(width, 1, &pixels);
        verify(width, 1, &vec![0; pixels.len()]);
    }
}

#[test]
fn rejects_invalid_dimensions_and_lengths_before_compression() {
    for (width, height) in [(0, 0), (0, 1), (1, 0)] {
        assert_eq!(
            encode_rgba8(width, height, &[]),
            Err(EncodeError::EmptyDimensions)
        );
    }
    assert_eq!(
        encode_rgba8(u32::MAX, u32::MAX, &[]),
        Err(EncodeError::SizeOverflow)
    );
    for length in [0, 3, 5] {
        assert_eq!(
            encode_rgba8(1, 1, &vec![0; length]),
            Err(EncodeError::PixelLength {
                expected: 4,
                actual: length
            })
        );
    }
}

#[test]
fn preserves_authored_mips_in_the_player_image_loader() {
    let pixels = [
        vec![17; 8 * 4 * 4],
        vec![29; 4 * 2 * 4],
        vec![43; 2 * 4],
        vec![71; 4],
    ];
    let levels: Vec<_> = pixels.iter().map(Vec::as_slice).collect();
    let bytes = encode_levels(8, 4, &levels).unwrap();
    let reader = Reader::new(&bytes).unwrap();
    assert_eq!(reader.header().level_count, 4);
    for (level, pixels) in reader.levels().zip(&pixels) {
        let mut decoded = Vec::new();
        ruzstd::decoding::StreamingDecoder::new(level.data)
            .unwrap()
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(&decoded, pixels);
    }
    let image = ktx2_buffer_to_image(&bytes, CompressedImageFormats::NONE, false).unwrap();
    assert_eq!(image.texture_descriptor.mip_level_count, 4);
    assert_eq!(image.data.unwrap(), pixels.concat());
    assert_eq!(encode_levels(1, 1, &levels), Err(EncodeError::LevelCount));
    assert_eq!(encode_levels(8, 4, &[]), Err(EncodeError::LevelCount));
    assert!(encode_levels(8, 4, &[&pixels[0], &pixels[2]]).is_err());
}

#[test]
fn cache_identity_includes_recipe_dimensions_and_pixels() {
    let pixels = [23; 24];
    let hash = fingerprint(2, 3, &pixels);
    assert_eq!(hash, fingerprint(2, 3, &pixels));
    assert_ne!(hash, crate::digest(&pixels)); // Invalidates old portrait cooks.
    assert_ne!(hash, fingerprint(3, 2, &pixels));
    assert_ne!(hash, fingerprint(2, 3, &[24; 24]));
}

#[test]
fn png_adapter_and_repeated_cooks_preserve_pixels_and_files() {
    let directory = tempfile::tempdir().unwrap();
    let png = directory.path().join("editable.png");
    let output = directory.path().join("nested/texture.ktx2");
    let pixels = [31, 73, 127, 0, 11, 13, 17, 255];
    image::save_buffer(&png, &pixels, 2, 1, image::ColorType::Rgba8).unwrap();
    cook_png(&png, &output).unwrap();
    assert_eq!(
        fs::read(&output).unwrap(),
        encode_rgba8(2, 1, &pixels).unwrap()
    );
    // No sleep or filesystem timestamp-resolution assumption is needed.
    fs::File::options()
        .write(true)
        .open(&output)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH)
        .unwrap();
    let timestamp = fs::metadata(&output).unwrap().modified().unwrap();
    cook(2, 1, &pixels, &output).unwrap();
    assert_eq!(
        fs::metadata(&output).unwrap().modified().unwrap(),
        timestamp
    );
    assert!(cook(2, 1, &pixels[..7], &output).is_err());
    assert_eq!(
        fs::read(&output).unwrap(),
        encode_rgba8(2, 1, &pixels).unwrap()
    );
    assert!(!output.with_extension("partial").exists());
}
