use super::*;

#[test]
fn converter_levels_survive_png_publication_without_filtering() -> Result<()> {
    let levels = vec![
        Pixels {
            width: 3,
            height: 2,
            rgba: (0..24).map(|i| i * 7).collect(),
        },
        Pixels {
            width: 1,
            height: 1,
            rgba: vec![231, 19, 88, 0],
        },
    ];
    let bytes = crate::ktx2::encode_levels(
        3,
        2,
        &levels
            .iter()
            .map(|level| level.rgba.as_slice())
            .collect::<Vec<_>>(),
    )?;
    let restored = ktx2(&bytes)?;
    assert_eq!(restored, levels);
    for (original, restored) in levels.iter().zip(restored) {
        assert_eq!(&png(&restored.png()?)?, original);
    }
    assert_ne!(
        generate(levels[0].clone(), false)?[1],
        levels[1],
        "the fixture must distinguish preserved authored pixels from generated mips"
    );
    Ok(())
}

#[test]
fn generated_mips_include_odd_edges_and_use_explicit_color_space() -> Result<()> {
    let base = Pixels {
        width: 3,
        height: 1,
        rgba: vec![0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255],
    };
    assert_eq!(generate(base.clone(), false)?[1].rgba, [85, 85, 85, 255]);
    assert_eq!(generate(base, true)?[1].rgba, [156, 156, 156, 255]);
    let transparent = Pixels {
        width: 2,
        height: 1,
        rgba: vec![255, 0, 0, 0, 0, 0, 255, 255],
    };
    assert_eq!(generate(transparent, false)?[1].rgba, [0, 0, 255, 128]);
    let sizes = generate(
        Pixels {
            width: 5,
            height: 3,
            rgba: vec![127; 60],
        },
        false,
    )?
    .into_iter()
    .map(|level| (level.width, level.height))
    .collect::<Vec<_>>();
    assert_eq!(sizes, [(5, 3), (2, 1), (1, 1)]);
    Ok(())
}

#[test]
fn malformed_dimensions_and_precision_are_rejected() -> Result<()> {
    assert!(
        Pixels {
            width: 0,
            height: 1,
            rgba: vec![]
        }
        .png()
        .is_err()
    );
    assert!(
        Pixels {
            width: 2,
            height: 1,
            rgba: vec![0; 4]
        }
        .png()
        .is_err()
    );
    assert!(
        validate_levels(&[
            Pixels {
                width: 4,
                height: 4,
                rgba: vec![0; 64]
            },
            Pixels {
                width: 1,
                height: 1,
                rgba: vec![0; 4]
            },
        ])
        .is_err()
    );
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes).write_image(
        &[0x12, 0x34],
        1,
        1,
        image::ExtendedColorType::L16,
    )?;
    assert!(
        png(&bytes).is_err(),
        "sixteen-bit replacements must not be silently truncated"
    );
    let valid = Pixels {
        width: 1,
        height: 1,
        rgba: vec![1, 2, 3, 4],
    }
    .png()?;
    assert!(png(&valid[..valid.len() / 2]).is_err());
    Ok(())
}
