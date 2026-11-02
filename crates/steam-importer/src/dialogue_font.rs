//! Source glyph pages packed into the common editable bitmap font.
use crate::{output, pe, source::Source, ui_images};
use anyhow::{Context, Result, ensure};
use resonance_content::font::{BitmapFont, Glyph};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn cook(source: &Source, output: &Path) -> Result<BitmapFont> {
    let executable = pe::Image::parse(&source.executable)?;
    let latin = ui_images::read(source, "PAL_FONTB0.TOTEXB_D")?;
    ensure!(
        latin.dimensions() == (768, 1728),
        "unsupported Latin font atlas"
    );
    let latin = image::imageops::resize(&latin, 384, 864, image::imageops::FilterType::Triangle);
    let mut pages = Vec::new();
    for page in 0..3 {
        let pixels = ui_images::read(source, &format!("FONTDATA_PIC{page:03}.TOTEXB_D"))?;
        ensure!(
            pixels.dimensions() == (384, 288),
            "unsupported dialogue font page"
        );
        pages.push(pixels);
    }
    let mut tiles = BTreeMap::<char, image::RgbaImage>::new();
    let tile = |page: &image::RgbaImage, index: u32| {
        image::imageops::crop_imm(page, index % 16 * 24, index / 16 * 24, 24, 24).to_image()
    };
    for code in 0x8140_u16..0x8440 {
        if code & 255 < 0x40 {
            continue;
        }
        let bytes = code.to_be_bytes();
        let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes);
        if !invalid && text.chars().count() == 1 {
            tiles
                .entry(text.chars().next().unwrap())
                .or_insert_with(|| {
                    tile(
                        &pages[usize::from((code >> 8) - 0x81)],
                        u32::from((code & 255) - 0x40),
                    )
                });
        }
    }
    // This executable table stores Shift-JIS byte pairs, even on the LE port.
    let ascii = executable.slice(0x8ad8a8, 96 * 2)?;
    for byte in 32_u8..127 {
        let code = u16::from_be_bytes(ascii[usize::from(byte - 32) * 2..][..2].try_into().unwrap());
        ensure!(
            (0x8140..0x8440).contains(&code) && code & 255 >= 0x40,
            "invalid ASCII font mapping"
        );
        let index = u32::from((code >> 8) - 0x81) * 192 + u32::from((code & 255) - 0x40);
        tiles.insert(char::from(byte), tile(&latin, index));
    }
    // The Western atlas's extended row follows its ASCII/Shift-JIS section.
    for byte in 128_u8..=255 {
        let bytes = [byte];
        let (text, _, invalid) = encoding_rs::WINDOWS_1252.decode(&bytes);
        if !invalid && text.chars().all(|ch| !ch.is_control()) {
            tiles.insert(
                text.chars().next().unwrap(),
                tile(&latin, u32::from(byte) + 160),
            );
        }
    }
    let width = 416;
    let height = (tiles.len() as u32).div_ceil(16) * 26;
    let mut atlas = image::RgbaImage::new(width, height);
    atlas.put_pixel(0, 0, image::Rgba([255; 4]));
    let mut glyphs = BTreeMap::new();
    for (index, (character, pixels)) in tiles.into_iter().enumerate() {
        let x = index as u32 % 16 * 26 + 1;
        let y = index as u32 / 16 * 26 + 1;
        image::imageops::replace(&mut atlas, &pixels, i64::from(x), i64::from(y));
        let advance = if character == ' ' {
            12
        } else if character.is_ascii() || character <= '\u{ff}' {
            pixels
                .enumerate_pixels()
                .filter(|(_, _, p)| p[3] > 32)
                .map(|(x, _, _)| x + 2)
                .max()
                .unwrap_or(12)
                .min(24)
        } else {
            24
        };
        glyphs.insert(
            character,
            Glyph {
                rect: [x, y, 24, 24],
                advance,
            },
        );
    }
    let kana = executable.slice(0x8ad820, 66 * 2)?;
    for byte in 0xa1_u8..=0xdf {
        let code = u16::from_be_bytes(
            kana[usize::from((byte & 0x7f) - 32) * 2..][..2]
                .try_into()
                .unwrap(),
        );
        let code = code.to_be_bytes();
        let (full, _, invalid) = encoding_rs::SHIFT_JIS.decode(&code);
        ensure!(
            !invalid && full.chars().count() == 1,
            "invalid kana font mapping"
        );
        let glyph = glyphs
            .get(&full.chars().next().unwrap())
            .context("missing kana glyph")?
            .clone();
        glyphs.insert(
            char::from_u32(0xff61 + u32::from(byte - 0xa1)).unwrap(),
            glyph,
        );
    }
    // Use the same missing-page fallback as the GameCube importer.
    let fallback = glyphs.get(&'□').context("missing fallback glyph")?.clone();
    glyphs.insert('\u{fffd}', fallback.clone());
    for value in 0x8440u16..=0xfcfc {
        let bytes = value.to_be_bytes();
        let (decoded, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes);
        let mut characters = decoded.chars();
        if !invalid
            && let Some(character) = characters.next()
            && characters.next().is_none()
        {
            glyphs.entry(character).or_insert_with(|| fallback.clone());
        }
    }
    let font = BitmapFont {
        texture: "fonts/dialogue.ktx2".into(),
        width,
        height,
        line_height: 24,
        glyphs,
    };
    font.validate()?;
    ui_images::write(output, &font.texture, &atlas)?;
    output::json(&output.join("fonts/dialogue.json"), &font)?;
    Ok(font)
}
