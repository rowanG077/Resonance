//! Scenario auxiliary messages, adapted from the author's message-table tooling.
//! Source text and expressions are decoded once for cooking.
mod document;
mod expression;
use encoding_rs::{SHIFT_JIS, WINDOWS_1252};
pub use expression::Expression;
use std::borrow::Cow;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MessageError {
    #[error("invalid message offset table")]
    Table,
    #[error("message {0} has no terminator")]
    Terminator(usize),
    #[error("message {0} contains a truncated escape or character")]
    Character(usize),
    #[error("message {0} contains an unterminated control expression")]
    Expression(usize),
    #[error("message {0}: {1}")]
    ExpressionValue(usize, String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Text { text: String },
    Control { opcode: u8, expression: Expression },
    Raw { value: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub tokens: Vec<Token>,
}

/// Import-time text encodings. Cooked auxiliary text and message documents use UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    ShiftJis,
    Windows1252,
    /// English Steam skits retain Shift-JIS punctuation alongside isolated
    /// Western characters that are not valid Shift-JIS sequences.
    ShiftJisWithWesternFallback,
}

impl TextEncoding {
    pub fn is_lead_byte(self, byte: u8) -> bool {
        matches!(self, Self::ShiftJis | Self::ShiftJisWithWesternFallback)
            && matches!(byte, 0x81..=0x9f | 0xe0..=0xfc)
    }

    pub fn takes_trailing_byte(self, byte: u8, next: Option<u8>) -> bool {
        self.is_lead_byte(byte)
            && (self != Self::ShiftJisWithWesternFallback
                || next.is_some_and(|next| {
                    SHIFT_JIS
                        .decode_without_bom_handling_and_without_replacement(&[byte, next])
                        .is_some()
                }))
    }

    /// Decode a source text span while leaving unsupported glyph bytes explicit.
    pub fn decode(self, bytes: &[u8]) -> Option<Cow<'_, str>> {
        match self {
            Self::Utf8 => std::str::from_utf8(bytes).ok().map(Cow::Borrowed),
            Self::ShiftJis => SHIFT_JIS.decode_without_bom_handling_and_without_replacement(bytes),
            Self::Windows1252 => {
                WINDOWS_1252.decode_without_bom_handling_and_without_replacement(bytes)
            }
            Self::ShiftJisWithWesternFallback => {
                let mut text = String::new();
                let mut at = 0;
                while at < bytes.len() {
                    let width = if self.takes_trailing_byte(bytes[at], bytes.get(at + 1).copied()) {
                        2
                    } else {
                        1
                    };
                    let part = &bytes[at..at + width];
                    if let Some(value) =
                        SHIFT_JIS.decode_without_bom_handling_and_without_replacement(part)
                    {
                        text.push_str(&value);
                    } else {
                        text.push_str(&WINDOWS_1252.decode_without_bom_handling(part).0);
                    }
                    at += width;
                }
                Some(Cow::Owned(text))
            }
        }
    }
}

/// Cooked auxiliary text uses UTF-8; original encodings are selected explicitly
/// by importers. Offsets remain relative to a 16- or 32-bit auxiliary table.
pub fn parse(region: &[u8]) -> Result<Vec<Message>, MessageError> {
    parse_with_encoding(region, TextEncoding::Utf8)
}

pub fn parse_with_encoding(
    region: &[u8],
    encoding: TextEncoding,
) -> Result<Vec<Message>, MessageError> {
    // A zero first offset means no message entries. The auxiliary region can
    // still contain alignment and other script data after that empty table.
    if region.is_empty() || region == [0, 0] || region.starts_with(&[0, 0, 0, 0]) {
        return Ok(Vec::new());
    }
    let (_, offsets) = auxiliary_offsets(region).ok_or(MessageError::Table)?;
    offsets
        .iter()
        .enumerate()
        .map(|(index, &start)| {
            let end = offsets
                .iter()
                .copied()
                .find(|&o| o > start)
                .unwrap_or(region.len());
            parse_body(&region[start..end], index, encoding)
        })
        .collect()
}

/// Validated width and entries for an auxiliary string/message offset table.
/// The first offset gives the table size. Entries are nondecreasing; duplicates are aliases.
pub fn auxiliary_offsets(region: &[u8]) -> Option<(usize, Vec<usize>)> {
    [4, 2]
        .into_iter()
        .find_map(|width| offsets(region, width).map(|values| (width, values)))
}

fn offsets(region: &[u8], width: usize) -> Option<Vec<usize>> {
    let read = |at: usize| -> Option<usize> {
        let bytes = region.get(at..at + width)?;
        Some(if width == 4 {
            u32::from_be_bytes(bytes.try_into().ok()?) as usize
        } else {
            u16::from_be_bytes(bytes.try_into().ok()?) as usize
        })
    };
    let size = read(0)?;
    if size == 0 || !size.is_multiple_of(width) || size > region.len() || size / width > 65536 {
        return None;
    }
    let mut result = Vec::with_capacity(size / width);
    for at in (0..size).step_by(width) {
        let value = read(at)?;
        if value < size || value >= region.len() || result.last().is_some_and(|&last| last > value)
        {
            return None;
        }
        result.push(value);
    }
    Some(result)
}

