//! Text and control decoding shared by menu owners that need it.
use crate::dol;
use anyhow::{Context, Result, ensure};
use resonance_content::menu_data::{MenuSpan, MenuText};

pub(crate) fn pointer(exe: &[u8], address: u32) -> Result<String> {
    string(exe, dol::u32(dol::slice(exe, address, 4)?, 0)?)
}

pub(crate) fn table<const N: usize>(exe: &[u8], address: u32) -> Result<[String; N]> {
    Ok(dol::slice(exe, address, N * 4)?
        .chunks_exact(4)
        .map(|row| string(exe, dol::u32(row, 0)?))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap())
}

pub(crate) fn string(exe: &[u8], address: u32) -> Result<String> {
    ensure!(address != 0, "null required menu text");
    Ok(source(exe, address)?.0)
}

/// Native button/color controls include an operand that may be zero.
pub(crate) fn source(exe: &[u8], address: u32) -> Result<(String, u32)> {
    let mut source = String::new();
    let mut at = 0u32;
    let mut start = 0u32;
    loop {
        ensure!(at < 65536, "unterminated menu text");
        let value = dol::slice(
            exe,
            address.checked_add(at).context("menu address overflow")?,
            1,
        )?[0];
        if matches!(value, 0 | 11 | 12) {
            if start < at {
                let bytes = dol::slice(exe, address + start, (at - start) as usize)?;
                let (decoded, _, invalid) = encoding_rs::SHIFT_JIS.decode(bytes);
                ensure!(!invalid, "invalid menu text encoding");
                source.push_str(&decoded);
            }
            if value == 0 {
                return Ok((
                    source,
                    address
                        .checked_add(at + 1)
                        .context("menu address overflow")?,
                ));
            }
            source.push(char::from(value));
            at += 1;
            source.push(char::from(
                dol::slice(
                    exe,
                    address.checked_add(at).context("menu address overflow")?,
                    1,
                )?[0],
            ));
            start = at + 1;
        }
        at += 1;
    }
}

pub(crate) fn decode(source: &str, mut color: u8) -> Result<MenuText> {
    let mut lines = vec![Vec::new()];
    let mut start = 0;
    let mut chars = source
        .char_indices()
        .chain(std::iter::once((source.len(), '\0')));
    while let Some((at, value)) = chars.next() {
        if matches!(value, '\0' | '\n' | '\u{b}' | '\u{c}') {
            if start < at {
                lines.last_mut().unwrap().push(MenuSpan::Text {
                    text: source[start..at].into(),
                    color,
                });
            }
            match value {
                '\0' => break,
                '\n' => lines.push(Vec::new()),
                _ => {
                    let (operand_at, operand) = chars
                        .next()
                        .filter(|(at, _)| *at < source.len())
                        .context("missing menu control operand")?;
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
