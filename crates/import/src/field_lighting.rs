//! Offline cel-shading ramp conversion. Runtime samples an ordinary RGB mask.
use crate::tpl;
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<String> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let source = crate::all_assets::roles::toon_path(extracted, &executable)?;
    let mut source = fs::read(extracted.join("files").join(source))?;
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
    let path = "effects/toon-ramp.ktx2";
    crate::texture::cook(256, 32, &rgba, &output.join(path))?;
    Ok(path.into())
}
