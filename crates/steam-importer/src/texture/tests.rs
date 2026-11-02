use super::*;

fn resource(width: u32, height: u32, mips: u32, format: &[u8; 4], pixels: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 132];
    bytes[4..8].copy_from_slice(b"DDS ");
    for (at, value) in [
        (8, 124),
        (12, 0x1007 | if mips > 1 { 0x20000 } else { 0 }),
        (16, height),
        (20, width),
        (32, mips),
        (80, 32),
        (84, 4),
        (112, 0x1000),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[88..92].copy_from_slice(format);
    bytes.extend_from_slice(pixels);
    let length = bytes.len() as u32 - 4;
    bytes[..4].copy_from_slice(&length.to_le_bytes());
    bytes
}

fn color(first: u16, second: u16, indices: u32) -> Vec<u8> {
    [
        first.to_le_bytes().as_slice(),
        second.to_le_bytes().as_slice(),
        indices.to_le_bytes().as_slice(),
    ]
    .concat()
}

#[test]
fn descriptor_binds_its_own_payload_and_requires_matching_image_dimensions() -> Result<()> {
    let mut bytes = b"DPDF".to_vec();
    bytes.extend(
        [24u32, 1, 200, 0x00040300, 0x00010004, 0, 4]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    bytes.extend(b"Own_Image.totexp_p\0");
    let descriptor = Descriptor::read(&bytes)?;
    assert_eq!(descriptor.payload, "Own_Image.totexp_p");
    assert_eq!((descriptor.width, descriptor.height), (4, 4));
    let pixels = resource(4, 4, 1, b"DXT1", &color(0xf800, 0, 0));
    descriptor.decode(&pixels)?;
    let mut single = bytes.clone();
    single[16..18].copy_from_slice(&0x100u16.to_le_bytes());
    let single = Descriptor::read(&single)?;
    assert_eq!(single.payload, descriptor.payload);
    let decoded = single.decode(&pixels)?;
    assert_eq!(decoded.levels.len(), 1);
    assert_eq!(decoded.ktx2()?, descriptor.decode(&pixels)?.ktx2()?);
    assert!(
        descriptor
            .decode(&resource(8, 4, 1, b"DXT1", &[0; 16]))
            .is_err()
    );
    for (at, value) in [(8, 2u32), (12, 201), (16, 0), (20, 0x00020004), (24, 1)] {
        let mut bad = bytes.clone();
        bad[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            Descriptor::read(&bad).is_err(),
            "accepted invalid descriptor at {at}"
        );
    }
    assert!(Descriptor::read(&bytes[..bytes.len() - 1]).is_err());
    Ok(())
}

fn round_trip(decoded: &Decoded) -> Result<()> {
    let encoded = decoded.ktx2()?;
    assert_eq!(
        resonance_asset_writer::texture::ktx2(&encoded)?,
        decoded.levels
    );
    Ok(())
}

#[test]
fn bc1_keeps_transparency_block_order_and_authored_partial_mips() -> Result<()> {
    let red = color(0xf800, 0x001f, 0xe4e4e4e4);
    let transparent = color(0x001f, 0xf800, 0xe4e4e4e4);
    // Three distinct mip levels. The final 2x1 image still occupies one block.
    let pixels = [red.clone(), transparent.clone(), transparent, red].concat();
    let decoded = decode(&resource(8, 4, 3, b"DXT1", &pixels))?;
    assert_eq!(
        decoded
            .levels
            .iter()
            .map(|p| (p.width, p.height))
            .collect::<Vec<_>>(),
        [(8, 4), (4, 2), (2, 1)]
    );
    let base = &decoded.levels[0].rgba;
    assert_eq!(
        &base[..16],
        &[
            255, 0, 0, 255, 0, 0, 255, 255, 170, 0, 85, 255, 85, 0, 170, 255
        ]
    );
    assert_eq!(
        &base[16..32],
        &[0, 0, 255, 255, 255, 0, 0, 255, 127, 0, 127, 255, 0, 0, 0, 0]
    );
    assert_eq!(&base[..32], &base[32..64]);
    assert_eq!(&decoded.levels[1].rgba[12..16], &[0, 0, 0, 0]);
    assert_eq!(decoded.levels[2].rgba, [255, 0, 0, 255, 0, 0, 255, 255]);
    round_trip(&decoded)
}

#[test]
fn bc2_uses_explicit_alpha_without_bc1_transparency_or_premultiplication() -> Result<()> {
    let mut block = 0xfedcba9876543210u64.to_le_bytes().to_vec();
    // Ascending endpoints must still select four colors, even at zero alpha.
    block.extend(color(0x001f, 0xf800, u32::MAX));
    let decoded = decode(&resource(4, 4, 0, b"DXT3", &block))?;
    for (i, pixel) in decoded.levels[0].rgba.chunks_exact(4).enumerate() {
        assert_eq!(pixel, [170, 0, 85, i as u8 * 17]);
    }
    round_trip(&decoded)
}

#[test]
fn bc3_keeps_both_alpha_interpolation_modes_and_pixel_positions() -> Result<()> {
    let indices = (0..16).fold(0u64, |v, i| v | ((i % 8) << (3 * i)));
    for (first, second, expected) in [
        (255, 0, [255, 0, 219, 182, 146, 109, 73, 36]),
        (0, 255, [0, 255, 51, 102, 153, 204, 0, 255]),
        (100, 100, [100, 100, 100, 100, 100, 100, 0, 255]),
    ] {
        let mut block = vec![first, second];
        block.extend(&indices.to_le_bytes()[..6]);
        block.extend(color(0x001f, 0xf800, u32::MAX));
        let decoded = decode(&resource(4, 4, 1, b"DXT5", &block))?;
        for (i, pixel) in decoded.levels[0].rgba.chunks_exact(4).enumerate() {
            assert_eq!(pixel, [170, 0, 85, expected[i % 8]]);
        }
        round_trip(&decoded)?;
    }
    Ok(())
}

#[test]
fn dds_rejects_bad_extents_formats_and_surface_counts_before_decode() -> Result<()> {
    let bytes = resource(4, 4, 1, b"DXT1", &color(0xf800, 0, 0));
    for length in 0..bytes.len() {
        assert!(
            decode(&bytes[..length]).is_err(),
            "accepted truncation at {length}"
        );
    }
    for (at, value) in [
        (0, 1u32),
        (8, 128),
        (12, 0),
        (16, 0),
        (20, u32::MAX),
        (28, 1),
        (32, 2),
        (80, 0),
        (84, 0),
        (88, u32::from_le_bytes(*b"DX10")),
        (88, u32::from_le_bytes(*b"DXT2")),
        (92, 32),
        (112, 0),
        (116, 0x200),
    ] {
        let mut bad = bytes.clone();
        bad[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(decode(&bad).is_err(), "accepted bad field {at}");
    }
    assert!(decode(&resource(4, 4, 4, b"DXT1", &[0; 32])).is_err());
    assert!(decode(&resource(4, 4, 1, b"DXT1", &[0; 16])).is_err());
    let mut bad = bytes;
    bad[12..16].copy_from_slice(&0x81007u32.to_le_bytes());
    bad[24..28].copy_from_slice(&7u32.to_le_bytes());
    assert!(decode(&bad).is_err());
    Ok(())
}