fn flush(bytes: &mut Vec<u8>, tokens: &mut Vec<Token>, encoding: TextEncoding) {
    if bytes.is_empty() {
        return;
    }
    if let Some(text) = encoding.decode(bytes) {
        tokens.push(Token::Text {
            text: text.into_owned(),
        });
    } else {
        tokens.extend(bytes.iter().copied().map(|value| Token::Raw { value }));
    }
    bytes.clear();
}

fn parse_body(body: &[u8], index: usize, encoding: TextEncoding) -> Result<Message, MessageError> {
    let mut tokens = Vec::new();
    let mut text = Vec::new();
    let mut cursor = 0;
    while let Some(&byte) = body.get(cursor) {
        cursor += 1;
        match byte {
            0 => {
                flush(&mut text, &mut tokens, encoding);
                return Ok(Message { tokens });
            }
            // Skip padding without escaping the next byte, which may be a voice control.
            0x1f => {}
            1..=9 | 0x11 | 0x12 => {
                flush(&mut text, &mut tokens, encoding);
                // Control expressions may start at either byte
                // parity. Their embedded NULs are operands, not text terminators.
                let size = body[cursor..]
                    .windows(2)
                    .position(|pair| pair == [0x20, 0xff])
                    .ok_or(MessageError::Expression(index))?
                    + 2;
                tokens.push(Token::Control {
                    opcode: byte,
                    expression: Expression::from_native(&body[cursor..cursor + size])
                        .map_err(|error| MessageError::ExpressionValue(index, error))?,
                });
                cursor += size;
            }
            byte if encoding.takes_trailing_byte(byte, body.get(cursor).copied()) => {
                let &next = body
                    .get(cursor)
                    .filter(|&&b| b != 0)
                    .ok_or(MessageError::Character(index))?;
                text.extend([byte, next]);
                cursor += 1;
            }
            _ => text.push(byte),
        }
    }
    Err(MessageError::Terminator(index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn western_text_and_shift_jis_have_independent_character_boundaries() {
        let western = b"\0\0\0\x04caf\xe9\0";
        assert_eq!(
            parse_with_encoding(western, TextEncoding::Windows1252).unwrap()[0].tokens,
            [Token::Text {
                text: "café".into()
            }]
        );
        assert!(parse_with_encoding(western, TextEncoding::ShiftJis).is_err());
        let japanese = b"\0\0\0\x04\x82\xa0\0";
        assert_eq!(
            parse_with_encoding(japanese, TextEncoding::ShiftJis).unwrap()[0].tokens,
            [Token::Text { text: "あ".into() }]
        );
    }

    #[test]
    fn cooked_auxiliary_text_uses_utf8_without_an_encoding_selector() {
        let mut bytes = vec![0, 0, 0, 4];
        bytes.extend("羽 café".as_bytes());
        bytes.push(0);
        assert_eq!(
            parse(&bytes).unwrap()[0].tokens,
            [Token::Text {
                text: "羽 café".into()
            }]
        );
    }

    #[test]
    fn expressions_containing_zero_and_padding_do_not_terminate_text() {
        let mut bytes = vec![0, 0, 0, 4];
        bytes.extend(b"Hello ");
        bytes.extend([0x1f, 1, 0, 2, 0x30, 0, 0x20, 0xff, 0x1f, b'!', 0]);
        let messages = parse(&bytes).unwrap();
        assert_eq!(
            messages[0].tokens,
            [
                Token::Text {
                    text: "Hello ".into()
                },
                Token::Control {
                    opcode: 1,
                    expression: Expression::Constant(2)
                },
                Token::Text { text: "!".into() },
            ]
        );
    }

    #[test]
    fn short_tables_aliases_and_invalid_ranges() {
        let mut bytes = vec![0, 6, 0, 6, 0, 8];
        bytes.extend(b"A\0B\0");
        let messages = parse(&bytes).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0], messages[1]);
        assert_ne!(messages[0], messages[2]);
        assert!(parse(&[]).unwrap().is_empty());
        assert!(parse(&[0; 6]).unwrap().is_empty());
        assert!(parse(&[0; 18]).unwrap().is_empty());
        assert!(
            parse(&[0, 0, 0, 0, 0, 0, 0, 1, 0xfe, 0xfe])
                .unwrap()
                .is_empty()
        );
        assert!(parse(&[0, 0, 0, 4]).is_err());
        assert!(parse(&[0, 4, 0, 3, b'A', 0]).is_err());
        assert!(parse(&[0, 2, b'A']).is_err());
    }
}
