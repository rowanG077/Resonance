//! Fixed GC atlas cells used by equipment trails and wing sparkles, outside programs.
use super::*;
use resonance_content::battle_effect::EffectTexture;

pub(super) fn append(
    source: &Source,
    library: &Path,
    resource: &str,
    table: Option<&effect_atlas::Table>,
    owner: &str,
    texture: &mut EffectTexture,
) -> Result<()> {
    let pixels = crate::ui_images::read(source, resource)?;
    ensure!(
        pixels.width() >= 512 && pixels.height() >= 512,
        "resident fixed atlas is smaller than GC UV space"
    );
    trails(library, &pixels, table, owner, texture)?;
    for selector in 27..=31u8 {
        let mut quad = [[[193., 385.], [255., 385.], [193., 447.], [255., 447.]]];
        if let Some(table) = table {
            table.remap(u16::from(selector), &mut quad);
        }
        let mapped = quad[0];
        ensure!(
            mapped
                .iter()
                .flatten()
                .all(|value| value.is_finite() && value.fract() == 0.)
                && mapped[3][0] - mapped[0][0] == 62.
                && mapped[3][1] - mapped[0][1] == 62.,
            "wing sparkle remapping changes texel scale"
        );
        let mut image = image::RgbaImage::new(512, 512);
        for y in -1..=62i32 {
            for x in -1..=62i32 {
                let sx = (mapped[0][0] as i32 + x).clamp(0, pixels.width() as i32 - 1) as u32;
                let sy = (mapped[0][1] as i32 + y).clamp(0, pixels.height() as i32 - 1) as u32;
                image.put_pixel(
                    (193 + x) as u32,
                    (385 + y) as u32,
                    *pixels.get_pixel(sx, sy),
                );
            }
        }
        let page = texture.images.len();
        texture.images.push(crate::ui_images::write(
            library,
            &format!("{owner}/textures/wing-sparkle-{selector}.ktx2"),
            &image,
        )?);
        texture.palette_sets[0][usize::from(selector)] = Some(page);
    }
    Ok(())
}

fn trails(
    library: &Path,
    pixels: &image::RgbaImage,
    table: Option<&effect_atlas::Table>,
    owner: &str,
    texture: &mut EffectTexture,
) -> Result<()> {
    let normal = image::imageops::crop_imm(pixels, 0, 0, 512, 512).to_image();
    let highlight = highlighted(
        pixels,
        table.context("missing native highlighted trail remaps")?,
    )?;
    for (selector, name, pixels) in [
        (0, "weapon-trails", normal),
        (1, "weapon-trails-highlight", highlight),
    ] {
        let path = format!("{owner}/textures/{name}.ktx2");
        let image = crate::ui_images::write(library, &path, &pixels)?;
        let page = if let Some(page) = texture.images.iter().position(|old| old.path == path) {
            texture.images[page] = image;
            page
        } else {
            let page = texture.images.len();
            texture.images.push(image);
            page
        };
        texture
            .palette_sets
            .first_mut()
            .context("missing fixed atlas palette set")?[selector] = Some(page);
    }
    Ok(())
}

fn highlighted(pixels: &image::RgbaImage, table: &effect_atlas::Table) -> Result<image::RgbaImage> {
    let mut image = image::imageops::crop_imm(pixels, 0, 0, 512, 512).to_image();
    // Native blade history remaps four-pixel strips of these fixed64² cells.
    // All strips of a cell have one translation. Restore its full GC rectangle,
    // including the final unused native strip and the filtering margin.
    for [x, y, width, height] in [[448, 0, 64, 64], [448, 128, 64, 64]] {
        let mut offset = None;
        for row in table.rows.iter().filter(|row| row.palette == 1) {
            let [x1, y1, x2, y2] = row.source;
            if x1 == x2
                || y1 == y2
                || x1.min(x2) < x as f32
                || x1.max(x2) > (x + width) as f32
                || y1.min(y2) < y as f32
                || y1.max(y2) > (y + height) as f32
            {
                continue;
            }
            let mut quad = [[[x1, y1], [x2, y1], [x1, y2], [x2, y2]]];
            table.remap(1, &mut quad);
            let mapped = quad[0];
            let shift = [mapped[0][0] - x1, mapped[0][1] - y1];
            ensure!(
                shift.iter().all(|n| n.is_finite() && n.fract() == 0.)
                    && mapped[3][0] - x2 == shift[0]
                    && mapped[3][1] - y2 == shift[1],
                "highlighted trail remap changes texel scale"
            );
            let shift = shift.map(|value| value as i32);
            ensure!(
                offset.is_none_or(|previous| previous == shift),
                "highlighted trail strips disagree"
            );
            offset = Some(shift);
        }
        let [dx, dy] = offset.context("missing highlighted blade trail cell")?;
        for ty in y - 1..=y + height {
            for tx in x - 1..=x + width {
                if !(0..512).contains(&tx) || !(0..512).contains(&ty) {
                    continue;
                }
                let (sx, sy) = (tx + dx, ty + dy);
                ensure!(
                    sx >= 0 && sy >= 0 && sx < pixels.width() as i32 && sy < pixels.height() as i32,
                    "highlighted trail cell exceeds packed atlas"
                );
                image.put_pixel(
                    tx as u32,
                    ty as u32,
                    *pixels.get_pixel(sx as u32, sy as u32),
                );
            }
        }
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlighted_fixed_cells_restore_native_palette_without_tinting_the_page() -> Result<()> {
        let mut pixels = image::RgbaImage::from_pixel(1024, 1024, image::Rgba([90, 90, 90, 255]));
        for y in 448..512 {
            for x in 896..1024 {
                pixels.put_pixel(x, y, image::Rgba([30, 80, 180, 200]));
            }
        }
        let table = effect_atlas::Table {
            rows: vec![
                effect_atlas::Remap {
                    palette: 1,
                    source: [448., 64., 452., 0.],
                    packed: [896., 512., 900., 448.],
                },
                effect_atlas::Remap {
                    palette: 1,
                    source: [448., 192., 452., 128.],
                    packed: [960., 512., 964., 448.],
                },
            ],
        };
        let image = highlighted(&pixels, &table)?;
        ensure!(
            image.get_pixel(470, 32).0 == [30, 80, 180, 200]
                && image.get_pixel(500, 160).0 == [30, 80, 180, 200],
            "missing highlighted cells"
        );
        ensure!(
            image.get_pixel(50, 50).0 == [90, 90, 90, 255],
            "unrelated atlas region changed"
        );
        Ok(())
    }
}
