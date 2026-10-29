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
