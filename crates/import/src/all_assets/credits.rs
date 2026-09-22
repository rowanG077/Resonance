//! Credits text and layout controls, consumed by the original credits renderer.
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub(crate) struct Resources {
    pub text: String,
    pictures: String,
    backgrounds: String,
    music: String,
}

impl Resources {
    pub fn read(executable: &[u8]) -> Result<Self> {
        let resources = Self {
            text: crate::dol::text(executable, 0x801dfacc)?,
            pictures: crate::dol::text(executable, 0x801dfa80)?,
            backgrounds: crate::dol::text(executable, 0x801dfa90)?,
            music: crate::dol::text(executable, 0x801dfad8)?,
        };
        for path in [
            &resources.text,
            &resources.pictures,
            &resources.backgrounds,
            &resources.music,
        ] {
            resonance_content::validate_asset_path(path)?;
        }
        Ok(resources)
    }
}

pub(crate) fn cook_resources(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    crate::embedded::write(
        file,
        output,
        "credits-resources",
        &Resources::read(executable)?,
    )
}

#[derive(Debug, Serialize)]
pub(crate) struct Credits {
    version: u8,
    canvas: [u16; 2],
    style: Style,
    scroll: Scroll,
    operations: Vec<Operation>,
}

/// Undeclared text needs a layout command to distinguish it from ordinary text.
/// Declared credits bypass detection and report malformed input directly.
pub(crate) fn detect(bytes: &[u8]) -> Option<Credits> {
    let credits = decode(bytes).ok()?;
    credits
        .operations
        .iter()
        .any(|op| {
            matches!(
                op,
                Operation::CenterLine | Operation::VerticalSpace { .. } | Operation::Picture { .. }
            )
        })
        .then_some(credits)
}

#[derive(Debug, Serialize)]
struct Style {
    glyph_size: [u16; 2],
    color: [u8; 4],
    line_height: u16,
    tab_width: u16,
}

#[derive(Debug, Serialize)]
struct Scroll {
    height_pixels: i32,
    /// Speed is height_pixels / speed_divisor_ticks pixels per update.
    speed_divisor_ticks: u32,
    /// Scrolling stops once its offset reaches height_pixels - stop_margin_pixels.
    stop_margin_pixels: u16,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Operation {
    Text {
        text: String,
    },
    /// Center the remaining line using the original font's glyph advances.
    CenterLine,
    Newline,
    Tab,
    /// Reset X and advance Y; the command's terminating newline is consumed.
    VerticalSpace {
        pixels: i32,
    },
    /// Draw at the current cursor without advancing it.
    Picture {
        index: u8,
    },
    /// The original reports these controls but performs no layout operation.
    IgnoredControl {
        code: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        argument: Option<u8>,
    },
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Credits> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    ensure!(
        bytes[end..].iter().all(|byte| *byte == 0),
        "data after credits terminator"
    );
    let bytes = &bytes[..end];
    let mut operations = Vec::new();
    let mut text = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if matches!(byte, 0x81..=0x9f | 0xe0..=0xfb) {
            text.extend_from_slice(
                bytes
                    .get(cursor..cursor + 2)
                    .context("truncated credits Shift-JIS character")?,
            );
            cursor += 2;
            continue;
        }
        cursor += 1;
        if !matches!(byte, b'@' | b'\n' | b'\t') {
            // CR follows the ordinary glyph path and maps to a space. It also
            // contributes to centered line width, so CRLF must not be trimmed.
            text.push(if byte == b'\r' { b' ' } else { byte });
            continue;
        }
        flush_text(&mut text, &mut operations)?;
        let op = match byte {
            b'\n' => Operation::Newline,
            b'\t' => Operation::Tab,
            b'@' => {
                let code = *bytes.get(cursor).context("truncated credits control")?;
                cursor += 1;
                match code {
                    b'c' | b'C' => Operation::CenterLine,
                    b'\\' => {
                        let (pixels, next) = spacing(bytes, cursor)?;
                        cursor = next;
                        Operation::VerticalSpace { pixels }
                    }
                    b'p' | b'P' | b'w' | b'W' => {
                        let argument = *bytes
                            .get(cursor)
                            .context("truncated credits control argument")?;
                        cursor += 1;
                        if matches!(code, b'p' | b'P') {
                            Operation::Picture {
                                index: decimal(&[argument]) as u8,
                            }
                        } else {
                            Operation::IgnoredControl {
                                code,
                                argument: Some(argument),
                            }
                        }
                    }
                    _ => Operation::IgnoredControl {
                        code,
                        argument: None,
                    },
                }
            }
            _ => unreachable!(),
        };
        operations.push(op);
    }
    flush_text(&mut text, &mut operations)?;
    // Scrolling speed follows the authored height, independently of drawing.
    Ok(Credits {
        version: 1,
        canvas: [640, 480],
        style: Style {
            glyph_size: [23, 25],
            color: [255; 4],
            line_height: 25,
            tab_width: 96,
        },
        scroll: Scroll {
            height_pixels: height(bytes)?,
            speed_divisor_ticks: 21_600,
            stop_margin_pixels: 640,
        },
        operations,
    })
}

