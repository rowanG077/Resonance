//! The world particle controller selects four CLOUDTEX package members.
use super::*;

pub(super) fn cook(source: &Source, library: &Path, image: &pe::Image<'_>) -> Result<()> {
    // The world effect dispatch calls 0x4ad310; that consumer loads CLOUDTEX
    // and selects a frame from the same package handle, rather than field SAND.
    ensure!(
        image.slice(0x4b6ddf, 5)? == [0xe8, 0x2c, 0x65, 0xff, 0xff]
            && image.slice(0x4ad3e3, 10)? == [0x68, 0x5c, 0xfe, 0x8d, 0, 0xb9, 0x5c, 0xfe, 0x8d, 0]
            && image.slice(0x4ad480, 6)? == [0x8d, 0x88, 0x58, 0xfe, 0x8d, 0],
        "world dust texture consumer changed"
    );
    let bytes = source.require(image.text(0x8dfe5c)?)?;
    let package = Package::parse(&bytes)?;
    ensure!(package.len() == 4, "world dust frame count changed");
    for index in 0..package.len() {
        let image = crate::ui_images::cook(
            source,
            library,
            package
                .reference(index)?
                .context("missing world dust frame")?,
            &format!("world/effects/dust-{index}.ktx2"),
            false,
        )?;
        ensure!(
            (image.width, image.height) == (32, 32),
            "unexpected world dust dimensions"
        );
    }
    Ok(())
}
