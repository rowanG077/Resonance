use super::*;
use resonance_content::menu_data::{
    SynopsisData, SynopsisEntry, SynopsisLine, SynopsisLocation, SynopsisSpan,
};
use std::collections::BTreeMap;

pub(super) fn cook(
    font: &crate::font_directory::Metrics,
    ui: &synopsis_catalogue::Catalogue,
    map: &crate::all_assets::world_map::Catalogue,
) -> Result<SynopsisData> {
    let mut metrics = BTreeMap::new();
    let mut wrap = |text: &str| -> Result<Vec<SynopsisLine>> {
        let chars: Vec<_> = text.chars().collect();
        for &ch in &chars {
            if !ch.is_control() && !metrics.contains_key(&ch) {
                metrics.insert(ch, font.advance(font.code(ch)?));
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
    let entries = ui
        .entries
        .iter()
        .map(|row| {
            let id = row.location;
            let location = if id < 0x152 {
                let world = u8::from(id >= 0x100);
                let id = id & 255;
                let point = if id == 0 {
                    None
                } else {
                    let row = map
                        .world(usize::from(world))?
                        .get(usize::from(id))
                        .context("synopsis location exceeds world map")?;
                    Some(row.position.map(|value| (value / 300) as i16))
                };
                Some(SynopsisLocation { world, point })
            } else {
                None
            };
            Ok(SynopsisEntry {
                heading: ui.required(row.heading)?.to_owned(),
                title: ui.required(row.title)?.to_owned(),
                location,
                text: row
                    .text
                    .iter()
                    .map(|reference| {
                        reference
                            .map(|reference| wrap(ui.text(reference)))
                            .transpose()
                    })
                    .collect::<Result<Vec<_>>>()?
                    .try_into()
                    .unwrap(),
            })
        })
        .collect::<Result<_>>()?;
    Ok(SynopsisData {
        entries,
        months: ui
            .months
            .iter()
            .map(|&reference| Ok(ui.required(reference)?.to_owned()))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
    })
}
