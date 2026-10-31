//! GameCube dialogue glyphs and story captions into their final editable assets.
use crate::{dol, native_archive, text, world_texture};
use anyhow::{Context, Result, ensure};
use resonance_content::font::{
    BitmapFont, DialogueArt, Glyph, MovieSubtitles, SelectionArt, SubtitleCue, SubtitleLine,
    UiTexture,
};
use std::{collections::BTreeMap, fs, path::Path};

const FONT_FILE: u32 = 0x8017_cc00;
const PALETTE: u32 = 0x801f_88a0;
const KANA: u32 = 0x801f_8900;
const ASCII: u32 = 0x801f_8984;
const ADVANCES: u32 = 0x801f_9680;

fn code(exe: &[u8], character: char) -> Result<u16> {
    if character == '^' {
        return Ok(0x81a7);
    }
    let text = character.to_string();
    let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(&text);
    ensure!(!invalid, "unrepresentable font character {character:?}");
    match *encoded {
        [byte] => {
            let table = if byte & 0x80 == 0 { ASCII } else { KANA };
            dol::u16(
                dol::slice(exe, table, if table == ASCII { 192 } else { 132 })?,
                usize::from((byte & 0x7f).max(32) - 32) * 2,
            )
        }
        [high, low] => Ok(u16::from_be_bytes([high, low])),
        _ => anyhow::bail!("invalid font character {character:?}"),
    }
}

fn advance(exe: &[u8], code: u16) -> Result<u32> {
    if !(0x8140..=0x829a).contains(&code) {
        return Ok(24);
    }
    let index = usize::from((code >> 8) - 0x81) * 192 + usize::from(code & 255) - 64;
    let width = dol::slice(exe, ADVANCES, 288)?[index];
    Ok(if width == 0 { 24 } else { u32::from(width) })
}

fn rgb5a3(value: u16) -> [u8; 4] {
    if value & 0x8000 != 0 {
        let channel = |shift| {
            let v = ((value >> shift) & 31u16) as u8;
            (v << 3) | (v >> 2)
        };
        [channel(10), channel(5), channel(0), 255]
    } else {
        let alpha = ((value >> 12) & 7) as u8;
        [
            ((value >> 8) & 15) as u8 * 17,
            ((value >> 4) & 15) as u8 * 17,
            (value & 15) as u8 * 17,
            (alpha << 5) | (alpha << 2) | (alpha >> 1),
        ]
    }
}

fn glyph(source: &[u8], code: u16) -> Result<[u8; 576]> {
    let base = if (0x8140..0x8440).contains(&code) && code & 255 >= 0x40 {
        usize::from((code >> 8) - 0x81) * 0x6c00
            + usize::from(((code & 0xf0) - 0x40) >> 4) * 0x900
            + usize::from(code & 15) * 6
    } else {
        0x7e3c
    };
    let mut result = [0; 576];
    for row in 0..24 {
        let packed = source
            .get(base + row * 96..base + row * 96 + 6)
            .context("font glyph exceeds native bitmap")?;
        for column in 0..24 {
            result[row * 24 + column] = (packed[column / 4] >> (6 - column % 4 * 2)) & 3;
        }
    }
    Ok(result)
}

/// The menu atlas embeds one dialogue-font marker using the same native palette.
pub(crate) fn menu_glyph(
    exe: &[u8],
    extracted: &Path,
    character: char,
) -> Result<(u32, u32, Vec<u8>)> {
    let filename = text::string(exe, FONT_FILE)?;
    let source = native_archive::read_named(&extracted.join("disc1/files"), &filename)?;
    let palette = dol::slice(exe, PALETTE, 32)?;
    let colors = std::array::from_fn::<_, 16, _>(|i| {
        rgb5a3(u16::from_be_bytes([palette[i * 2], palette[i * 2 + 1]]))
    });
    let rgba = glyph(&source, code(exe, character)?)?
        .into_iter()
        .flat_map(|pixel| colors[usize::from(pixel)])
        .collect();
    Ok((24, 24, rgba))
}

fn repertoire(exe: &[u8]) -> Result<Vec<(char, u16)>> {
    let mut entries = (32u8..127)
        .map(char::from)
        .map(|character| Ok((character, code(exe, character)?)))
        .collect::<Result<Vec<_>>>()?;
    for value in 0x8140u16..0x8440 {
        if value & 255 < 0x40 {
            continue;
        }
        let bytes = value.to_be_bytes();
        let (decoded, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes);
        let mut characters = decoded.chars();
        if !invalid
            && let Some(character) = characters.next()
            && characters.next().is_none()
            && !entries.iter().any(|(existing, _)| *existing == character)
        {
            entries.push((character, value));
        }
    }
    entries.push(('\u{fffd}', 0xffff));
    Ok(entries)
}

