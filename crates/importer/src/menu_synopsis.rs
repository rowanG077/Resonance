//! Native journal entries directly into the editable synopsis document.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::menu_data::{
    SYNOPSIS_COUNT, SYNOPSIS_PATH, SynopsisData, SynopsisEntry, SynopsisLine, SynopsisLocation,
    SynopsisSpan,
};
use std::{collections::BTreeMap, fs, path::Path};

const ENTRIES: u32 = 0x802a1c30;
const WORLD_TABLES: u32 = 0x8035a1c8;
const ASCII_CODES: u32 = 0x801f8984;
const ALTERNATE_CODES: u32 = 0x801f8900;
const ADVANCES: u32 = 0x801f9680;

struct Font<'a> {
    ascii: &'a [u8],
    alternate: &'a [u8],
    advances: &'a [u8],
}

impl Font<'_> {
    fn width(&self, ch: char) -> Result<u32> {
        let code = if ch == '^' {
            0x81a7
        } else {
            let string = ch.to_string();
            let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(&string);
            ensure!(!invalid, "invalid journal character {ch:?}");
            match *encoded {
                [byte] => {
                    let table = if byte & 0x80 != 0 {
                        self.alternate
                    } else {
                        self.ascii
                    };
                    let index = usize::from((byte & 0x7f).max(32) - 32) * 2;
                    dol::u16(table, index)?
                }
                [high, low] => u16::from_be_bytes([high, low]),
                _ => anyhow::bail!("invalid journal character {ch:?}"),
            }
        };
        if !(0x8140..=0x829a).contains(&code) {
            return Ok(24);
        }
        let index = usize::from((code >> 8) - 0x81) * 192 + usize::from(code & 255) - 64;
        Ok(
            match *self.advances.get(index).context("missing font advance")? {
                0 => 24,
                width => u32::from(width),
            },
        )
    }
}

fn wrap(source: &str, font: &Font<'_>) -> Result<Vec<SynopsisLine>> {
    let chars: Vec<_> = source.chars().collect();
    let mut widths = BTreeMap::new();
    for &ch in &chars {
        if !ch.is_control() {
            widths.entry(ch).or_insert(font.width(ch)?);
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
                    color = u32::from(*chars.get(at).context("missing journal color")?);
                    ensure!(color <= 10, "invalid journal color {color}");
                    at += 1;
                }
                continue;
            }
            let overflows = if ch.is_ascii() {
                x + chars[at..]
                    .iter()
                    .take_while(|c| !c.is_whitespace() && !c.is_control())
                    .map(|c| widths[c])
                    .sum::<u32>()
                    > 604
                    && !matches!(ch, '.' | ',')
            } else {
                x + 24 > 604 && !matches!(ch, '、' | '。')
            };
            if overflows {
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
            x += if ch == ' ' { 12 } else { widths[&ch] };
            at += 1;
        }
        ensure!(at > start, "journal word exceeds the reading panel");
        lines.push(SynopsisLine(spans));
    }
    if lines.is_empty() {
        lines.push(SynopsisLine(Vec::new()));
    }
    Ok(lines)
}

fn location(exe: &[u8], id: u16) -> Result<Option<SynopsisLocation>> {
    if id >= 0x152 {
        return Ok(None);
    }
    let world = u8::from(id >= 0x100);
    let point = if id & 255 == 0 {
        None
    } else {
        let table = dol::u32(dol::slice(exe, WORLD_TABLES + u32::from(world) * 4, 4)?, 0)?;
        let row = dol::slice(exe, table + u32::from(id & 255) * 20, 8)?;
        Some([
            (dol::u32(row, 0)? as i32 / 300) as i16,
            (dol::u32(row, 4)? as i32 / 300) as i16,
        ])
    };
    Ok(Some(SynopsisLocation { world, point }))
}

fn convert(exe: &[u8]) -> Result<SynopsisData> {
    let font = Font {
        ascii: dol::slice(exe, ASCII_CODES, 96 * 2)?,
        alternate: dol::slice(exe, ALTERNATE_CODES, 66 * 2)?,
        advances: dol::slice(exe, ADVANCES, 288)?,
    };
    let entries = dol::slice(exe, ENTRIES, SYNOPSIS_COUNT * 24)?
        .chunks_exact(24)
        .map(|row| {
            let initial = dol::u32(row, 12)?;
            let text = if initial == 0 {
                Vec::new()
            } else {
                wrap(&text::string(exe, initial)?, &font)?
            };
            let mut updates = BTreeMap::new();
            for (state, offset) in [(2, 16), (3, 20)] {
                let pointer = dol::u32(row, offset)?;
                if pointer != 0 {
                    let lines = wrap(&text::string(exe, pointer)?, &font)?;
                    if lines != text {
                        updates.insert(state, lines);
                    }
                }
            }
            Ok(SynopsisEntry {
                heading: text::string(exe, dol::u32(row, 4)?)?,
                title: text::string(exe, dol::u32(row, 8)?)?,
                location: location(exe, dol::u16(row, 2)?)?,
                text,
                updates,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let document = SynopsisData {
        entries,
        months: text::table(exe, 0x801df814)?,
    };
    document.validate()?;
    Ok(document)
}

pub fn cook_menu_synopsis(extracted: &Path, library: &Path) -> Result<()> {
    let output = library.join(SYNOPSIS_PATH);
    let document = convert(&fs::read(extracted.join("disc1/sys/main.dol"))?)?;
    crate::output::json(&output, &document)?;
    Ok(())
}
