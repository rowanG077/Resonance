//! Native HUD art in the shared 640x480 GameCube battle layout.
use crate::{effect_atlas, output, package::Package, pe, source::Source, text::Bank, ui_images};
use anyhow::{Context, Result, ensure};
use image::{RgbaImage, imageops};
use resonance_content::{
    battle_ui::{Art, document},
    font::{BitmapFont, Glyph},
};
use std::{collections::BTreeMap, path::Path};

mod atlas;

/// All media is authored by this owner; no source-edition UI branches reach runtime.
pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let texts = source.require("TOS_TEXT.TXB")?;
    let bank = Bank::parse(&texts)?;
    let slot = source.language.slot();
    let mut art: Art<String> = serde_json::from_str(include_str!("battle_ui/layout.json"))?;
    let bytes = source.require("BTLusual.pac")?;
    let usual = Package::parse(&bytes)?;
    let images = Package::parse(usual.member(4)?.context("missing resident battle images")?)?;
    let name = images.reference(3)?.context("missing battle HUD atlas")?;
    let directory = effect_atlas::Directory::read(&image)?;
    let mut atlas = atlas::Atlas::new(ui_images::read(source, name)?, directory.get(name))?;
    let font = font(source, library, &image, &atlas.page(0)?)?;
    for (index, image) in art.targets.iter_mut().enumerate() {
        *image = atlas.publish(library, 16 + index as u16)?;
    }
    art.stun = atlas.publish(library, 62)?;
    art.notices.frame = atlas.publish(library, 80)?;
    art.notices.fill = atlas.publish(library, 81)?;
    for (index, image) in art.notices.icons.iter_mut().enumerate() {
        *image = atlas.publish(library, 82 + index as u16)?;
    }
    for (index, image) in art.notices.faces.iter_mut().enumerate() {
        *image = atlas.publish(library, 32 + index as u16)?;
    }
    for (image, palette) in art
        .stat_icons
        .iter_mut()
        .zip([94, 93, 91, 87, 89, 90, 42, 43, 88, 92])
    {
        *image = atlas.publish(library, palette)?;
    }
    for (index, image) in art.feedback.scan_icons.iter_mut().enumerate() {
        *image = atlas.publish(library, 48 + index as u16)?;
    }
    art.feedback.unison_frame = atlas.publish(library, 27)?;
    for (index, image) in art.feedback.escape.iter_mut().enumerate() {
        *image = atlas.publish(library, 29 + index as u16)?;
    }
    atlas.commands(library, &mut art.commands)?;
    let bytes = source.require(
        images
            .reference(2)?
            .context("missing battle portrait directory")?,
    )?;
    let portraits = Package::parse(&bytes)?;
    ensure!(portraits.len() == 9, "battle portrait count changed");
    for (index, portrait) in art.portraits.iter_mut().enumerate() {
        let pixels = ui_images::read(
            source,
            portraits
                .reference(index)?
                .context("missing battle portrait")?,
        )?;
        let pixels = imageops::resize(&pixels, 64, 256, imageops::FilterType::Triangle);
        let path = portrait
            .as_ref()
            .context("missing portrait output binding")?
            .path
            .clone();
        *portrait = Some(ui_images::write(library, &path, &pixels)?);
    }
    let text = |address| {
        bank.plain(
            pe::word(image.slice(address, 4)?, 0)?,
            encoding_rs::WINDOWS_1252,
        )
    };
    for (index, name) in art.commands.names.iter_mut().enumerate() {
        *name = text(0x7c5db4 + (slot - 1) * 32 + index as u32 * 4)?;
    }
    art.commands.cancel_escape = text(0x7c5e6c + (slot - 1) * 24)?;
    art.commands.selector_labels = [
        text(0x7c5e74 + (slot - 1) * 24)?,
        text(0x7c5e78 + (slot - 1) * 24)?,
    ];
    art.feedback.item_label = text(0x7db13c + (slot - 1) * 60)?;
    art.feedback.unison_ready = text(0x7db12c + (slot - 1) * 60)?;
    art.notices.defeated = text(0x7300cc + (slot - 1) * 8)?;
    let validation = art.clone().with_font(font);
    validation.validate()?;
    validation.validate_required_dialogue(&validation.font)?;
    output::json(&library.join(document::PATH), &art)?;
    eprintln!("  Battle UI: shared HUD, battle font and nine portraits");
    Ok(())
}