fn font(exe: &[u8], source: &[u8]) -> Result<(BitmapFont, Vec<u8>)> {
    ensure!(
        !source.is_empty() && source.len().is_multiple_of(96 * 24),
        "invalid native font bitmap length"
    );
    let palette = dol::slice(exe, PALETTE, 32)?;
    let colors = std::array::from_fn::<_, 16, _>(|i| {
        rgb5a3(u16::from_be_bytes([palette[i * 2], palette[i * 2 + 1]]))
    });
    let entries = repertoire(exe)?;
    let width = 416;
    let height = (entries.len() as u32).div_ceil(16) * 26;
    let mut rgba = vec![0; (width * height * 4) as usize];
    rgba[..4].fill(255);
    let mut glyphs = BTreeMap::new();
    let mut native = BTreeMap::new();
    for (i, (character, value)) in entries.into_iter().enumerate() {
        let x = i as u32 % 16 * 26 + 1;
        let y = i as u32 / 16 * 26 + 1;
        for (pixel, index) in glyph(source, value)?.into_iter().zip(0..) {
            let column = index % 24;
            let row = index / 24;
            let at = (((y + row) * width + x + column) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&colors[usize::from(pixel)]);
        }
        let item = Glyph {
            rect: [x, y, 24, 24],
            advance: advance(exe, value)?,
        };
        native.insert(value, item.clone());
        glyphs.insert(character, item);
    }
    for character in '\u{ff61}'..='\u{ff9f}' {
        glyphs.insert(
            character,
            native
                .get(&code(exe, character)?)
                .context("halfwidth glyph is absent")?
                .clone(),
        );
    }
    let fallback = glyphs
        .remove(&'\u{fffd}')
        .context("missing fallback glyph")?;
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
    Ok((font, rgba))
}

fn subtitles(exe: &[u8], font: &BitmapFont) -> Result<MovieSubtitles> {
    let table = dol::u32(dol::slice(exe, 0x801f_9da0, 4)?, 0)?;
    ensure!(table == 0x801f_9ccc, "unknown story caption table");
    let mut cues = Vec::with_capacity(13);
    for index in 0..13 {
        let row = dol::slice(exe, table + index * 16, 16)?;
        let frame = u32::from(dol::u16(row, 0)?);
        let mut lines = Vec::new();
        for line in 0..3 {
            let address = dol::u32(row, 4 + line * 4)?;
            if address == 0 {
                continue;
            }
            let caption = text::string(exe, address)?;
            if caption.is_empty() {
                continue;
            }
            let mut width = 0;
            for character in caption.chars() {
                let glyph = font
                    .glyphs
                    .get(&character)
                    .with_context(|| format!("story caption needs uncooked glyph {character:?}"))?;
                let scale = if resonance_content::font::is_single_byte(character) {
                    18. / 17.
                } else {
                    1.
                };
                width += (glyph.advance as f32 * scale) as i32 - 1;
            }
            lines.push(SubtitleLine {
                position: [320. - (width / 2) as f32, 240. + line as f32 * 29.],
                text: caption,
            });
        }
        cues.push(SubtitleCue { frame, lines });
    }
    let track = MovieSubtitles { cues };
    track.validate()?;
    Ok(track)
}

pub fn cook_dialogue_font(extracted: &Path, library: &Path) -> Result<()> {
    let disc = extracted.join("disc1");
    let exe = fs::read(disc.join("sys/main.dol"))?;
    let filename = text::string(&exe, FONT_FILE)?;
    let source = native_archive::read_named(&disc.join("files"), &filename)?;
    let (font, rgba) = font(&exe, &source)?;
    let captions = subtitles(&exe, &font)?;
    for (relative, bytes) in [
        (
            "fonts/dialogue.json",
            resonance_asset_writer::json::encode(&font)?,
        ),
        (
            "fonts/dialogue.ktx2",
            resonance_asset_writer::ktx2::encode_rgba8(font.width, font.height, &rgba)?,
        ),
        (
            "ui/story-subtitles.json",
            resonance_asset_writer::json::encode(&captions)?,
        ),
    ] {
        let path = library.join(relative);
        crate::output::write(&path, &bytes)?;
    }
    cook_font_styles(extracted, library)
}