fn spacing(bytes: &[u8], cursor: usize) -> Result<(i32, usize)> {
    let end = bytes[cursor..]
        .iter()
        .position(|&b| b == b'\n')
        .context("unterminated credits vertical spacing")?
        + cursor;
    // Only the first 62 bytes reach the numeric parser; the whole line is consumed.
    Ok((decimal(&bytes[cursor..end.min(cursor + 62)]), end + 1))
}

/// The scrolling prepass only interprets spacing controls. A newline used as a
/// picture/ignored argument still contributes height even though drawing skips it.
fn height(bytes: &[u8]) -> Result<i32> {
    let mut height = 0i32;
    let mut cursor = 0;
    while let Some(&byte) = bytes.get(cursor) {
        let advance = match byte {
            b'@' if bytes.get(cursor + 1) == Some(&b'\\') => {
                let (pixels, next) = spacing(bytes, cursor + 2)?;
                cursor = next;
                pixels
            }
            b'\n' => {
                cursor += 1;
                25
            }
            _ => {
                cursor += if matches!(byte, 0x81..=0x9f | 0xe0..=0xfb) {
                    2
                } else {
                    1
                };
                0
            }
        };
        height = height
            .checked_add(advance)
            .context("credits height overflow")?;
    }
    Ok(height)
}

/// Decimal prefix parsing used by the original controls, including saturation.
fn decimal(mut bytes: &[u8]) -> i32 {
    while matches!(bytes.first(), Some(b' ' | b'\t'..=b'\r')) {
        bytes = &bytes[1..];
    }
    let negative = bytes.first() == Some(&b'-');
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        bytes = &bytes[1..];
    }
    let limit = i32::MAX as u32 + u32::from(negative);
    let value = bytes
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .fold(0u32, |value, byte| {
            value
                .saturating_mul(10)
                .saturating_add(u32::from(byte - b'0'))
                .min(limit)
        });
    if negative {
        value.wrapping_neg() as i32
    } else {
        value as i32
    }
}

