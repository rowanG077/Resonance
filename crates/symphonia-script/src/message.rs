//! Scenario auxiliary messages, adapted from the author's message-table tooling.
//! Text is decoded once for cooking; expressions and unknown bytes stay explicit.
use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Token {
    Text { text: String },
    Control { opcode: u8, expression: Vec<u8> },
    Raw { value: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub tokens: Vec<Token>,
}

/// Offsets are relative to the auxiliary region, with 16- or 32-bit entries.
/// Validate the complete candidate table; its first offset also gives its size.
pub fn parse(region: &[u8]) -> Result<Vec<Message>, MessageError> {
    let offsets = [4, 2]
        .into_iter()
        .find_map(|width| offsets(region, width))
        .ok_or(MessageError::Table)?;
    offsets
        .iter()
        .enumerate()
        .map(|(index, &start)| {
            let end = offsets
                .iter()
                .copied()
                .find(|&o| o > start)
                .unwrap_or(region.len());
            parse_body(&region[start..end], index)
        })
        .collect()
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

fn flush(bytes: &mut Vec<u8>, tokens: &mut Vec<Token>) {
    if bytes.is_empty() {
        return;
    }
    let (text, _, errors) = SHIFT_JIS.decode(bytes);
    if errors {
        tokens.extend(bytes.iter().copied().map(|value| Token::Raw { value }));
    } else {
        tokens.push(Token::Text {
            text: text.into_owned(),
        });
    }
    bytes.clear();
}

fn parse_body(body: &[u8], index: usize) -> Result<Message, MessageError> {
    let mut tokens = Vec::new();
    let mut text = Vec::new();
    let mut cursor = 0;
    while let Some(&byte) = body.get(cursor) {
        cursor += 1;
        match byte {
            0 => {
                flush(&mut text, &mut tokens);
                return Ok(Message { tokens });
            }
            // Skip padding without escaping the next byte, which may be a voice control.
            0x1f => {}
            1..=9 | 0x11 | 0x12 => {
                flush(&mut text, &mut tokens);
                // Control expressions may start at either byte
                // parity and finds their literal 20 FF terminator. Keep them as
                // compatibility payload, never treat their embedded NULs as text end.
                let size = body[cursor..]
                    .windows(2)
                    .position(|pair| pair == [0x20, 0xff])
                    .ok_or(MessageError::Expression(index))?
                    + 2;
                tokens.push(Token::Control {
                    opcode: byte,
                    expression: body[cursor..cursor + size].to_vec(),
                });
                cursor += size;
            }
            0x81..=0x9f | 0xe0..=0xfc => {
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
                    expression: vec![0, 2, 0x30, 0, 0x20, 0xff]
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
        assert!(parse(&[0, 0, 0, 4]).is_err());
        assert!(parse(&[0, 4, 0, 3, b'A', 0]).is_err());
        assert!(parse(&[0, 2, b'A']).is_err());
    }
}
