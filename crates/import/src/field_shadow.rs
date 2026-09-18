//! Contact shadow ground quad and atlas from EFFECT.TPL inside effect.cab.
use crate::{dol, tpl};
use anyhow::{Context, Result, ensure};
use resonance_content::field::ContactShadow;
use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
};

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<ContactShadow> {
    let mut archive =
        cab::Cabinet::new(Cursor::new(fs::read(extracted.join("files/effect.cab"))?))?;
    let mut source = Vec::new();
    archive
        .read_file("EFFECT.TPL")?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut source)?;
    ensure!(
        source.len() <= 16 * 1024 * 1024,
        "effect texture archive exceeds limit"
    );
    let textures = tpl::parse_tpl(&source)?;
    // Atlas 2's IA8 bytes exactly match
    // the independent settled-classroom texture observation (see research).
    let texture = textures.get(2).context("effect atlas is missing")?;
    ensure!(
        (texture.width, texture.height, texture.format) == (256, 256, 3),
        "unsupported contact shadow atlas"
    );
    let rgba = tpl::decode_texture(&source, texture)?;
    fs::create_dir_all(output.join("effects"))?;
    let path = "effects/contact-shadow.ktx2";
    crate::texture::cook(256, 256, &rgba, &output.join(path))?;
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let value = |address| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(&executable, address, 4)?.try_into()?,
        ))
    };
    // The quad follows bone 1, has alpha 64, and sits two world units
    // above the surface. Convert the configured diameter to half-extents.
    Ok(ContactShadow {
        texture: path.into(),
        uv_size: [0.25; 2],
        half_size: (value(0x8035B080)? * value(0x8035B18C)? * value(0x8035AFFC)?).trunc(),
        height_offset: value(0x8035B080)?,
        alpha: 64,
        anchor_node: 1,
    })
}
