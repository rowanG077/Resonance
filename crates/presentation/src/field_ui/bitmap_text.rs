//! Shared bitmap text layout for menus and host overlays.
use anyhow::{Context, Result};
use resonance_content::font::BitmapFont;

pub(super) const LINE_SPACING: f32 = 2.;

#[derive(Clone, Copy)]
pub(super) enum Spacing {
    /// Fixed half-cell spaces used by the menu artwork.
    Menu,
    /// Authored advances for every glyph, including spaces.
    Font,
}

fn advance(font: &BitmapFont, character: char, width: f32, spacing: Spacing) -> Result<f32> {
    if character == ' ' && matches!(spacing, Spacing::Menu) {
        return Ok((width / 2.).trunc());
    }
    let glyph = font
        .glyphs
        .get(&character)
        .with_context(|| format!("uncooked menu glyph {character:?}"))?;
    // Full-width characters occupy one cell; Latin and half-width kana use
    // the font's proportional advances.
    Ok(if resonance_content::font::is_single_byte(character) {
        (glyph.advance as f32 * width / 24.).trunc()
    } else {
        width
    })
}

pub(super) fn width(font: &BitmapFont, text: &str, width: f32, spacing: Spacing) -> Result<f32> {
    text.chars().map(|c| advance(font, c, width, spacing)).sum()
}

pub(super) fn draw(
    font: &BitmapFont,
    text: &str,
    [left, mut y]: [f32; 2],
    [width, height]: [f32; 2],
    spacing: Spacing,
    mut quad: impl FnMut([f32; 4], [f32; 4]),
) -> Result<()> {
    let mut x = left;
    for character in text.chars() {
        if character == '\n' {
            x = left;
            y += height + LINE_SPACING;
            continue;
        }
        let advance = advance(font, character, width, spacing)?;
        if character != ' ' || matches!(spacing, Spacing::Font) {
            let [u, v, w, h] = font.glyphs[&character].rect.map(|v| v as f32);
            quad([x, y, x + width, y + height], [u, v, u + w, v + h]);
        }
        x += advance;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::font::Glyph;

    #[test]
    fn measurement_and_drawing_share_proportional_and_full_width_spacing() -> Result<()> {
        let font = BitmapFont {
            texture: String::new(),
            width: 24,
            height: 24,
            line_height: 24,
            glyphs: [('A', 11), ('界', 3), ('ｱ', 7), (' ', 17)]
                .into_iter()
                .map(|(character, advance)| {
                    (
                        character,
                        Glyph {
                            rect: [0, 0, 24, 24],
                            advance,
                        },
                    )
                })
                .collect(),
        };
        assert_eq!(width(&font, "A 界ｱ", 18., Spacing::Menu)?, 40.);
        assert_eq!(width(&font, "A 界ｱ", 18., Spacing::Font)?, 43.);
        let mut quads = Vec::new();
        draw(
            &font,
            "A 界ｱ\nA",
            [10., 20.],
            [18., 24.],
            Spacing::Menu,
            |rect, _| quads.push(rect),
        )?;
        assert_eq!(
            quads,
            [
                [10., 20., 28., 44.],
                [27., 20., 45., 44.],
                [45., 20., 63., 44.],
                [10., 46., 28., 70.],
            ]
        );
        quads.clear();
        draw(
            &font,
            "A 界ｱ",
            [10., 20.],
            [18., 24.],
            Spacing::Font,
            |rect, _| quads.push(rect),
        )?;
        assert_eq!(
            quads,
            [
                [10., 20., 28., 44.],
                [18., 20., 36., 44.],
                [30., 20., 48., 44.],
                [48., 20., 66., 44.],
            ]
        );
        Ok(())
    }
}
