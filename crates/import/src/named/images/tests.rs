use super::*;

#[test]
fn authored_mips_publish_as_editable_pngs_and_republish_without_loss() -> Result<()> {
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let republished = tempfile::tempdir()?;
    let levels = vec![
        texture::Pixels {
            width: 4,
            height: 2,
            rgba: vec![73; 32],
        },
        texture::Pixels {
            width: 2,
            height: 1,
            rgba: vec![31, 97, 201, 127, 201, 97, 31, 0],
        },
        texture::Pixels {
            width: 1,
            height: 1,
            rgba: vec![11, 23, 251, 3],
        },
    ];
    fs::write(
        source.path().join("native.ktx2"),
        resonance_asset_writer::ktx2::encode_levels(
            4,
            2,
            &levels
                .iter()
                .map(|level| level.rgba.as_slice())
                .collect::<Vec<_>>(),
        )?,
    )?;
    let mut publisher = Publisher::new(source.path(), output.path())?;
    let path = publisher.image("native.ktx2", "meshes/props/sign/wood")?;
    assert_eq!(path, "meshes/props/sign/wood.texture.json");
    assert_eq!(
        crate::texture::image_levels(output.path(), &path)?.1,
        levels
    );
    assert!(!output.path().join("meshes/props/sign/wood.ktx2").exists());
    let path2 =
        Publisher::new(output.path(), republished.path())?.image(&path, "meshes/sign/wood")?;
    crate::texture::compare_images(output.path(), &path, republished.path(), &path2)?;

    let mut definition: Definition = serde_json::from_slice(&fs::read(output.path().join(&path))?)?;
    definition.mips = Mips::Generate;
    definition.color_space = ColorSpace::Srgb;
    fs::write(output.path().join(&path), serde_json::to_vec(&definition)?)?;
    let generated = Publisher::new(output.path(), republished.path())?
        .image(&path, "meshes/replacement/wood")?;
    let copied: Definition =
        serde_json::from_slice(&fs::read(republished.path().join(generated))?)?;
    assert!(matches!(copied.mips, Mips::Generate));
    assert_eq!(copied.color_space, ColorSpace::Srgb);
    assert_eq!(copied.image, "meshes/replacement/wood.png");
    assert!(
        !republished
            .path()
            .join("meshes/replacement/wood-mip-1.png")
            .exists()
    );
    Ok(())
}
