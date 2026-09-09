//! Offline cel-shading ramp conversion. Runtime samples an ordinary RGB mask.
use crate::tpl;
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

pub(crate) fn cook(extracted: &Path, output: &Path, ktx: &Path) -> Result<String> {
    let mut source = fs::read(extracted.join("files/toon.tpl"))?;
    let mut texture = tpl::parse_tpl(&source)?
        .into_iter()
        .next()
        .context("toon ramp is missing")?;
    ensure!(
        (texture.width, texture.height, texture.format) == (256, 32, 8),
        "unsupported toon ramp"
    );
    // Encode shade/bright/unchanged-white weights in RGB. Updating script
    // colors can then reuse the same GPU texture.
    texture.palette_offset = Some(source.len());
    texture.palette_entries = 16;
    texture.palette_format = 2;
    for i in 0..16 {
        source.extend(
            match i {
                14 => 0xFC00u16,
                15 => 0x83E0,
                _ => 0x801F,
            }
            .to_be_bytes(),
        );
    }
    let rgba = tpl::decode_texture(&source, &texture)?;
    let intermediate = output.join("intermediate/effects/toon-ramp.png");
    fs::create_dir_all(intermediate.parent().unwrap())?;
    fs::create_dir_all(output.join("effects"))?;
    image::save_buffer(&intermediate, &rgba, 256, 32, image::ColorType::Rgba8)?;
    let path = "effects/toon-ramp.ktx2";
    crate::texture::cook(ktx, &intermediate, &output.join(path))?;
    Ok(path.into())
}