fn flush_text(text: &mut Vec<u8>, operations: &mut Vec<Operation>) -> Result<()> {
    if !text.is_empty() {
        let (decoded, _, invalid) = encoding_rs::SHIFT_JIS.decode(text);
        ensure!(!invalid, "invalid credits Shift-JIS text");
        operations.push(Operation::Text {
            text: decoded.into_owned(),
        });
        text.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn credits_controls_follow_decimal_prefix_and_separate_height_rules() -> Result<()> {
        for (bytes, expected) in [
            (b" +12 trailing".as_slice(), 12),
            (b"-6\r", -6),
            (b"invalid", 0),
            (b"\x0b\x0c\r\t+7 trailing", 7),
            (b"999999999999", i32::MAX),
            (b"-999999999999", i32::MIN),
        ] {
            assert_eq!(decimal(bytes), expected);
        }
        let credits = decode(b"@p\n@w\n@\\ +12 suffix\r\n")?;
        assert_eq!(credits.scroll.height_pixels, 62);
        assert_eq!(credits.operations[0], Operation::Picture { index: 0 });
        assert_eq!(
            credits.operations[2],
            Operation::VerticalSpace { pixels: 12 }
        );
        let mut long = b"@\\6".to_vec();
        long.extend([b' '; 80]);
        long.extend(b"42\n");
        assert_eq!(decode(&long)?.scroll.height_pixels, 6);
        assert!(detect(b"ordinary prose").is_none());
        assert!(detect(b"@cCredits\n").is_some());
        assert!(detect(b"@cInvalid\0payload").is_none());
        Ok(())
    }

    #[test]
    fn credits_resources_follow_native_declarations_with_arbitrary_names() -> Result<()> {
        let mut executable = vec![0; 0x180];
        for (offset, value) in [(0x1c, 0x100u32), (0x64, 0x801dfa80), (0xac, 0x80)] {
            executable[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        for (offset, name) in [
            (0, "pictures.gfx"),
            (16, "wall.gfx"),
            (76, "story.note"),
            (88, "ending.bin"),
        ] {
            executable[0x100 + offset..0x100 + offset + name.len()]
                .copy_from_slice(name.as_bytes());
        }
        let resources = Resources::read(&executable)?;
        assert_eq!(resources.text, "story.note");
        assert_eq!(resources.pictures, "pictures.gfx");
        assert_eq!(resources.backgrounds, "wall.gfx");
        assert_eq!(resources.music, "ending.bin");
        let root = crate::temporary_path(&std::env::temp_dir().join("credits-declaration"));
        fs::create_dir_all(root.join("files"))?;
        fs::write(root.join("files/story.note"), b"@cCredits\n")?;
        let result = super::super::roles::credits_path(&root, &executable);
        fs::remove_dir_all(&root)?;
        assert_eq!(result?, "story.note");
        assert!(super::super::roles::credits_path(&root, &executable).is_err());
        let invalid = b"../bad.text\0";
        executable[0x14c..0x14c + invalid.len()].copy_from_slice(invalid);
        assert!(Resources::read(&executable).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; checks credits data without rendering"]
    fn original_credits_resources_and_programs_preserve_both_languages() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let extracted = local.join(format!("disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let resources = Resources::read(&executable)?;
            for (path, expected) in [
                (&resources.text, "US_end.txt"),
                (&resources.pictures, "end_roll.tpl"),
                (&resources.backgrounds, "end_roll_wall.tpl"),
                (&resources.music, "tos_ending.adx"),
            ] {
                assert_eq!(path, expected);
                assert!(extracted.join("files").join(path).is_file());
            }
            for (source, height, operations, digest) in [
                (
                    "US_end.txt",
                    20325,
                    2212,
                    "d2e1a625b418bf15381a67a6285564e5690edb49a18b5501b89e799ad470dd38",
                ),
                (
                    "end.txt",
                    20077,
                    2175,
                    "3f0770ad82610d82be88d2780c39b659916ef5a8550173387038d63da10cabcf",
                ),
            ] {
                let bytes = fs::read(extracted.join("files").join(source))?;
                let credits = detect(&bytes).context("undetected credits program")?;
                assert_eq!(credits.scroll.height_pixels, height);
                assert_eq!(credits.operations.len(), operations);
                let canonical = serde_json::to_vec(&serde_json::to_value(credits)?)?;
                assert_eq!(crate::digest(&canonical), digest);
            }
        }
        Ok(())
    }

    #[test]
    fn credits_preserve_spacing_picture_and_ignored_control_semantics() {
        let credits = decode(b"@cAlice\r\n@\\6\r\n@p4\r\n@6\r\n").unwrap();
        assert_eq!(credits.scroll.height_pixels, 81);
        assert_eq!(
            credits.operations,
            [
                Operation::CenterLine,
                Operation::Text {
                    text: "Alice ".into()
                },
                Operation::Newline,
                Operation::VerticalSpace { pixels: 6 },
                Operation::Picture { index: 4 },
                Operation::Text { text: " ".into() },
                Operation::Newline,
                Operation::IgnoredControl {
                    code: b'6',
                    argument: None
                },
                Operation::Text { text: " ".into() },
                Operation::Newline,
            ]
        );
        assert!(decode(b"@\\6").is_err());
        assert!(decode(b"@p").is_err());
    }

    #[test]
    fn shift_jis_trail_bytes_are_text_not_controls() {
        let credits = decode(b"@c\x81\x40\r\n").unwrap();
        assert_eq!(
            credits.operations[1],
            Operation::Text {
                text: "\u{3000} ".into()
            }
        );
        assert!(decode(b"\x81").is_err());
        assert!(decode(b"credits\0hidden").is_err());
    }
}
