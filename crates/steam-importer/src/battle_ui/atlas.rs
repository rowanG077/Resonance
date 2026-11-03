//! Restore the HUD's original palette-role coordinates from packed RGBA art.
use super::*;
use crate::effect_atlas::Table;
use resonance_content::{battle_ui::CommandArt, font::UiTexture};
use sha2::{Digest, Sha256};

pub(super) struct Atlas<'a> {
    pixels: RgbaImage,
    table: Option<&'a Table>,
    published: BTreeMap<u16, UiTexture>,
    hashes: BTreeMap<[u8; 32], UiTexture>,
}

impl<'a> Atlas<'a> {
    pub(super) fn new(pixels: RgbaImage, table: Option<&'a Table>) -> Result<Self> {
        ensure!(
            pixels.dimensions() == (2048, 1024),
            "unsupported native HUD atlas dimensions"
        );
        Ok(Self {
            pixels: imageops::resize(&pixels, 1024, 512, imageops::FilterType::Triangle),
            table,
            published: BTreeMap::new(),
            hashes: BTreeMap::new(),
        })
    }

    pub(super) fn page(&self, palette: u16) -> Result<RgbaImage> {
        let mut image = imageops::crop_imm(&self.pixels, 0, 0, 512, 512).to_image();
        let mut occupied = vec![0; 512 * 512];
        if let Some(table) = self.table {
            for row in &table.rows {
                if row.palette != palette {
                    continue;
                }
                let [x1, y1, x2, y2] = row.source;
                if x1 == x2 || y1 == y2 {
                    continue;
                }
                let mut quad = [[[x1, y1], [x2, y1], [x1, y2], [x2, y2]]];
                table.remap(palette, &mut quad);
                let packed = quad[0];
                ensure!(
                    row.source
                        .iter()
                        .chain(packed.iter().flatten())
                        .all(|n| n.is_finite() && n.fract() == 0.)
                        && x1 >= 0.
                        && y1 >= 0.
                        && x2 <= 512.
                        && y2 <= 512.
                        && x2 >= x1
                        && y2 >= y1
                        && packed[3][0] - packed[0][0] == x2 - x1
                        && packed[3][1] - packed[0][1] == y2 - y1,
                    "HUD palette {palette} changes texel scale or source bounds"
                );
                let [x1, y1, x2, y2] = [x1, y1, x2, y2].map(|n| n as i32);
                for y in y1 - 1..=y2 {
                    for x in x1 - 1..=x2 {
                        if !(0..512).contains(&x) || !(0..512).contains(&y) {
                            continue;
                        }
                        let sx = (packed[0][0] as i32 + x - x1)
                            .clamp(0, self.pixels.width() as i32 - 1)
                            as u32;
                        let sy = (packed[0][1] as i32 + y - y1)
                            .clamp(0, self.pixels.height() as i32 - 1)
                            as u32;
                        let color = self.pixels.get_pixel(sx, sy);
                        let interior = x >= x1 && x < x2 && y >= y1 && y < y2;
                        let priority = if interior { 2 } else { 1 };
                        let index = y as usize * 512 + x as usize;
                        ensure!(
                            occupied[index] != 2
                                || priority != 2
                                || image.get_pixel(x as u32, y as u32) == color,
                            "HUD palette {palette} has conflicting source cells"
                        );
                        if priority > occupied[index] {
                            image.put_pixel(x as u32, y as u32, *color);
                            occupied[index] = priority;
                        }
                    }
                }
            }
        }
        Ok(image)
    }

    pub(super) fn publish(&mut self, library: &Path, palette: u16) -> Result<UiTexture> {
        if let Some(image) = self.published.get(&palette) {
            return Ok(image.clone());
        }
        let pixels = self.page(palette)?;
        let hash: [u8; 32] = Sha256::digest(pixels.as_raw()).into();
        let image = if let Some(image) = self.hashes.get(&hash) {
            image.clone()
        } else {
            let path = format!("ui/battle/pages/palette-{palette:02}.ktx2");
            let image = ui_images::write(library, &path, &pixels)?;
            self.hashes.insert(hash, image.clone());
            image
        };
        self.published.insert(palette, image.clone());
        Ok(image)
    }

    pub(super) fn commands(&self, library: &Path, art: &mut CommandArt) -> Result<()> {
        let mut pixels = RgbaImage::new(512, 1024);
        let mut tile = 0;
        let mut copy = |palette, rect: &mut [u16; 4]| -> Result<()> {
            let source = self.page(palette)?;
            let [u, v, width, height] = rect.map(u32::from);
            ensure!(
                u + width <= 512 && v + height <= 512 && width <= 124 && height <= 60,
                "invalid command atlas cell"
            );
            let x = tile % 4 * 128 + 2;
            let y = tile / 4 * 64 + 2;
            imageops::replace(
                &mut pixels,
                &imageops::crop_imm(&source, u, v, width, height).to_image(),
                i64::from(x),
                i64::from(y),
            );
            *rect = [x as u16, y as u16, width as u16, height as u16];
            tile += 1;
            Ok(())
        };
        for (icons, palettes) in art.icons.iter_mut().zip([
            &[3, 4, 5][..],
            &[75, 76, 77, 77, 77],
            &[6, 7, 8, 9],
            &[10, 11],
            &[12, 13, 14],
            &[15],
        ]) {
            for (rect, &palette) in icons.iter_mut().zip(palettes) {
                copy(palette, rect)?;
            }
        }
        for (palette, rect) in [
            (2, &mut art.background),
            (86, &mut art.shadow),
            (47, &mut art.disabled),
            (58, &mut art.plate),
            (21, &mut art.cursor),
            (0, &mut art.cursor_shadow),
        ] {
            copy(palette, rect)?;
        }
        for (index, rect) in art.faces.iter_mut().enumerate() {
            copy(32 + index as u16, rect)?;
        }
        art.texture = ui_images::write(library, "ui/battle/commands/atlas.ktx2", &pixels)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect_atlas::Remap;
    #[test]
    fn palette_cells_restore_their_original_coordinates_without_recoloring_other_roles()
    -> Result<()> {
        let mut pixels = RgbaImage::from_pixel(2048, 1024, image::Rgba([5, 7, 11, 255]));
        for y in 20..36 {
            for x in 1200..1216 {
                pixels.put_pixel(x, y, image::Rgba([40, 80, 120, 255]));
            }
        }
        let table = Table {
            rows: vec![Remap {
                palette: 17,
                source: [10., 10., 18., 18.],
                packed: [600., 10., 608., 18.],
            }],
        };
        let atlas = Atlas::new(pixels, Some(&table))?;
        assert_eq!(atlas.page(17)?.get_pixel(13, 13).0, [40, 80, 120, 255]);
        assert_eq!(atlas.page(16)?.get_pixel(13, 13).0, [5, 7, 11, 255]);
        assert_eq!(atlas.page(17)?.get_pixel(5, 5).0, [5, 7, 11, 255]);
        Ok(())
    }
}
