//! Two-bit bitmap font atlas and proportional glyph metrics.
use crate::{digest, dol, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::font::{BitmapFont, DialogueArt, Glyph, MovieSubtitles};
mod layout;
pub(crate) use layout::system_texture;
use std::{collections::BTreeMap, fs, path::Path};

/// System strings store literal palette changes, rather than script expressions.
pub(crate) fn system_text(bytes: &[u8]) -> Result<Vec<resonance_content::font::TextSpan>> {
    use resonance_content::font::TextSpan;
    let mut end = bytes
        .iter()
        .position(|&v| v == 0)
        .context("unterminated system text")?;
    // The system formatter terminates its last line with a newline. It does not
    // create an extra empty line in the displayed window.
    if end > 0 && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    let mut spans = Vec::new();
    let (mut start, mut at, mut color) = (0, 0, 0);
    while at <= end {
        if at == end || bytes[at] == 3 {
            if start < at {
                let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes[start..at]);
                ensure!(!invalid, "invalid system text encoding");
                let span = TextSpan {
                    text: text.into_owned(),
                    color,
                };
                span.validate()?;
                spans.push(span);
            }
            if at == end {
                break;
            }
            ensure!(at + 1 < end, "truncated system text color");
            color = match bytes[at + 1] {
                0x39 => 0, // Restore the normal white text palette.
                value @ 1..=6 => value,
                _ => anyhow::bail!("unsupported system text palette"),
            };
            at += 2;
            start = at;
        } else {
            at += 1;
        }
    }
    Ok(spans)
}

#[cfg(test)]
mod system_text_tests {
    use super::*;

    #[test]
    fn literal_colors_and_line_endings() {
        let spans = system_text(b"Press\x03\x04 A\x03\x39 to\ncontinue.\n\0").unwrap();
        assert_eq!(
            spans
                .iter()
                .map(|s| (s.text.as_str(), s.color))
                .collect::<Vec<_>>(),
            [("Press", 0), (" A", 4), (" to\ncontinue.", 0)]
        );
        for invalid in [
            b"unterminated".as_slice(),
            b"\x03\0",
            b"\x03\x07text\0",
            b"\x81\0",
        ] {
            assert!(system_text(invalid).is_err(), "{invalid:?}");
        }
    }
}

pub(crate) struct PreparedDialogue {
    pub font: BitmapFont,
    pub art: DialogueArt,
}

pub(crate) use layout::prepare;

/// Dialogue repertoire, subtitles, windows and selection layout, without field preparation.
pub(crate) fn cook_embedded(extracted: &Path, output: &Path) -> Result<()> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    read_font(extracted, &executable)?.publish(output)?;
    write_atomic(
        &output.join("embedded/dialogue.json"),
        &serde_json::to_vec_pretty(&layout::Recipe::read(&executable)?)?,
    )
}

struct DecodedFont {
    font: BitmapFont,
    pixels: Vec<u8>,
    subtitles: MovieSubtitles,
}

impl DecodedFont {
    fn publish(self, output: &Path) -> Result<BitmapFont> {
        crate::texture::cook(
            self.font.width,
            self.font.height,
            &self.pixels,
            &output.join(&self.font.texture),
        )?;
        for (path, bytes) in [
            (
                "fonts/dialogue.json",
                serde_json::to_vec_pretty(&self.font)?,
            ),
            (
                "ui/story-subtitles.json",
                serde_json::to_vec_pretty(&self.subtitles)?,
            ),
        ] {
            write_atomic(&output.join(path), &bytes)?;
        }
        Ok(self.font)
    }
}

fn read_font(extracted: &Path, executable: &[u8]) -> Result<DecodedFont> {
    let directory = crate::font_directory::Directory::read(executable)?;
    let files = extracted.join("files");
    let source = fs::read(files.join(crate::field_resources::resolve_path(
        &files,
        &directory.startup,
    )?))?;
    crate::font_directory::validate_size(source.len() as u64)?;
    let colors = directory.palette.map(rgb5a3);
    // Cook the font's complete first three Shift-JIS pages, including both
    // quotation marks. A single sampled conversation is not a font inventory.
    let mut repertoire = (32u8..127)
        .map(char::from)
        .map(|character| Ok((character, directory.metrics.code(character)?)))
        .collect::<Result<Vec<_>>>()?;
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
    // This font has one shared bitmap for characters outside its three pages.
    // Cook it once and declare its aliases up front, so cooking a field with
    // Japanese debug text cannot resize the atlas used by every other field.
    repertoire.push(('\u{fffd}', 0xffff));
    let (width, height) = (416, (repertoire.len() as u32).div_ceil(16) * 26);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    // A white texel in the outer gutter supports ordinary solid UI quads.
    rgba[..4].fill(255);
    let mut glyphs = BTreeMap::new();
    let mut native_glyphs = BTreeMap::new();
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
        let advance = directory.metrics.advance(code);
        let glyph = Glyph {
            rect: [x, y, 24, 24],
            advance,
        };
        native_glyphs.insert(code, glyph.clone());
        glyphs.insert(character, glyph);
    }
    // Single-byte kana retain their Unicode identity and share the native
    // bitmap selected by the alternate character table.
    for character in '\u{ff61}'..='\u{ff9f}' {
        let code = directory.metrics.code(character)?;
        glyphs.insert(
            character,
            native_glyphs
                .get(&code)
                .context("halfwidth font bitmap is missing")?
                .clone(),
        );
    }
    let fallback = glyphs.remove(&'\u{fffd}').unwrap();
    for character in fallback_characters() {
        glyphs.entry(character).or_insert_with(|| fallback.clone());
    }
    for skit in crate::all_assets::skits::definitions_from_source(executable)? {
        ensure!(
            skit.title.chars().all(|c| glyphs.contains_key(&c)),
            "source font cannot represent skit {}",
            skit.id
        );
    }
    let texture = "fonts/dialogue.ktx2";
    let font = BitmapFont {
        version: 1,
        texture: texture.into(),
        width,
        height,
        line_height: 24,
        glyphs,
        source_sha256: digest(&source),
        executable_sha256: digest(executable),
    };
    font.validate()?;
    let subtitles = read_subtitles(executable, &font)?;
    Ok(DecodedFont {
        font,
        pixels: rgba,
        subtitles,
    })
}
fn fallback_characters() -> impl Iterator<Item = char> {
    (0x8440u16..=0xfcfc).filter_map(|code| {
        let bytes = code.to_be_bytes();
        let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes);
        let mut chars = text.chars();
        let character = chars.next()?;
        (!invalid && chars.next().is_none()).then_some(character)
    })
}

