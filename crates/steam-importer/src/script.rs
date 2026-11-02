//! Normalize Steam scenario storage at import, keeping the shared VM format.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;
pub use symphonia_script::message::TextEncoding;

pub struct Script {
    pub bytes: Vec<u8>,
    pub messages: Vec<symphonia_script::message::Message>,
}

/// Prepare canonical bytecode and Unicode messages through the shared decoders.
pub(crate) fn decode(bytes: &[u8], encoding: TextEncoding) -> Result<Script> {
    let bytes = normalize(bytes, encoding)?;
    let header = symphonia_script::scenario::parse_header(&bytes)?;
    symphonia_script::Program::decode(&bytes)?;
    let messages = if header.auxiliary_words == 0 {
        Vec::new()
    } else {
        symphonia_script::message::parse_with_encoding(
            &bytes[header.auxiliary_offset()..],
            encoding,
        )?
    };
    Ok(Script { bytes, messages })
}

/// Preserve PCs and all source content while changing numeric storage to the
/// existing canonical bytecode representation. Registry entries and message
/// offsets are 32-bit values; code and control expressions are 16-bit words.
/// Text remains byte-for-byte intact. Script patches are applied separately.
pub(crate) fn normalize(bytes: &[u8], encoding: TextEncoding) -> Result<Vec<u8>> {
    ensure!(
        (8..=4 * 1024 * 1024).contains(&bytes.len()),
        "invalid Steam scenario length"
    );
    let mut result = bytes.to_vec();
    swap_words(&mut result[..8]);
    let header = symphonia_script::scenario::parse_header(&result)?;
    let registry_end = 8 + usize::from(header.registry_count) * 12;
    let code_start = header.code_base();
    let code_end = if header.auxiliary_words == 0 {
        bytes.len()
    } else {
        header.auxiliary_offset()
    };
    ensure!(
        registry_end <= code_start
            && code_start <= code_end
            && code_end <= bytes.len()
            && code_end.is_multiple_of(2),
        "invalid Steam scenario regions"
    );
    for field in result[8..registry_end].chunks_exact_mut(4) {
        field.reverse();
    }
    swap_words(&mut result[code_start..code_end]);
    if header.auxiliary_words != 0 {
        normalize_messages(&mut result[code_end..], encoding)?;
    }
    Ok(result)
}

fn swap_words(bytes: &mut [u8]) {
    for word in bytes.chunks_exact_mut(2) {
        word.swap(0, 1);
    }
}

fn offsets(bytes: &[u8], width: usize) -> Option<Vec<usize>> {
    let read = |at: usize| {
        let field = bytes.get(at..)?.get(..width)?;
        Some(if width == 4 {
            u32::from_le_bytes(field.try_into().ok()?) as usize
        } else {
            u16::from_le_bytes(field.try_into().ok()?) as usize
        })
    };
    let size = read(0)?;
    if size == 0 || !size.is_multiple_of(width) || size > bytes.len() || size / width > 65536 {
        return None;
    }
    let mut result = Vec::with_capacity(size / width);
    for at in (0..size).step_by(width) {
        let value = read(at)?;
        if value < size || value >= bytes.len() || result.last().is_some_and(|&last| last > value) {
            return None;
        }
        result.push(value);
    }
    Some(result)
}

