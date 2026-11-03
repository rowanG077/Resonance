//! Indexed menu text. A TXB contains several languages in one physical bank;
//! its executable references, rather than its archive, choose the encoding.
use super::pe::word;
use anyhow::{Context, Result, ensure};
use std::ops::Range;

pub struct Bank<'a> {
    bytes: &'a [u8],
    strings: Vec<Range<usize>>,
    limits: Vec<usize>,
}

impl<'a> Bank<'a> {
    /// Plain native text without menu-control interpretation (for staff rolls).
    pub(crate) fn plain(
        &self,
        index: u32,
        encoding: &'static encoding_rs::Encoding,
    ) -> Result<String> {
        let range = self
            .strings
            .get(index as usize)
            .context("plain text ID exceeds bank")?;
        let (text, invalid) = encoding.decode_without_bom_handling(&self.bytes[range.clone()]);
        ensure!(!invalid, "invalid plain text encoding");
        Ok(text.into_owned())
    }
    /// Literal field-service text uses dialogue controls rather than menu controls.
    pub(super) fn system(&self, index: u32) -> Result<Vec<resonance_content::font::TextSpan>> {
        use resonance_content::font::TextSpan;
        let range = self
            .strings
            .get(index as usize)
            .context("system text ID exceeds bank")?;
        let bytes = &self.bytes[range.start..self.limits[index as usize]];
        let mut spans = Vec::new();
        let (mut at, mut start, mut color) = (0, 0, 0);
        loop {
            let byte = *bytes.get(at).context("unterminated system text")?;
            if matches!(byte, 0 | 2 | 3 | 6) {
                if start < at {
                    let (text, invalid) =
                        encoding_rs::WINDOWS_1252.decode_without_bom_handling(&bytes[start..at]);
                    ensure!(!invalid, "invalid system text encoding");
                    spans.push(TextSpan {
                        text: text.into_owned(),
                        color,
                    });
                }
                if byte == 0 {
                    break;
                }
                let operand = *bytes.get(at + 1).context("truncated system text control")?;
                match byte {
                    2 => ensure!(at == 0 && operand == 1, "unknown system window control"),
                    3 => {
                        color = match operand {
                            8 | 9 | 0x39 => 0,
                            1..=6 => operand,
                            _ => anyhow::bail!("unknown system text palette"),
                        }
                    }
                    6 => {
                        // Native save-point confirmation uses device button 18.
                        // The shared field service uses the GameCube A button.
                        ensure!(operand == 18, "unbound system button {operand}");
                        spans.push(TextSpan {
                            text: "A".into(),
                            color: 4,
                        });
                    }
                    _ => unreachable!(),
                }
                at += 2;
                start = at;
            } else {
                ensure!(
                    byte >= 32 || matches!(byte, 10 | 12),
                    "unsupported system text control"
                );
                at += 1;
            }
        }
        for span in &spans {
            span.validate()?;
        }
        Ok(spans)
    }

    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        ensure!(
            bytes.get(..4) == Some(b"BRXT") && word(bytes, 4)? == 110,
            "unsupported Steam text bank"
        );
        let base = word(bytes, 20)? as usize;
        ensure!(base >= 32, "Steam text section overlaps header");
        let section = bytes
            .get(base..)
            .context("Steam text section exceeds file")?;
        ensure!(
            section.get(..4) == Some(b"NBXT"),
            "missing Steam text strings"
        );
        let count = word(section, 4)? as usize;
        let table = word(section, 16)? as usize;
        ensure!(
            count <= 1_000_000 && table >= 32,
            "invalid Steam text directory"
        );
        let table_end = table
            .checked_add(count * 4)
            .context("Steam text directory overflow")?;
        let offsets = section
            .get(table..table_end)
            .context("truncated Steam text directory")?
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()) as usize)
            .collect::<Vec<_>>();
        ensure!(
            offsets
                .iter()
                .all(|&offset| (table_end..section.len()).contains(&offset)),
            "Steam text string overlaps directory or exceeds file"
        );
        let mut boundaries = offsets.clone();
        boundaries.push(section.len());
        boundaries.sort_unstable();
        boundaries.dedup();
        let limits = offsets
            .iter()
            .map(|&start| base + boundaries[boundaries.partition_point(|&v| v <= start)])
            .collect();
        let strings = offsets
            .into_iter()
            .map(|start| {
                let end = boundaries[boundaries.partition_point(|&v| v <= start)];
                let length = section[start..end]
                    .iter()
                    .position(|&v| v == 0)
                    .context("unterminated Steam text string")?;
                Ok(base + start..base + start + length)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            bytes,
            strings,
            limits,
        })
    }

    /// Preserve menu button/color controls and their byte operands, including
    /// zero operands. These are consumed by the shared menu text decoder.
    pub fn menu(&self, index: u32, encoding: &'static encoding_rs::Encoding) -> Result<String> {
        self.menu_with_buttons(index, encoding, Ok)
    }

    /// Resolve device-button controls in the caller's input context while
    /// keeping fixed icons and palette controls unchanged.
    pub fn menu_with_buttons(
        &self,
        index: u32,
        encoding: &'static encoding_rs::Encoding,
        button: impl Fn(u8) -> Result<u8>,
    ) -> Result<String> {
        let start = self
            .strings
            .get(index as usize)
            .context("Steam text ID exceeds bank")?
            .start;
        Ok(Self::menu_text(
            &self.bytes[start..self.limits[index as usize]],
            encoding,
            &button,
        )?
        .0)
    }

    pub(super) fn menu_paragraphs(&self, index: u32) -> Result<Vec<String>> {
        let start = self
            .strings
            .get(index as usize)
            .context("Steam text ID exceeds bank")?
            .start;
        let mut bytes = &self.bytes[start..self.limits[index as usize]];
        let mut paragraphs = Vec::new();
        loop {
            ensure!(paragraphs.len() < 32, "too many Steam menu paragraphs");
            let (text, consumed) = Self::menu_text(bytes, encoding_rs::WINDOWS_1252, &Ok)?;
            paragraphs.push(text);
            bytes = &bytes[consumed..];
            if *bytes
                .first()
                .context("missing Steam menu paragraph terminator")?
                == 0
            {
                return Ok(paragraphs);
            }
        }
    }

    fn menu_text(
        bytes: &[u8],
        encoding: &'static encoding_rs::Encoding,
        button: &impl Fn(u8) -> Result<u8>,
    ) -> Result<(String, usize)> {
        let (mut at, mut part) = (0, 0);
        let mut output = String::new();
        loop {
            ensure!(at < 65536, "unterminated Steam menu text");
            let value = *bytes.get(at).context("truncated Steam menu text")?;
            // Western menu captions retain 0x81-prefixed Shift-JIS symbols
            // (notably Colette's matching-costume title). Preserve the native
            // glyph code through Unicode, as the GameCube font importer does.
            // Other high bytes remain Western accents, not guessed lead bytes.
            let symbol = encoding == encoding_rs::WINDOWS_1252 && value == 0x81;
            if matches!(value, 0 | 11 | 12 | 13) || symbol {
                let (text, invalid) = encoding.decode_without_bom_handling(&bytes[part..at]);
                ensure!(!invalid, "invalid Steam menu text encoding");
                output.push_str(&text);
                if value == 0 {
                    return Ok((output, at + 1));
                }
                if symbol {
                    let pair = bytes
                        .get(at..at + 2)
                        .context("truncated Steam menu symbol")?;
                    let text = encoding_rs::SHIFT_JIS
                        .decode_without_bom_handling_and_without_replacement(pair)
                        .context("invalid Steam menu symbol")?;
                    output.push_str(&text);
                    at += 2;
                    part = at;
                    continue;
                }
                let mut operand = *bytes
                    .get(at + 1)
                    .context("missing Steam menu text operand")?;
                // Native 0x53233b reads the same authored GameCube button ID
                // as 0x0b, then applies Steam's device-glyph conversion. Our
                // shared captions retain the GameCube glyph before that step.
                if value == 13 {
                    ensure!(operand < 32, "unsupported Steam device button glyph");
                    operand = button(operand)?;
                    ensure!(operand < 32, "invalid normalized button glyph");
                }
                output.push(char::from(if value == 13 { 11 } else { value }));
                output.push(char::from(operand));
                at += 2;
                part = at;
            } else {
                ensure!(
                    value >= 32 || matches!(value, 9 | 10),
                    "unsupported Steam menu text control"
                );
                at += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 80];
        b[..4].copy_from_slice(b"BRXT");
        b[4..8].copy_from_slice(&110u32.to_le_bytes());
        b[20..24].copy_from_slice(&32u32.to_le_bytes());
        b[32..36].copy_from_slice(b"NBXT");
        b[36..40].copy_from_slice(&4u32.to_le_bytes());
        b[48..52].copy_from_slice(&32u32.to_le_bytes());
        for (i, offset) in [56u32, 48, 56, 62].into_iter().enumerate() {
            b[64 + i * 4..68 + i * 4].copy_from_slice(&offset.to_le_bytes());
        }
        b.extend(b"Fant\xf4me\0Yuan\0\0\0");
        b
    }

    #[test]
    fn western_menu_symbols_keep_the_native_glyph_without_consuming_accents() -> Result<()> {
        let mut bytes = fixture();
        bytes[36..40].copy_from_slice(&1_u32.to_le_bytes());
        bytes.truncate(88);
        bytes.extend_from_slice(b"Pr\xe9s\xe9a \x81\xcd\0");
        let bank = Bank::parse(&bytes)?;
        assert_eq!(bank.menu(0, encoding_rs::WINDOWS_1252)?, "Préséa ∀");
        Ok(())
    }

    #[test]
    fn menu_controls_preserve_zero_and_non_ascii_operands() -> Result<()> {
        let mut bytes = fixture();
        bytes.truncate(80);
        for at in [64, 68, 72, 76] {
            bytes[at..at + 4].copy_from_slice(&48u32.to_le_bytes());
        }
        bytes.extend(b"A\x0b\x00B\x0c\xffC\0");
        let bank = Bank::parse(&bytes)?;
        assert_eq!(bank.menu(0, encoding_rs::WINDOWS_1252)?, "A\u{b}\0B\u{c}ÿC");
        bytes.truncate(80);
        bytes.extend(b"A\x0b\0");
        assert!(
            Bank::parse(&bytes)?
                .menu(0, encoding_rs::WINDOWS_1252)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn device_button_controls_normalize_to_shared_gamecube_glyphs() -> Result<()> {
        let mut bytes = fixture();
        bytes[36..40].copy_from_slice(&1_u32.to_le_bytes());
        bytes.truncate(88);
        bytes.extend_from_slice(b"Use \r\x17 or \r\x08 and \r\0.\0");
        let bank = Bank::parse(&bytes)?;
        assert_eq!(
            bank.menu(0, encoding_rs::WINDOWS_1252)?,
            "Use \u{b}\x17 or \u{b}\x08 and \u{b}\0."
        );
        assert_eq!(
            bank.menu_with_buttons(0, encoding_rs::WINDOWS_1252, |id| Ok(if id == 8 {
                6
            } else {
                id
            }))?,
            "Use \u{b}\x17 or \u{b}\x06 and \u{b}\0."
        );
        for suffix in [&b"\r\x20\0"[..], b"\r\0"] {
            bytes.truncate(88);
            bytes.extend_from_slice(suffix);
            assert!(
                Bank::parse(&bytes)?
                    .menu(0, encoding_rs::WINDOWS_1252)
                    .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn indexed_captions_retain_aliases_empty_strings_and_language_encoding() -> Result<()> {
        let bytes = fixture();
        let bank = Bank::parse(&bytes)?;
        assert_eq!(bank.menu(0, encoding_rs::WINDOWS_1252)?, "Yuan");
        assert_eq!(bank.menu(1, encoding_rs::WINDOWS_1252)?, "Fantôme");
        assert_eq!(
            bank.menu(0, encoding_rs::WINDOWS_1252)?,
            bank.menu(2, encoding_rs::WINDOWS_1252)?
        );
        assert_eq!(bank.menu(3, encoding_rs::WINDOWS_1252)?, "");
        assert!(bank.menu(4, encoding_rs::WINDOWS_1252).is_err());
        for (offset, value) in [
            (4, 0),
            (20, 4),
            (36, u32::MAX),
            (48, 8),
            (64, 1),
            (64, u32::MAX),
        ] {
            let mut bad = bytes.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(Bank::parse(&bad).is_err());
        }
        let mut bad = bytes.clone();
        bad[80..88].fill(b'x');
        assert!(Bank::parse(&bad).is_err());
        let mut expression = bytes.clone();
        expression[88] = 5;
        assert!(
            Bank::parse(&expression)?
                .menu(0, encoding_rs::WINDOWS_1252)
                .is_err()
        );
        Ok(())
    }
}

/// Language-selected executable references into the common TXB bank.
pub(crate) struct Menu<'a> {
    pub image: super::pe::Image<'a>,
    pub bank: Bank<'a>,
    pub slot: u32,
}
impl Menu<'_> {
    pub fn word(&self, at: u32) -> Result<u32> {
        super::pe::word(self.image.slice(at, 4)?, 0)
    }
    pub fn directory(&self, at: u32) -> Result<u32> {
        self.word(at + self.slot * 4)
    }
    pub fn caption(&self, at: u32, index: u32) -> Result<String> {
        self.bank
            .menu(self.word(at + index * 4)?, encoding_rs::WINDOWS_1252)
    }
    pub fn strings(&self, at: u32, count: u32) -> Result<Vec<String>> {
        let directory = self.directory(at)?;
        (0..count).map(|i| self.caption(directory, i)).collect()
    }
}
