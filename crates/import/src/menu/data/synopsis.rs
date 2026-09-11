use super::*;
use resonance_content::menu_data::{
    SYNOPSIS_COUNT, SynopsisData, SynopsisEntry, SynopsisLine, SynopsisLocation, SynopsisSpan,
};
use std::collections::BTreeMap;

pub(super) fn cook(
    executable: &[u8],
    text: &impl Fn(&[u8], usize) -> Result<String>,
) -> Result<SynopsisData> {
    let mut metrics = BTreeMap::new();
    let mut wrap = |text: &str| -> Result<Vec<SynopsisLine>> {
        let chars: Vec<_> = text.chars().collect();
        for &ch in &chars {
            if !ch.is_control() && !metrics.contains_key(&ch) {
                let code = if ch.is_ascii() {
                    if ch == '^' {
                        0x81a7
                    } else {
                        let address = 0x801f8984 + (ch as u32 - 32) * 2;
                        u16::from_be_bytes(dol::slice(executable, address, 2)?.try_into()?)
                    }
                } else {
                    let character = ch.to_string();
                    let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(&character);
                    ensure!(
                        !invalid && bytes.len() == 2,
                        "invalid synopsis character {ch:?}"
                    );
                    u16::from_be_bytes(bytes.as_ref().try_into()?)
                };
                metrics.insert(ch, crate::font::glyph_advance(executable, code)?);
            }
        }
        let mut lines = Vec::new();
        let mut at = 0;
        while at < chars.len() {
            let start = at;
            let mut x = 24;
            let mut color = 9;
            let mut spans: Vec<SynopsisSpan> = Vec::new();
            while let Some(&ch) = chars.get(at) {
                if ch.is_control() {
                    at += 1;
                    if ch == '\n' {
                        break;
                    }
                    if ch == '\u{c}' {
                        color = *chars.get(at).context("truncated synopsis color")? as u32;
                        ensure!(color <= 10, "invalid synopsis color {color}");
                        at += 1;
                    }
                    continue;
                }
                let overflow = if ch.is_ascii() {
                    x + chars[at..]
                        .iter()
                        .take_while(|c| !c.is_whitespace() && !c.is_control())
                        .map(|c| metrics[c])
                        .sum::<u32>()
                        > 604
                        && !matches!(ch, '.' | ',')
                } else {
                    x + 24 > 604 && !matches!(ch, '、' | '。')
                };
                if overflow {
                    break;
                }
                if spans
                    .last()
                    .is_none_or(|span| u32::from(span.color) != color)
                {
                    spans.push(SynopsisSpan {
                        text: String::new(),
                        color: color as u8,
                    });
                }
                spans.last_mut().unwrap().text.push(ch);
                x += if ch == ' ' { 12 } else { metrics[&ch] };
                at += 1;
            }
            ensure!(at > start, "synopsis word exceeds the reading panel");
            lines.push(SynopsisLine(spans));
        }
        if lines.is_empty() {
            lines.push(SynopsisLine(Vec::new()));
        }
        Ok(lines)
    };
    let entries = dol::slice(executable, 0x802a1c30, SYNOPSIS_COUNT * 24)?
        .chunks_exact(24)
        .map(|row| {
            let id = u16::from_be_bytes(row[2..4].try_into()?);
            let location = if id < 0x152 {
                let world = u8::from(id >= 0x100);
                let id = id & 255;
                let base = [0x8026ae80, 0x8026b650][usize::from(world)];
                let point = if id == 0 {
                    None
                } else {
                    let point = dol::slice(executable, base + u32::from(id) * 20, 8)?;
                    Some(
                        std::array::from_fn(|axis| {
                            i32::from_be_bytes(point[axis * 4..axis * 4 + 4].try_into().unwrap())
                                / 300
                        })
                        .map(|v| v as i16),
                    )
                };
                Some(SynopsisLocation { world, point })
            } else {
                None
            };
            Ok(SynopsisEntry {
                heading: text(row, 4)?,
                title: text(row, 8)?,
                location,
                text: (0..3)
                    .map(|i| {
                        if row[12 + i * 4..16 + i * 4] == [0; 4] {
                            Ok(None)
                        } else {
                            wrap(&text(row, 12 + i * 4)?).map(Some)
                        }
                    })
                    .collect::<Result<Vec<_>>>()?
                    .try_into()
                    .unwrap(),
            })
        })
        .collect::<Result<_>>()?;
    Ok(SynopsisData {
        entries,
        months: (0..12)
            .map(|i| text(&(0x8035d974u32 + i * 4).to_be_bytes(), 0))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
    })
}