/// HUD capitals and numerals retain the GC's narrow metrics. The native Latin
/// page supplies punctuation and Western accents in the same ordinary font.
fn font(
    source: &Source,
    library: &Path,
    image: &pe::Image<'_>,
    hud: &RgbaImage,
) -> Result<BitmapFont> {
    let latin = ui_images::read(source, "PAL_FONTB0.TOTEXB_D")?;
    ensure!(
        latin.dimensions() == (768, 1728),
        "unsupported native Latin font dimensions"
    );
    let latin = imageops::resize(&latin, 384, 864, imageops::FilterType::Triangle);
    let mut atlas = RgbaImage::new(512, 1376);
    imageops::replace(&mut atlas, hud, 0, 0);
    imageops::replace(&mut atlas, &latin, 0, 512);
    let mut glyphs = BTreeMap::new();
    let tile = |index: u32| Glyph {
        rect: [index % 16 * 24, 512 + index / 16 * 24, 24, 24],
        advance: 13,
    };
    let mapping = image.slice(0x8ad8a8, 96 * 2)?;
    for byte in 32u8..127 {
        let at = usize::from(byte - 32) * 2;
        let code = u16::from_be_bytes(mapping[at..at + 2].try_into()?);
        ensure!(
            (0x8140..0x8440).contains(&code) && code & 255 >= 0x40,
            "invalid native ASCII glyph mapping"
        );
        let index = u32::from((code >> 8) - 0x81) * 192 + u32::from((code & 255) - 0x40);
        glyphs.insert(char::from(byte), tile(index));
    }
    for byte in 128u8..=255 {
        let bytes = [byte];
        let (text, _, invalid) = encoding_rs::WINDOWS_1252.decode(&bytes);
        if !invalid && text.chars().all(|ch| !ch.is_control()) {
            glyphs.insert(text.chars().next().unwrap(), tile(u32::from(byte) + 160));
        }
    }
    for byte in b'0'..=b'Z' {
        let position = match byte {
            b'0'..=b'9' => [u32::from(byte - b'0') * 16, 0],
            b'A'..=b'F' => [160 + u32::from(byte - b'A') * 16, 0],
            b'G'..=b'V' => [u32::from(byte - b'G') * 16, 24],
            b'W'..=b'Z' => [u32::from(byte - b'W') * 16, 48],
            _ => continue,
        };
        let glyph = Glyph {
            rect: [position[0], position[1], 16, 23],
            advance: 13,
        };
        glyphs.insert(char::from(byte), glyph.clone());
        if byte.is_ascii_uppercase() {
            glyphs.insert(char::from(byte.to_ascii_lowercase()), glyph);
        }
    }
    let font = BitmapFont {
        texture: "fonts/battle.ktx2".into(),
        width: atlas.width(),
        height: atlas.height(),
        line_height: 24,
        glyphs,
    };
    font.validate()?;
    ui_images::write(library, &font.texture, &atlas)?;
    output::json(&library.join(document::FONT), &font)?;
    Ok(font)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gc_layout_uses_the_current_named_art_schema() -> Result<()> {
        let art: Art<String> = serde_json::from_str(include_str!("battle_ui/layout.json"))?;
        assert_eq!(art.portraits.iter().flatten().count(), 9);
        assert_eq!(
            art.commands.icons.iter().map(Vec::len).collect::<Vec<_>>(),
            [3, 5, 4, 2, 3, 1]
        );
        assert_eq!(art.font, document::FONT);
        Ok(())
    }
}
