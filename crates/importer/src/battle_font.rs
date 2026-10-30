//! Battle HUD lettering from its native palette page and REL coordinates.
use crate::{battle_ui_hud, rel::Rel};
use anyhow::Result;
use resonance_content::font::{BitmapFont, Glyph};
use std::{collections::BTreeMap, fs, path::Path};

pub fn cook_battle_font(extracted: &Path, library: &Path) -> Result<()> {
    let disc = extracted.join("disc1");
    let image = battle_ui_hud::hud_page(&disc, 0)?;
    let module = Rel::read(fs::read(disc.join("files/US_r_Top2Btl.rel"))?)?;
    let punctuation = module.slice(4, 0x26e0, 15 * 4)?;
    let glyphs: BTreeMap<_, _> = (b'!'..=b'Z')
        .filter_map(|character| {
            let position = match character {
                b'(' => [328, 136],
                b')' => [336, 136],
                b'!'..=b'/' => {
                    let at = usize::from(character - b'!') * 4;
                    [
                        u32::from(u16::from_be_bytes([punctuation[at], punctuation[at + 1]])),
                        u32::from(u16::from_be_bytes([
                            punctuation[at + 2],
                            punctuation[at + 3],
                        ])),
                    ]
                }
                b'0'..=b'9' => [u32::from(character - b'0') * 16, 0],
                b'A'..=b'F' => [160 + u32::from(character - b'A') * 16, 0],
                b'G'..=b'V' => [u32::from(character - b'G') * 16, 24],
                b'W'..=b'Z' => [u32::from(character - b'W') * 16, 48],
                _ => return None,
            };
            Some((
                char::from(character),
                Glyph {
                    rect: [
                        position[0],
                        position[1],
                        if matches!(character, b'(' | b')') {
                            8
                        } else {
                            16
                        },
                        23,
                    ],
                    advance: 13,
                },
            ))
        })
        .collect();
    let font = BitmapFont {
        texture: "fonts/battle.ktx2".into(),
        width: 512,
        height: 512,
        line_height: 24,
        glyphs,
    };
    font.validate()?;
    for (relative, bytes) in [
        (
            "fonts/battle.json",
            resonance_asset_writer::json::encode(&font)?,
        ),
        ("fonts/battle.ktx2", image),
    ] {
        let path = library.join(relative);
        crate::output::write(&path, &bytes)?;
    }
    Ok(())
}