fn normalize_messages(bytes: &mut [u8], encoding: TextEncoding) -> Result<()> {
    if bytes.is_empty() || bytes == [0, 0] || bytes.starts_with(&[0; 4]) {
        return Ok(());
    }
    let (width, offsets) = [4, 2]
        .into_iter()
        .find_map(|width| offsets(bytes, width).map(|offsets| (width, offsets)))
        .context("invalid Steam message table")?;
    let table_end = offsets.len() * width;
    for field in bytes[..table_end].chunks_exact_mut(width) {
        field.reverse();
    }
    // Shared message offsets must only be normalized once.
    let offsets: Vec<_> = offsets
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    for (index, &start) in offsets.iter().enumerate() {
        let end = offsets.get(index + 1).copied().unwrap_or(bytes.len());
        let body = &mut bytes[start..end];
        let mut at = 0;
        let mut terminated = false;
        while let Some(&byte) = body.get(at) {
            at += 1;
            match byte {
                0 => {
                    terminated = true;
                    break;
                }
                1..=9 | 0x11 | 0x12 => {
                    let length = body[at..]
                        .chunks_exact(2)
                        .take(2048)
                        .position(|word| word == [0xff, 0x20])
                        .context("unterminated Steam message expression")?
                        * 2
                        + 2;
                    swap_words(&mut body[at..at + length]);
                    at += length;
                }
                byte if encoding.takes_trailing_byte(byte, body.get(at).copied()) => {
                    ensure!(
                        body.get(at).is_some_and(|&byte| byte != 0),
                        "truncated Steam message character"
                    );
                    at += 1;
                }
                _ => (),
            }
        }
        ensure!(terminated, "unterminated Steam message");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia_script::{
        Program,
        message::{self, Expression, Token},
    };

    #[test]
    fn registry_and_aliased_unaligned_message_expressions_keep_their_values() -> Result<()> {
        let mut native = Vec::new();
        for word in [10_u16, 0, 11, 1] {
            native.extend(word.to_le_bytes());
        }
        for word in [1_u32, 0x12345678, 0] {
            native.extend(word.to_le_bytes());
        }
        native.extend(0x20ff_u16.to_le_bytes());
        native.extend(8_u32.to_le_bytes());
        native.extend(8_u32.to_le_bytes());
        native.extend(b"Hi\x1f\x09");
        for word in [0x0200_u16, 0x1234, 0x000b, 0x3000, 0x20ff] {
            native.extend(word.to_le_bytes());
        }
        native.extend(b"!\0");
        let canonical = normalize(&native, TextEncoding::ShiftJis)?;
        let program = Program::decode(&canonical)?;
        assert_eq!(program.event(1, 0x12345678), Some(0));
        let messages = message::parse(&canonical[22..])?;
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], messages[1]);
        assert_eq!(
            messages[0].tokens[1],
            Token::Control {
                opcode: 9,
                expression: Expression::Constant(0x000b1234)
            }
        );
        assert_eq!(messages[0].tokens[0], Token::Text { text: "Hi".into() });
        Ok(())
    }

    #[test]
    fn malformed_regions_and_expressions_are_rejected() {
        assert!(normalize(&[], TextEncoding::ShiftJis).is_err());
        assert!(normalize(&[4, 0, 0, 0, 3, 0, 0, 0], TextEncoding::ShiftJis).is_err());
        let native = [4, 0, 0, 0, 5, 0, 0, 0, 0xff, 0x20, 4, 0, 0, 0, 9, 0, 0];
        assert!(normalize(&native, TextEncoding::ShiftJis).is_err());
        // Unknown programs are never repaired merely for having a bad branch.
        assert!(
            decode(
                &[4, 0, 0, 0, 0, 0, 0, 0, 1, 0x20, 0xff, 0xff],
                TextEncoding::ShiftJis
            )
            .is_err()
        );
    }

    #[test]
    fn mixed_english_skit_text_preserves_japanese_punctuation_and_western_accents() -> Result<()> {
        let mut native = vec![4, 0, 0, 0, 5, 0, 0, 0, 0xff, 0x20, 4, 0, 0, 0];
        // The accent immediately precedes a control expression, so treating it
        // as a Shift-JIS lead byte would also corrupt the numeric byte order.
        native.extend(b"caf\xe9\x09\x01\x00\x00\x30\xff\x20 \x81gOK\x81h\x82\xa0\0");
        let script = decode(&native, TextEncoding::ShiftJisWithWesternFallback)?;
        assert_eq!(
            script.messages[0].tokens,
            [
                Token::Text {
                    text: "café".into()
                },
                Token::Control {
                    opcode: 9,
                    expression: Expression::Constant(1)
                },
                Token::Text {
                    text: " “OK”あ".into()
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn western_text_retains_accents_next_to_control_expressions() -> Result<()> {
        let mut native = vec![4, 0, 0, 0, 5, 0, 0, 0, 0xff, 0x20, 4, 0, 0, 0];
        native.extend(b"Z\xe9los: caf\xe9\x09\x01\x00\x00\x30\xff\x20!\0");
        let script = decode(&native, TextEncoding::Windows1252)?;
        assert_eq!(
            script.messages[0].tokens[0],
            Token::Text {
                text: "Zélos: café".into()
            }
        );
        assert_eq!(
            script.messages[0].tokens[1],
            Token::Control {
                opcode: 9,
                expression: Expression::Constant(1)
            }
        );
        assert_eq!(
            script.messages[0].tokens[2],
            Token::Text { text: "!".into() }
        );
        Ok(())
    }
}
