//! Two-bit bitmap font atlas and proportional glyph metrics.
use crate::{digest, dol, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::font::{BitmapFont, DialogueArt, Glyph, UiTexture};
use std::{collections::BTreeMap, fs, path::Path};

pub fn cook(extracted: &Path, output: &Path, ktx: &Path) -> Result<()> {
    cook_repertoire(extracted, output, ktx, &Default::default())
}

pub(crate) fn cook_repertoire(
    extracted: &Path,
    output: &Path,
    ktx: &Path,
    required: &std::collections::BTreeSet<char>,
) -> Result<()> {
    let source = fs::read(extracted.join("files/u_f_fontb0.dat"))?;
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    // The dialogue font uses this four-color palette.
    let mapping = dol::slice(&executable, 0x801F8984, 96 * 2)?;
    let widths = dol::slice(&executable, 0x801F9680, 0x180)?;
    let palette = dol::slice(&executable, 0x801F88A0, 8)?;
    let colors: Vec<_> = palette
        .chunks_exact(2)
        .map(|p| rgb5a3(u16::from_be_bytes(p.try_into().unwrap())))
        .collect();
    // Cook the font's complete first three Shift-JIS pages, including both
    // quotation marks. A single sampled conversation is not a font inventory.
    let mut repertoire: Vec<(char, u16)> = (32u8..127)
        .map(|byte| {
            let i = usize::from(byte - 32);
            (
                char::from(byte),
                if byte == b'^' {
                    0x81a7
                } else {
                    u16::from_be_bytes(mapping[i * 2..i * 2 + 2].try_into().unwrap())
                },
            )
        })
        .collect();
    for code in 0x8140u16..0x8440 {
        if code & 255 < 0x40 {
            continue;
        }
        let bytes = code.to_be_bytes();
        let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes);
        let mut chars = text.chars();
        if !invalid
            && let Some(character) = chars.next()
            && chars.next().is_none()
            && !repertoire.iter().any(|(c, _)| *c == character)
        {
            repertoire.push((character, code));
        }
    }
    for &character in required {
        if matches!(character, '\n' | '\r' | '\u{c}')
            || repertoire.iter().any(|(c, _)| *c == character)
        {
            continue;
        }
        let text = character.to_string();
        let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(&text);
        ensure!(
            !invalid && bytes.len() == 2,
            "source font cannot represent {character:?}"
        );
        let code = u16::from_be_bytes(bytes.as_ref().try_into()?);
        // Out-of-range Shift-JIS uses the bitmap at 0x7E3C. Keep this fallback
        // for imported debug text; runtime glyphs must still be present in the atlas.
        repertoire.push((character, code));
    }
    let (width, height) = (416, (repertoire.len() as u32).div_ceil(16) * 26);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    // A white texel in the outer gutter supports ordinary solid UI quads.
    rgba[..4].fill(255);
    let mut glyphs = BTreeMap::new();
    for (i, (character, code)) in repertoire.into_iter().enumerate() {
        let pixels = decode_glyph(&source, code)?;
        let x = (i as u32 % 16) * 26 + 1;
        let y = (i as u32 / 16) * 26 + 1;
        for row in 0..24 {
            for column in 0..24 {
                let at = (((y + row) * width + x + column) * 4) as usize;
                rgba[at..at + 4]
                    .copy_from_slice(&colors[usize::from(pixels[(row * 24 + column) as usize])]);
            }
        }
        let advance = if (0x8140..=0x829A).contains(&code) {
            let index = usize::from((code >> 8) - 0x81) * 192 + usize::from((code & 255) - 0x40);
            let value = *widths.get(index).context("glyph metric outside table")?;
            if value == 0 { 24 } else { u32::from(value) }
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
    let intermediate = output.join("intermediate/fonts/dialogue.png");
    fs::create_dir_all(intermediate.parent().unwrap())?;
    image::save_buffer(&intermediate, &rgba, width, height, image::ColorType::Rgba8)?;
    fs::create_dir_all(output.join("fonts"))?;
    let texture = "fonts/dialogue.ktx2";
    crate::texture::cook(ktx, &intermediate, &output.join(texture))?;
    let font = BitmapFont {
        version: 1,
        texture: texture.into(),
        width,
        height,
        line_height: 24,
        glyphs,
        source_sha256: digest(&source),
        executable_sha256: digest(&executable),
    };
    font.validate()?;
    cook_subtitles(&executable, output, &font)?;
    write_atomic(
        &output.join("fonts/dialogue.json"),
        &serde_json::to_vec_pretty(&font)?,
    )?;
    cook_windows(extracted, output, ktx)?;
    Ok(())
}

fn cook_subtitles(executable: &[u8], output: &Path, font: &BitmapFont) -> Result<()> {
    use resonance_content::font::{MovieSubtitles, SubtitleCue, SubtitleLine};
    // Movie 1 has sixteen-byte cues with three nullable text pointers.
    let base = u32::from_be_bytes(dol::slice(executable, 0x801F9DA0, 4)?.try_into()?);
    ensure!(base == 0x801F9CCC, "unknown story subtitle table");
    let mut cues = Vec::new();
    for i in 0..13 {
        let row = dol::slice(executable, base + i * 16, 16)?;
        let frame = u32::from(u16::from_be_bytes(row[..2].try_into()?));
        let mut lines = Vec::new();
        for (line, pointer) in row[4..].chunks_exact(4).enumerate() {
            let address = u32::from_be_bytes(pointer.try_into()?);
            if address == 0 {
                continue;
            }
            let mut bytes = Vec::new();
            for offset in 0..1024 {
                let byte = dol::slice(executable, address + offset, 1)?[0];
                if byte == 0 {
                    break;
                }
                bytes.push(byte);
            }
            ensure!(bytes.len() < 1024, "unterminated subtitle");
            let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes);
            ensure!(!invalid, "invalid subtitle encoding");
            let mut width = 0;
            for character in text.chars() {
                let glyph = font
                    .glyphs
                    .get(&character)
                    .with_context(|| format!("uncooked subtitle glyph {character:?}"))?;
                let scale = if character.is_ascii() { 18. / 17. } else { 1. };
                width += (glyph.advance as f32 * scale) as i32 - 1;
            }
            lines.push(SubtitleLine {
                position: [320. - (width / 2) as f32, 240. + line as f32 * 29.],
                text: text.into_owned(),
            });
        }
        cues.push(SubtitleCue { frame, lines });
    }
    let track = MovieSubtitles {
        version: 1,
        movie: 1,
        cues,
    };
    track.validate()?;
    write_atomic(
        &output.join("ui/story-subtitles.json"),
        &serde_json::to_vec_pretty(&track)?,
    )
}

/// Compose system.tpl’s nine textures into frames, speaker tabs, and fills.
fn cook_windows(extracted: &Path, output: &Path, ktx: &Path) -> Result<()> {
    let source = fs::read(extracted.join("files/system.tpl"))?;
    let mut textures = Vec::new();
    fs::create_dir_all(output.join("ui"))?;
    fs::create_dir_all(output.join("intermediate/ui"))?;
    for (index, (width, height, pixels)) in crate::tpl::decode(&source)?.into_iter().enumerate() {
        let png = output.join(format!("intermediate/ui/system-{index}.png"));
        image::save_buffer(&png, &pixels, width, height, image::ColorType::Rgba8)?;
        let path = format!("ui/system-{index}.ktx2");
        crate::texture::cook(ktx, &png, &output.join(&path))?;
        textures.push(UiTexture {
            path,
            width,
            height,
        });
    }
    // The default selection style chooses an embedded cursor atlas.
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let mode = (dol::slice(&executable, 0x80219cf8, 8)?[7] >> 4) & 3;
    let (address, size, index) = match mode {
        0 => (0x80249500, 0x280, 0),
        1 => (0x80249280, 0x280, 0),
        2 => (0x80236e80, 0x2500, 13),
        _ => anyhow::bail!("unsupported default cursor mode"),
    };
    let cursor_bank = dol::slice(&executable, address, size)?;
    let decoded = crate::tpl::decode(cursor_bank)?;
    let (width, height, pixels) = decoded.get(index).context("default cursor is missing")?;
    let png = output.join("intermediate/ui/choice-cursor.png");
    image::save_buffer(&png, pixels, *width, *height, image::ColorType::Rgba8)?;
    let path = "ui/choice-cursor.ktx2";
    crate::texture::cook(ktx, &png, &output.join(path))?;
    let art = DialogueArt {
        version: 2,
        font: "fonts/dialogue.json".into(),
        textures,
        cursor: UiTexture {
            path: path.into(),
            width: *width,
            height: *height,
        },
        selection: resonance_content::font::SelectionArt {
            mode,
            color: dol::slice(&executable, 0x8019ad10 + u32::from(mode) * 28 + 24, 4)?
                .try_into()?,
            row_offsets: dol::slice(&executable, 0x801ac004, 9)?
                .iter()
                .map(|v| *v as i8)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            bob_amplitude: f32::from_be_bytes(
                dol::slice(
                    &executable,
                    if mode == 0 { 0x8035d8b0 } else { 0x8035d8b4 },
                    4,
                )?
                .try_into()?,
            ),
            bob_step: f32::from_be_bytes(dol::slice(&executable, 0x8035d8a8, 4)?.try_into()?)
                * if mode == 0 { 2. } else { 1. }
                / f32::from_be_bytes(dol::slice(&executable, 0x8035d8ac, 4)?.try_into()?),
        },
        source_sha256: digest(&source),
    };
    art.validate()?;
    write_atomic(
        &output.join("ui/dialogue.json"),
        &serde_json::to_vec_pretty(&art)?,
    )?;
    Ok(())
}
fn decode_glyph(source: &[u8], code: u16) -> Result<[u8; 576]> {
    let base = if (0x8140..0x8440).contains(&code) && (code & 255) >= 0x40 {
        usize::from((code >> 8) - 0x81) * 0x6c00
            + usize::from(((code & 0xf0) - 0x40) >> 4) * 0x900
            + usize::from(code & 15) * 6
    } else {
        0x7e3c
    };
    let mut pixels = [0; 576];
    for row in 0..24 {
        let bytes = source
            .get(base + row * 96..base + row * 96 + 6)
            .context("glyph exceeds source bitmap")?;
        for column in 0..24 {
            pixels[row * 24 + column] = (bytes[column / 4] >> (6 - (column % 4) * 2)) & 3;
        }
    }
    Ok(pixels)
}

pub(crate) fn validate_messages(
    font: &BitmapFont,
    messages: &[symphonia_script::message::Message],
) -> Result<()> {
    for (index, message) in messages.iter().enumerate() {
        for token in &message.tokens {
            if let symphonia_script::message::Token::Text { text } = token {
                for character in text.chars().filter(|c| !matches!(c, '\n' | '\r' | '\u{c}')) {
                    ensure!(
                        font.glyphs.contains_key(&character),
                        "message {index} needs uncooked glyph {character:?}"
                    );
                }
            }
        }
    }
    Ok(())
}
fn rgb5a3(v: u16) -> [u8; 4] {
    if v & 0x8000 != 0 {
        let channel = |shift: u32| {
            let c = ((v >> shift) & 31u16) as u8;
            (c << 3) | (c >> 2)
        };
        [channel(10), channel(5), channel(0), 255]
    } else {
        let a = ((v >> 12) & 7) as u8;
        [
            ((v >> 8) & 15) as u8 * 17,
            ((v >> 4) & 15) as u8 * 17,
            (v & 15) as u8 * 17,
            (a << 5) | (a << 2) | (a >> 1),
        ]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_glyph_order_and_row_stride_are_preserved() {
        let mut source = vec![0; 24 * 96];
        source[0] = 0b00011011;
        source[96] = 0b11100100;
        let pixels = decode_glyph(&source, 0x8140).unwrap();
        assert_eq!(&pixels[..4], &[0, 1, 2, 3]);
        assert_eq!(&pixels[24..28], &[3, 2, 1, 0]);
        assert!(decode_glyph(&source[..10], 0x8140).is_err());
    }
}