const WINDOW_IMAGES: [&str; 9] = [
    "ui/dialogue/symbols.ktx2",
    "ui/dialogue/window-shading.ktx2",
    "ui/windows/cracked-stone.ktx2",
    "ui/windows/rough-stone.ktx2",
    "ui/windows/fine-grain.ktx2",
    "ui/windows/speckled.ktx2",
    "ui/windows/small-stones.ktx2",
    "ui/dialogue/lined-fill.ktx2",
    "ui/windows/white-fill.ktx2",
];

/// Retain complete basic/Kanji font atlases, including alternate native styles.
/// Their original 24-pixel cell grid stays editable without repeated glyph maps.
pub fn cook_font_styles(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let directory = extracted.join("disc1/files");
    let palette = dol::slice(&exe, PALETTE, 8)?;
    let colors = std::array::from_fn::<_, 4, _>(|i| {
        rgb5a3(u16::from_be_bytes([palette[i * 2], palette[i * 2 + 1]]))
    });
    let mut files = (0..16)
        .map(|index| text::pointer(&exe, 0x801f_88c0 + index * 4))
        .collect::<Result<Vec<_>>>()?;
    let table = files.clone();
    for entry in fs::read_dir(&directory)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if (name.ends_with("fontb0.dat") || name.ends_with("fontb1.dat")) && !files.contains(&name)
        {
            files.push(name);
        }
    }
    let style = |name: &str| {
        name.strip_suffix("fontb0.dat")
            .or_else(|| name.strip_suffix("fontb1.dat"))
            .unwrap()
            .trim_end_matches('_')
            .replace('_', "-")
    };
    let mut unique = BTreeMap::<Vec<u8>, String>::new();
    let mut images = BTreeMap::new();
    let mut styles = BTreeMap::<String, BTreeMap<String, String>>::new();
    for source in files {
        if images.contains_key(&source) {
            continue;
        }
        let bytes = fs::read(directory.join(&source))?;
        ensure!(
            !bytes.is_empty() && bytes.len().is_multiple_of(96 * 24),
            "invalid font atlas {source}"
        );
        let role = if source.ends_with("fontb0.dat") {
            "basic"
        } else {
            "kanji"
        };
        let name = style(&source);
        let name = if name.is_empty() {
            "standard".to_owned()
        } else {
            name
        };
        let path = if let Some(path) = unique.get(&bytes) {
            path.clone()
        } else {
            let rgba = bytes
                .iter()
                .flat_map(|byte| {
                    [6, 4, 2, 0]
                        .into_iter()
                        .flat_map(move |shift| colors[usize::from(byte >> shift & 3)])
                })
                .collect::<Vec<_>>();
            let path = format!("fonts/styles/{name}-{role}.ktx2");
            write_asset(
                library,
                &path,
                &resonance_asset_writer::ktx2::encode_rgba8(384, (bytes.len() / 96) as u32, &rgba)?,
            )?;
            unique.insert(bytes, path.clone());
            path
        };
        styles
            .entry(name)
            .or_default()
            .insert(role.into(), path.clone());
        images.insert(source, path);
    }
    let mut mappings = BTreeMap::new();
    for index in 0..8 {
        let name = style(&table[index]);
        let name = if name.is_empty() {
            "standard".to_owned()
        } else {
            name
        };
        styles.insert(
            name.clone(),
            BTreeMap::from([
                ("basic".into(), images[&table[index]].clone()),
                ("kanji".into(), images[&table[index + 8]].clone()),
            ]),
        );
        mappings.insert(index as u32, name);
    }
    write_asset(
        library,
        "fonts/mappings.json",
        &resonance_asset_writer::json::encode(&mappings)?,
    )?;
    write_asset(
        library,
        "fonts/styles.json",
        &resonance_asset_writer::json::encode(
            &serde_json::json!({"cell": [24,24], "styles": styles}),
        )?,
    )
}

