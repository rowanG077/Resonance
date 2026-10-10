//! Party HUD and result artwork.
use crate::{read::Field, rel::Rel, source_assets::section, texture::Texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_ui::Art,
    font::{BitmapFont, Glyph, UiTexture},
};
use std::{collections::BTreeSet, fs, path::Path};

/// All source discovery and conversion finishes before the descriptor is published.
pub fn publish(extracted: &Path, output: &Path) -> Result<Vec<String>> {
    let sources = crate::source_assets::Sources::read(extracted)?;
    let usual = fs::read(extracted.join("files").join(sources.usual))?;
    let module = Rel::read(&extracted.join("files").join(sources.module))?;
    publish_source(&usual, &module, output, "battle")
}

pub(crate) fn publish_source(
    usual: &[u8],
    module: &Rel,
    output: &Path,
    prefix: &str,
) -> Result<Vec<String>> {
    let images = section(usual, 4)?;
    let portraits = crate::texture::decode_source(section(images, 2)?)?
        .write(output)?
        .textures
        .into_iter()
        .map(|texture| {
            let texture = texture.context("missing battle portrait")?;
            sampler(&texture, true)?;
            texture.image(0)
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected nine battle portraits"))?;
    let atlas = crate::texture::decode_source(section(images, 3)?)?.write(output)?;
    ensure!(atlas.textures.len() == 1, "expected one battle HUD atlas");
    let atlas = atlas.textures[0]
        .as_ref()
        .context("missing battle HUD atlas")?;
    ensure!(
        atlas.dimensions == [512, 512] && atlas.images.len() == 96,
        "unsupported battle HUD atlas"
    );
    sampler(atlas, false)?;
    let art = read(usual, module, atlas, portraits)?;
    art.validate()?;
    let path = format!("{prefix}/ui.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&art)?)?;
    Ok(std::iter::once(path)
        .chain(art.files().map(str::to_owned))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

fn sampler(texture: &Texture, repeat: bool) -> Result<()> {
    use resonance_content::{TextureWrap, texture::Filter};
    ensure!(
        texture.sampler.wrap.iter().all(|wrap| if repeat {
            matches!(wrap, TextureWrap::Repeat)
        } else {
            matches!(wrap, TextureWrap::Clamp)
        }) && matches!(texture.sampler.min_filter, Filter::Linear)
            && matches!(texture.sampler.mag_filter, Filter::Linear),
        "unsupported battle UI sampler"
    );
    Ok(())
}

fn read(usual: &[u8], module: &Rel, atlas: &Texture, portraits: [UiTexture; 9]) -> Result<Art> {
    let at = |offset: usize| module.at((4, offset));
    ensure!(
        at(0xd8)?.get(..5) == Some(&[10, 0, 11, 12, 1]),
        "unsupported battle texture bindings"
    );
    let punctuation = <[[u16; 2]; 15]>::read(at(0x26e0)?, 0)?;
    let font = font(usual, module, atlas.image(0)?, punctuation);
    Ok(Art {
        version: Art::VERSION,
        font,
        portraits: portraits.map(Some),
    })
}

fn font(usual: &[u8], module: &Rel, image: UiTexture, punctuation: [[u16; 2]; 15]) -> BitmapFont {
    let glyphs = (b'!'..=b'Z')
        .filter_map(|ch| {
            let [x, y] = match ch {
                b'!'..=b'/' => punctuation[usize::from(ch - b'!')].map(u32::from),
                b'0'..=b'9' => [u32::from(ch - b'0') * 16, 0],
                b'A'..=b'F' => [160 + u32::from(ch - b'A') * 16, 0],
                b'G'..=b'V' => [u32::from(ch - b'G') * 16, 24],
                b'W'..=b'Z' => [u32::from(ch - b'W') * 16, 48],
                _ => return None,
            };
            Some((
                char::from(ch),
                Glyph {
                    rect: [x, y, 16, 23],
                    advance: 13,
                },
            ))
        })
        .collect();
    BitmapFont {
        version: 1,
        texture: image.path,
        width: image.width,
        height: image.height,
        line_height: 24,
        glyphs,
        source_sha256: crate::digest(usual),
        executable_sha256: crate::digest(&module.bytes),
    }
}

#[cfg(test)]
mod tests;