fn read_subtitles(executable: &[u8], font: &BitmapFont) -> Result<MovieSubtitles> {
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
                let scale = if resonance_content::font::is_single_byte(character) {
                    18. / 17.
                } else {
                    1.
                };
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
    Ok(track)
}

/// Decode a menu symbol into the same RGBA pixels as the shared text atlas.
pub(crate) fn glyph_image(
    extracted: &Path,
    executable: &[u8],
    character: u8,
) -> Result<(u32, u32, Vec<u8>)> {
    ensure!((32..127).contains(&character), "menu symbol is not ASCII");
    let directory = crate::font_directory::Directory::read(executable)?;
    let code = directory.metrics.code(char::from(character))?;
    let colors = directory.palette.map(rgb5a3);
    let files = extracted.join("files");
    let source = fs::read(files.join(crate::field_resources::resolve_path(
        &files,
        &directory.startup,
    )?))?;
    crate::font_directory::validate_size(source.len() as u64)?;
    Ok((
        24,
        24,
        decode_glyph(&source, code)?
            .into_iter()
            .flat_map(|pixel| colors[usize::from(pixel)])
            .collect(),
    ))
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
pub(crate) fn rgb5a3(v: u16) -> [u8; 4] {
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
    #[ignore = "requires both extracted discs; validates font pixels and every field message without output"]
    fn original_font_covers_every_field_and_native_halfwidth_bitmap() -> Result<()> {
        use symphonia_script::{message, scenario};
        let mut messages = 0;
        for disc in [1, 2] {
            let extracted = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let directory = crate::font_directory::Directory::read(&executable)?;
            let files = extracted.join("files");
            let source = fs::read(files.join(crate::field_resources::resolve_path(
                &files,
                &directory.startup,
            )?))?;
            let decoded = read_font(&extracted, &executable)?;
            for byte in 0xa1u8..=0xdf {
                let bytes = [byte];
                let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes);
                ensure!(!invalid, "invalid halfwidth encoding");
                let character = text.chars().next().context("missing halfwidth character")?;
                let glyph = &decoded.font.glyphs[&character];
                let code = directory.metrics.alternate_codes[usize::from((byte & 0x7f) - 0x20)];
                assert_eq!(glyph.advance, directory.metrics.advance(code));
                let pixels = decode_glyph(&source, code)?;
                for row in 0..24 {
                    for column in 0..24 {
                        let [x, y, _, _] = glyph.rect;
                        let at = ((y as usize + row) * decoded.font.width as usize
                            + x as usize
                            + column)
                            * 4;
                        assert_eq!(
                            decoded.pixels[at..at + 4],
                            rgb5a3(directory.palette[usize::from(pixels[row * 24 + column])]),
                            "disc {disc} halfwidth {character:?} pixel {row}/{column}"
                        );
                    }
                }
            }
            for path in crate::field_catalogue::map_paths(&extracted)? {
                let map = crate::field::MapArchive::open(&files.join(&path))?;
                let script = map.section(6)?;
                let header = scenario::parse_header(script)?;
                let field = message::parse(&script[header.auxiliary_offset()..])?;
                validate_messages(&decoded.font, &field)
                    .with_context(|| format!("disc {disc} {path}"))?;
                messages += field.len();
            }
        }
        println!("Validated {messages} messages and all 63 halfwidth glyphs on both discs");
        Ok(())
    }

    #[test]
    fn packed_glyph_order_and_row_stride_are_preserved() {
        let mut source = vec![0; 24 * 96];
        source[0] = 0b00011011;
        source[96] = 0b11100100;
        let pixels = decode_glyph(&source, 0x8140).unwrap();
        assert_eq!(&pixels[..4], &[0, 1, 2, 3]);
        assert_eq!(&pixels[24..28], &[3, 2, 1, 0]);
        assert!(decode_glyph(&source[..10], 0x8140).is_err());
        let aliases: std::collections::BTreeSet<_> = fallback_characters().collect();
        assert!(aliases.contains(&'漢'));
        assert!(!aliases.contains(&'A'));
        assert!(!aliases.contains(&'\u{fffd}'));
        assert!(!aliases.contains(&'🙂'));
        assert!(aliases.len() < 16384 - 512);
    }
}