fn image(tpl: &[u8], index: usize, path: &str) -> Result<(UiTexture, Vec<u8>)> {
    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "invalid dialogue TPL");
    let table = dol::u32(tpl, 8)? as usize;
    ensure!(index < dol::u32(tpl, 4)? as usize, "missing dialogue image");
    let descriptor = dol::u32(tpl, table + index * 8)? as usize;
    let height = u32::from(dol::u16(tpl, descriptor)?);
    let width = u32::from(dol::u16(tpl, descriptor + 2)?);
    let format = dol::u32(tpl, descriptor + 4)?;
    let bytes = if format == 14 {
        world_texture::tpl_texture(tpl, index)?.0
    } else {
        ensure!(matches!(format, 2 | 8), "unsupported dialogue image format");
        let (block_width, block_height): (usize, usize) = if format == 2 { (8, 4) } else { (8, 8) };
        let columns = (width as usize).div_ceil(block_width);
        let rows = (height as usize).div_ceil(block_height);
        let source = dol::u32(tpl, descriptor + 8)? as usize;
        let packed = tpl
            .get(source..source + columns * rows * 32)
            .context("truncated dialogue image")?;
        let colors = if format == 8 {
            let header = dol::u32(tpl, table + index * 8 + 4)? as usize;
            ensure!(header != 0, "dialogue cursor has no palette");
            let count = usize::from(dol::u16(tpl, header)?);
            ensure!(
                dol::u32(tpl, header + 4)? == 2,
                "unsupported cursor palette"
            );
            let start = dol::u32(tpl, header + 8)? as usize;
            (0..count)
                .map(|i| dol::u16(tpl, start + i * 2).map(rgb5a3))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let mut rgba = vec![0; (width * height * 4) as usize];
        for (block_index, block) in packed.chunks_exact(32).enumerate() {
            let left = block_index % columns * block_width;
            let top = block_index / columns * block_height;
            for y in 0..block_height {
                for x in 0..block_width {
                    if left + x >= width as usize || top + y >= height as usize {
                        continue;
                    }
                    let pixel = y * block_width + x;
                    let color = if format == 2 {
                        let value = block[pixel];
                        let intensity = (value & 15) * 17;
                        [intensity, intensity, intensity, (value >> 4) * 17]
                    } else {
                        let value = block[pixel / 2];
                        let id = if pixel % 2 == 0 {
                            value >> 4
                        } else {
                            value & 15
                        };
                        *colors
                            .get(usize::from(id))
                            .context("dialogue cursor palette index is missing")?
                    };
                    let at = ((top + y) * width as usize + left + x) * 4;
                    rgba[at..at + 4].copy_from_slice(&color);
                }
            }
        }
        resonance_asset_writer::ktx2::encode_rgba8(width, height, &rgba)?
    };
    Ok((
        UiTexture {
            path: path.into(),
            width,
            height,
        },
        bytes,
    ))
}

fn write_asset(library: &Path, relative: &str, bytes: &[u8]) -> Result<()> {
    crate::output::write(&library.join(relative), bytes)
}

pub fn cook_dialogue_art(extracted: &Path, library: &Path) -> Result<()> {
    let disc = extracted.join("disc1");
    let exe = fs::read(disc.join("sys/main.dol"))?;
    let source =
        native_archive::read_named(&disc.join("files"), &text::string(&exe, 0x8017_a55c)?)?;
    let mut textures = Vec::new();
    for (index, path) in WINDOW_IMAGES.into_iter().enumerate() {
        let (image, bytes) = image(&source, index, path)?;
        write_asset(library, path, &bytes)?;
        textures.push(image);
    }

    let settings = dol::slice(&exe, 0x8021_9cf8, 8)?;
    let mode = (settings[7] >> 4) & 3;
    ensure!(mode <= 2, "unsupported native dialogue style");
    let (address, length, cursor_index) = match mode {
        0 => (0x8024_9500, 0x280, 0),
        1 => (0x8024_9280, 0x280, 0),
        _ => (0x8023_6e80, 0x2500, 13),
    };
    let cursor_path = "ui/cursors/gold-blade.ktx2";
    let (cursor, bytes) = image(
        dol::slice(&exe, address, length)?,
        cursor_index,
        cursor_path,
    )?;
    write_asset(library, cursor_path, &bytes)?;
    let motion = dol::slice(&exe, 0x8035_d8a8, 16)?;
    let float = |offset| -> Result<f32> {
        let value = f32::from_bits(dol::u32(motion, offset)?);
        ensure!(value.is_finite(), "invalid dialogue cursor motion");
        Ok(value)
    };
    let divisor = float(4)?;
    ensure!(divisor != 0., "zero dialogue cursor phase divisor");
    let rows = dol::slice(&exe, 0x801a_c004, 9)?;
    let selection = SelectionArt {
        mode,
        color: dol::slice(&exe, 0x8019_ad10 + u32::from(mode) * 28 + 24, 4)?.try_into()?,
        row_offsets: std::array::from_fn(|i| rows[i] as i8),
        bob_amplitude: float(if mode == 0 { 8 } else { 12 })?,
        bob_step: float(0)? * if mode == 0 { 2. } else { 1. } / divisor,
    };
    let art = DialogueArt {
        font: "fonts/dialogue.json".into(),
        textures,
        cursor,
        selection,
    };
    art.validate()?;
    write_asset(
        library,
        "ui/dialogue.json",
        &resonance_asset_writer::json::encode(&art)?,
    )
}
