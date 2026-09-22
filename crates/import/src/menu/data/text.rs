use super::*;
use crate::dol;
use resonance_content::menu_data::{MenuSpan, MenuText};

/// Keep inline controls and their operands while decoding the surrounding text.
/// A zero button/color operand does not terminate a paragraph.
pub(crate) fn source(executable: &[u8], address: u32) -> Result<(String, u32)> {
    let byte = |at| -> Result<u8> {
        Ok(dol::slice(
            executable,
            address
                .checked_add(at)
                .context("menu text address overflow")?,
            1,
        )?[0])
    };
    let mut text = String::new();
    let (mut start, mut at) = (0, 0);
    loop {
        ensure!(at < 65536, "unterminated menu text");
        let value = byte(at)?;
        if matches!(value, 0 | 11 | 12) {
            if start < at {
                let (part, _, invalid) = encoding_rs::SHIFT_JIS.decode(dol::slice(
                    executable,
                    address + start,
                    (at - start) as usize,
                )?);
                ensure!(!invalid, "invalid menu text encoding");
                text.push_str(&part);
            }
            match value {
                0 => {
                    return Ok((
                        text,
                        address
                            .checked_add(at + 1)
                            .context("menu text address overflow")?,
                    ));
                }
                11 | 12 => {
                    text.push(char::from(value));
                    at += 1;
                    text.push(char::from(byte(at)?));
                }
                _ => unreachable!(),
            }
            start = at + 1;
        }
        at += 1;
    }
}

pub(super) fn decode(text: &str, mut color: u8) -> Result<MenuText> {
    let mut lines = vec![Vec::new()];
    let mut start = 0;
    let mut chars = text
        .char_indices()
        .chain(std::iter::once((text.len(), '\0')));
    while let Some((at, value)) = chars.next() {
        if matches!(value, '\0' | '\n' | '\u{b}' | '\u{c}') {
            if start < at {
                lines.last_mut().unwrap().push(MenuSpan::Text {
                    text: text[start..at].into(),
                    color,
                });
            }
            match value {
                '\0' => {
                    ensure!(at == text.len(), "unexpected menu text terminator");
                    break;
                }
                '\n' => lines.push(Vec::new()),
                _ => {
                    let (operand_at, operand) = chars
                        .next()
                        .filter(|(at, _)| *at < text.len())
                        .context("missing menu text operand")?;
                    let operand = u8::try_from(u32::from(operand))?;
                    if value == '\u{b}' {
                        lines
                            .last_mut()
                            .unwrap()
                            .push(MenuSpan::Button { sprite: operand });
                    } else {
                        color = operand;
                    }
                    start = operand_at + char::from(operand).len_utf8();
                    continue;
                }
            }
            start = at + value.len_utf8();
        }
    }
    let text = MenuText { lines };
    text.validate()?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executable(text: &[u8]) -> Vec<u8> {
        let mut dol = vec![0; 256];
        dol[..4].copy_from_slice(&256u32.to_be_bytes());
        dol[0x48..0x4c].copy_from_slice(&0x80000000u32.to_be_bytes());
        dol[0x90..0x94].copy_from_slice(&(text.len() as u32).to_be_bytes());
        dol.extend(text);
        dol
    }

    #[test]
    fn menu_codes_become_spans_and_truncated_or_unknown_codes_fail() {
        // A zero icon/color operand is data, not the paragraph terminator.
        let bytes = executable(b"Use \x0b\0\x0c\0 now\nAgain\0Next\0\0");
        let (cooked, end) = source(&bytes, 0x80000000).unwrap();
        assert_eq!(cooked, "Use \u{b}\0\u{c}\0 now\nAgain");
        let first = decode(&cooked, 9).unwrap();
        assert_eq!(
            serde_json::to_value(first).unwrap(),
            serde_json::json!({"lines":[
                [{"kind":"text","text":"Use ","color":9},{"kind":"button","sprite":0},
                 {"kind":"text","text":" now","color":0}],
                [{"kind":"text","text":"Again","color":0}]
            ]})
        );
        let second = decode(&source(&bytes, end).unwrap().0, 9).unwrap();
        assert!(matches!(&second.lines[0][0], MenuSpan::Text { text, color: 9 } if text == "Next"));
        let count = decode(
            &source(&executable(b"Remaining:\x0c\x09%d\0"), 0x80000000)
                .unwrap()
                .0,
            8,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(count).unwrap(),
            serde_json::json!({"lines":[[
                {"kind":"text","text":"Remaining:","color":8},
                {"kind":"text","text":"%d","color":9}
            ]]})
        );
        for text in [
            &b"unterminated"[..],
            b"\x0b",
            b"\x0c",
            b"\x0b\x20\0",
            b"\x0c\x0bA\0",
            b"\x01\0",
        ] {
            assert!(
                source(&executable(text), 0x80000000)
                    .and_then(|(text, _)| decode(&text, 9))
                    .is_err(),
                "accepted malformed text {text:?}"
            );
        }
        for text in ["\u{b}", "\u{c}", "\0", "\u{b}\u{100}"] {
            assert!(decode(text, 9).is_err());
        }
        let japanese = executable(b"\x82\xa0\x0c\x08\x82\xa2\0");
        let (cooked, end) = source(&japanese, 0x80000000).unwrap();
        assert_eq!(cooked, "あ\u{c}\u{8}い");
        assert_eq!(end, 0x80000007);
        assert_eq!(
            decode(&cooked, 9).unwrap().texts().collect::<Vec<_>>(),
            ["あ", "い"]
        );
    }
}
