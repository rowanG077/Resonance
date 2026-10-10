//! Fixed-cell feedback and proportional labels fitted to their available space.
use super::*;

pub(super) fn glyphs(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    [mut x, y]: [f32; 2],
    [width, height]: [f32; 2],
    color: [u8; 4],
) -> Result<()> {
    for character in text.chars() {
        if character != ' ' {
            let glyph = font.glyphs.get(&character).context("missing HUD glyph")?;
            let [u, v, w, h] = glyph.rect.map(|v| v as f32);
            let rect = [x, y, x + width, y + height];
            let uv = [u, v, u + w, v + h];
            batch.quad(rect, uv, hud_color(color));
        }
        x += width;
    }
    Ok(())
}

/// Preserve glyph proportions and shrink the whole line to fit its rectangle.
pub(super) fn fit(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    [x, y, width, height]: [f32; 4],
    color: [u8; 4],
) -> Result<()> {
    let glyphs = text
        .chars()
        .map(|ch| font.glyphs.get(&ch).context("missing HUD glyph"))
        .collect::<Result<Vec<_>>>()?;
    let mut bounds = [0_f32, font.line_height as f32];
    let mut advance = 0.;
    for glyph in &glyphs {
        bounds[0] = bounds[0].max(advance + glyph.rect[2] as f32);
        bounds[1] = bounds[1].max(glyph.rect[3] as f32);
        advance += glyph.advance as f32;
    }
    let scale = (width / bounds[0].max(1.)).min(height / bounds[1]);
    advance = 0.;
    for glyph in glyphs {
        let [u, v, w, h] = glyph.rect.map(|v| v as f32);
        batch.quad(
            [
                x + advance * scale,
                y,
                x + (advance + w) * scale,
                y + h * scale,
            ],
            [u, v, u + w, v + h],
            hud_color(color),
        );
        advance += glyph.advance as f32;
    }
    Ok(())
}
