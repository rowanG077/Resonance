use super::*;
use resonance_content::menu_data::{MenuSpan, MenuText};

/// Parse one terminated paragraph, returning the next paragraph's address.
pub(super) fn paragraph(executable: &[u8], address: u32) -> Result<(MenuText, u32)> {
    paragraph_color(executable, address, 9)
}

pub(super) fn paragraph_color(
    executable: &[u8],
    address: u32,
    mut color: u8,
) -> Result<(MenuText, u32)> {
    let byte = |at| -> Result<u8> {
        Ok(dol::slice(
            executable,
            address
                .checked_add(at)
                .context("menu text address overflow")?,
            1,
        )?[0])
    };
    let mut lines = vec![Vec::new()];
    let (mut start, mut at) = (0, 0);
    loop {
        ensure!(at < 65536, "unterminated menu text");
        let value = byte(at)?;
        if matches!(value, 0 | 10..=12) {
            if start < at {
                let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(dol::slice(
                    executable,
                    address + start,
                    (at - start) as usize,
                )?);
                ensure!(!invalid, "invalid menu text encoding");
                lines.last_mut().unwrap().push(MenuSpan::Text {
                    text: text.into_owned(),
                    color,
                });
            }
            match value {
                0 => {
                    let text = MenuText { lines };
                    text.validate()?;
                    return Ok((
                        text,
                        address
                            .checked_add(at + 1)
                            .context("menu text address overflow")?,
                    ));
                }
                10 => lines.push(Vec::new()),
                11 => {
                    at += 1;
                    lines
                        .last_mut()
                        .unwrap()
                        .push(MenuSpan::Button { sprite: byte(at)? });
                }
                12 => {
                    at += 1;
                    color = byte(at)?;
                }
                _ => unreachable!(),
            }
            start = at + 1;
        }
        at += 1;
    }
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
        let (first, next) = paragraph(&bytes, 0x80000000).unwrap();
        assert_eq!(
            serde_json::to_value(first).unwrap(),
            serde_json::json!({"lines":[
                [{"kind":"text","text":"Use ","color":9},{"kind":"button","sprite":0},
                 {"kind":"text","text":" now","color":0}],
                [{"kind":"text","text":"Again","color":0}]
            ]})
        );
        let (second, _) = paragraph(&bytes, next).unwrap();
        assert!(matches!(&second.lines[0][0], MenuSpan::Text { text, color: 9 } if text == "Next"));
        let (count, _) =
            paragraph_color(&executable(b"Remaining:\x0c\x09%d\0"), 0x80000000, 8).unwrap();
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
                paragraph(&executable(text), 0x80000000).is_err(),
                "accepted malformed text {text:?}"
            );
        }
    }
}
