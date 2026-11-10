//! Scenario auxiliary messages, adapted from the author's message-table tooling.
//! Source text and expressions are decoded once for cooking.
mod document;
mod expression;
use encoding_rs::SHIFT_JIS;
pub use expression::Expression;
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

/// Offsets are relative to the auxiliary region, with 16- or 32-bit entries.
/// Validate the complete candidate table; its first offset also gives its size.
pub fn parse(region: &[u8]) -> Result<Vec<Message>, MessageError> {
    let Some(table) = offset_table(region)? else {
        return Ok(Vec::new());
    };
    table
        .offsets
        .iter()
        .enumerate()
        .map(|(index, &start)| {
            let end = table.offsets[index + 1..]
                .iter()
                .copied()
                .find(|&o| o > start)
                .unwrap_or(region.len());
            parse_body(&region[start..end], index)
        })
        .collect()
}

/// A complete validated table of big-endian byte offsets relative to its region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetTable {
    pub entry_bytes: usize,
    pub offsets: Vec<usize>,
}

pub fn offset_table(region: &[u8]) -> Result<Option<OffsetTable>, MessageError> {
    // A zero first offset denotes an empty table, possibly followed by other data.
    if region.is_empty() || region == [0, 0] || region.starts_with(&[0, 0, 0, 0]) {
        return Ok(None);
    }
    [4, 2]
        .into_iter()
        .find_map(|entry_bytes| {
            offsets(region, entry_bytes).map(|offsets| OffsetTable {
                entry_bytes,
                offsets,
            })
        })
        .map(Some)
        .ok_or(MessageError::Table)
}

/// Exact control/expression bytes, including the leading control byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlRange {
    pub opcode: u8,
    pub bytes: std::ops::Range<usize>,
}

#[derive(Debug, Default)]
pub struct SourceRanges {
    pub text: Vec<std::ops::Range<usize>>,
    pub controls: Vec<ControlRange>,
}

/// Contiguous text byte ranges in valid bodies; aliased entries share ranges.
pub fn text_ranges(region: &[u8], table: &OffsetTable) -> Vec<std::ops::Range<usize>> {
    source_ranges(region, table).text
}

/// Source locations validated by the same parser as the runtime message tokens.
/// Padding, terminators and malformed bodies remain explicit raw data.
pub fn source_ranges(region: &[u8], table: &OffsetTable) -> SourceRanges {
    let mut ranges = SourceRanges::default();
    for (index, &start) in table.offsets.iter().enumerate() {
        if index > 0 && table.offsets[index - 1] == start {
            continue;
        }
        let end = table.offsets[index + 1..]
            .iter()
            .copied()
            .find(|&o| o > start)
            .unwrap_or(region.len());
        let Some(body) = region.get(start..end) else {
            continue;
        };
        let mut local = SourceRanges::default();
        if parse_body_ranges(body, index, &mut local).is_ok() {
            ranges.text.extend(
                local
                    .text
                    .into_iter()
                    .map(|r| start + r.start..start + r.end),
            );
            ranges
                .controls
                .extend(local.controls.into_iter().map(|r| ControlRange {
                    opcode: r.opcode,
                    bytes: start + r.bytes.start..start + r.bytes.end,
                }));
        }
    }
    ranges
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
    parse_body_ranges(body, index, &mut SourceRanges::default())
}

fn parse_body_ranges(
    body: &[u8],
    index: usize,
    ranges: &mut SourceRanges,
) -> Result<Message, MessageError> {
    let mut run_start = None;
    let mut tokens = Vec::new();
    let mut text = Vec::new();
    let mut cursor = 0;
    while let Some(&byte) = body.get(cursor) {
        cursor += 1;
        if matches!(byte, 0..=9 | 0x11 | 0x12 | 0x1f) {
            if let Some(start) = run_start.take() {
                ranges.text.push(start..cursor - 1);
            }
        } else {
            run_start.get_or_insert(cursor - 1);
        }
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
                // parity. Their embedded NULs are operands, not text terminators.
                let size = body[cursor..]
                    .windows(2)
                    .position(|pair| pair == [0x20, 0xff])
                    .ok_or(MessageError::Expression(index))?
                    + 2;
                ranges.controls.push(ControlRange {
                    opcode: byte,
                    bytes: cursor - 1..cursor + size,
                });
                tokens.push(Token::Control {
                    opcode: byte,
                    expression: Expression::from_native(&body[cursor..cursor + size])
                        .map_err(|error| MessageError::ExpressionValue(index, error))?,
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
