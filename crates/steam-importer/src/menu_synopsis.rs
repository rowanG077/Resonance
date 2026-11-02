//! Authored journal entries, using the shared GameCube reading-panel layout.
use crate::{
    output, pe,
    source::Source,
    text::{Bank, Menu},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    font::BitmapFont,
    menu_data::{SYNOPSIS_COUNT, SYNOPSIS_PATH, SynopsisData, SynopsisEntry, SynopsisLocation},
};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn cook(source: &Source, output: &Path, font: &BitmapFont) -> Result<()> {
    let bytes = source.require("TOS_TEXT.TXB")?;
    let source = Menu {
        image: pe::Image::parse(&source.executable)?,
        bank: Bank::parse(&bytes)?,
        slot: source.language.slot(),
    };
    output::json(&output.join(SYNOPSIS_PATH), &read(&source, font)?)
}

fn read(source: &Menu<'_>, font: &BitmapFont) -> Result<SynopsisData> {
    // 0x538e6e reads the shared-width rows; 0x53995a resolves their text indices
    // through the selected language directory. Zero alternate indices are absent.
    let text = source.directory(0x8b5cdc)?;
    let entries = source
        .image
        .slice(0x8b5d00, SYNOPSIS_COUNT * 24)?
        .chunks_exact(24)
        .map(|row| -> Result<_> {
            ensure!(
                pe::half(row, 0)? == 0,
                "unsupported Steam synopsis record flags"
            );
            let id = pe::half(row, 2)?;
            let location = if id < 0x152 {
                let world = u8::from(id >= 0x100);
                let local = u32::from(id & 255);
                let point = if local == 0 {
                    None
                } else {
                    let address = if world == 0 { 0x8b7700 } else { 0x8b7ed0 } + local * 20;
                    let point = [
                        source.word(address)? as i32,
                        source.word(address + 4)? as i32,
                    ];
                    ensure!(!point.contains(&-1), "missing synopsis map location");
                    // The common synopsis map is 256 x 192, smaller than the
                    // ordinary world-map menu's 384 x 288 drawing canvas.
                    Some([(point[0] / 300).try_into()?, (point[1] / 300).try_into()?])
                };
                Some(SynopsisLocation { world, point })
            } else {
                None
            };
            let body = |index| -> Result<_> {
                let index = pe::word(row, index)?;
                if index == 0 {
                    return Ok(Vec::new());
                }
                resonance_importer::wrap_synopsis(&source.caption(text, index)?, |ch| {
                    font.glyphs
                        .get(&ch)
                        .map(|glyph| glyph.advance)
                        .with_context(|| format!("missing synopsis glyph {ch:?}"))
                })
            };
            let base = body(12)?;
            let mut updates = BTreeMap::new();
            for (state, offset) in [(2, 16), (3, 20)] {
                if pe::word(row, offset)? != 0 {
                    let lines = body(offset)?;
                    if lines != base {
                        updates.insert(state, lines);
                    }
                }
            }
            Ok(SynopsisEntry {
                heading: source.caption(text, pe::word(row, 4)?)?,
                title: source.caption(text, pe::word(row, 8)?)?,
                location,
                text: base,
                updates,
            })
        })
        .collect::<Result<_>>()?;
    let months = source.directory(0x8b5cac)?;
    let data = SynopsisData {
        entries,
        months: (0..12)
            .map(|index| source.caption(months, index))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
    };
    data.validate()?;
    Ok(data)
}
