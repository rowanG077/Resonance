//! Four native startup logos and their editable playback sequence.
use crate::menu_sprites;
use anyhow::{Result, ensure};
use resonance_content::{BootAssets, startup::boot::Logo};
use std::{fs, path::Path};

const IMAGES: [&str; 4] = [
    "ui/boot/namco.ktx2",
    "ui/boot/namco-tales-studio.ktx2",
    "ui/boot/adx.ktx2",
    "ui/boot/licensed-by-nintendo.ktx2",
];

pub fn cook_boot(extracted: &Path, library: &Path) -> Result<()> {
    let source = fs::read(extracted.join("disc1/files/boot.tpl"))?;
    ensure!(
        crate::dol::u32(&source, 4)? == 4,
        "unexpected startup logo count"
    );
    let mut images = Vec::new();
    let mut backgrounds = [[0; 3]; 4];
    let mut sizes = [[0; 2]; 4];
    for (index, path) in IMAGES.into_iter().enumerate() {
        let (width, height, rgba) = menu_sprites::decode_image(&source, index)?;
        backgrounds[index].copy_from_slice(&rgba[..3]);
        // Two logos are 379 pixels wide; a transparent column completes their blocks.
        let mut logo = resonance_asset_writer::texture::Pixels {
            width,
            height,
            rgba,
        };
        logo.pad_to_blocks();
        sizes[index] = [logo.width, logo.height];
        images.push((path, resonance_asset_writer::gpu::encode(&[logo])?));
    }
    let sequence = BootAssets(
        [
            (3, 180, None),
            (0, 240, Some(120)),
            (1, 180, None),
            (2, 120, None),
        ]
        .into_iter()
        .map(|(index, fade_out_at, skip_after)| Logo {
            image: IMAGES[index].into(),
            size: sizes[index],
            fade_out_at,
            fade_ticks: 64,
            skip_after,
            background: backgrounds[index],
        })
        .collect(),
    );
    sequence.duration()?;
    for (path, bytes) in images {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    let output = library.join("ui/boot/sequence.json");
    crate::output::json(&output, &sequence)?;
    Ok(())
}
