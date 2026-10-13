//! Fixed-cell feedback and proportional labels fitted to their available space.
use super::*;

/// Stretch atlas glyphs with separate spacing and a vertical color gradient.
pub(super) fn italic(
    batch: &mut Batch,
    font: &BitmapFont,
    label: &str,
    [x, y]: [f32; 2],
    [width, height]: [f32; 2],
    advance: f32,
    skew: f32,
    colors: [[u8; 4]; 2],
) -> Result<()> {
    for (column, character) in label.chars().enumerate().filter(|(_, ch)| *ch != ' ') {
        let glyph = font
            .glyphs
            .get(&character)
            .context("missing italic HUD glyph")?;
        let [u, v, w, h] = glyph.rect.map(|value| value as f32);
        let left = x + column as f32 * advance;
        party::slanted_quad(
            batch,
            [left, y, left + width, y + height],
            [u, v, u + w, v + h],
            skew,
            [colors[0], colors[0], colors[1], colors[1]],
        );
    }
    Ok(())
}

pub(super) fn glyphs(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    position: [f32; 2],
    size: [f32; 2],
    color: [u8; 4],
) -> Result<()> {
    glyph_gradient(batch, font, text, position, size, [color; 2])
}

pub(super) fn glyph_gradient(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    [mut x, y]: [f32; 2],
    [width, height]: [f32; 2],
    [top, bottom]: [[u8; 4]; 2],
) -> Result<()> {
    for character in text.chars() {
        if character != ' ' {
            let glyph = font.glyphs.get(&character).context("missing HUD glyph")?;
            let [u, v, w, h] = glyph.rect.map(|v| v as f32);
            let scale = (width / w).min(height / h);
            let rect = [x, y, x + w * scale, y + h * scale];
            let uv = [u, v, u + w, v + h];
            party::slanted_quad(batch, rect, uv, 0., [top, top, bottom, bottom]);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_cells_preserve_the_bitmap_glyph_aspect() {
        let font = BitmapFont {
            version: 1,
            texture: String::new(),
            width: 32,
            height: 32,
            line_height: 20,
            glyphs: [(
                '1',
                resonance_content::font::Glyph {
                    rect: [0, 0, 10, 20],
                    advance: 10,
                },
            )]
            .into(),
            source_sha256: String::new(),
            executable_sha256: String::new(),
        };
        let mut batch = Batch::default();
        glyphs(&mut batch, &font, "11", [0., 0.], [10., 10.], [128; 4]).unwrap();
        assert_eq!(batch.positions[1][0] - batch.positions[0][0], 5.);
        assert_eq!(batch.positions[0][1] - batch.positions[2][1], 10.);
        assert_eq!(batch.positions[4][0] - batch.positions[0][0], 10.);
    }
}
