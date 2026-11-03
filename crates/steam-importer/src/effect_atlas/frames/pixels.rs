use super::*;
use crate::effect_atlas::Table;
use resonance_asset_writer::texture::Pixels;

impl Plan {
    pub(crate) fn image(
        &self,
        source: &Pixels,
        table: Option<&Table>,
        palette: u16,
    ) -> Result<Pixels> {
        ensure!(
            source.width > 0
                && source.height > 0
                && source.rgba.len() == source.width as usize * source.height as usize * 4,
            "invalid source sprite pixels"
        );
        let [width, height] = self.size;
        let mut rgba = vec![0; width as usize * height as usize * 4];
        // A panel's own texels take precedence over its neighbour's filtering
        // margin. Native atlas remaps can place adjacent panels far apart;
        // those two margins cannot both occupy the shared seam in one image.
        let mut occupied = vec![0u8; width as usize * height as usize];
        for &rect in &self.rectangles {
            let target = self.rectangle(rect)?;
            let shift = [
                i32::from(target[0]) - i32::from(rect[0]),
                i32::from(target[1]) - i32::from(rect[1]),
            ];
            for [x, y, w, h] in self.quads(rect)? {
                let mut quad = [[
                    [x as f32, y as f32],
                    [(x + w) as f32, y as f32],
                    [x as f32, (y + h) as f32],
                    [(x + w) as f32, (y + h) as f32],
                ]];
                if let Some(table) = table {
                    table.remap(palette, &mut quad);
                }
                let mapped = quad[0];
                ensure!(
                    mapped.iter().flatten().all(|v| v.is_finite())
                        && (w != 0 || mapped[3][0] == mapped[0][0])
                        && (h != 0 || mapped[3][1] == mapped[0][1]),
                    "sprite mapping expands a zero-size UV axis"
                );
                let scale = [
                    if w == 0 {
                        1.
                    } else {
                        (mapped[3][0] - mapped[0][0]) / w as f32
                    },
                    if h == 0 {
                        1.
                    } else {
                        (mapped[3][1] - mapped[0][1]) / h as f32
                    },
                ];
                for py in y.min(y + h) - 1..=y.max(y + h) {
                    for px in x.min(x + w) - 1..=x.max(x + w) {
                        let tx = px + shift[0];
                        let ty = py + shift[1];
                        let interior = (x.min(x + w)..x.max(x + w)).contains(&px)
                            && (y.min(y + h)..y.max(y + h)).contains(&py);
                        if !interior
                            && (!(0..width as i32).contains(&tx)
                                || !(0..height as i32).contains(&ty))
                        {
                            continue;
                        }
                        ensure!(
                            (0..width as i32).contains(&tx) && (0..height as i32).contains(&ty),
                            "sprite patch exceeds its frame cell"
                        );
                        // Restore GC pixel coordinates even when Steam packed
                        // the same cell at another resolution. Sample texel
                        // centers with the source's ordinary linear filtering.
                        let sx = mapped[0][0] + ((px - x) as f32 + 0.5) * scale[0] - 0.5;
                        let sy = mapped[0][1] + ((py - y) as f32 + 0.5) * scale[1] - 0.5;
                        let color = sample(source, sx, sy);
                        let index = ty as usize * width as usize + tx as usize;
                        let output = &mut rgba[index * 4..][..4];
                        let priority = if interior { 2 } else { 1 };
                        ensure!(
                            occupied[index] != 2 || priority != 2 || output == color,
                            "sprite palette {palette} has incompatible panel interiors at ({px},{py})"
                        );
                        if priority > occupied[index] {
                            output.copy_from_slice(&color);
                            occupied[index] = priority;
                        }
                    }
                }
            }
        }
        Ok(Pixels {
            width,
            height,
            rgba,
        })
    }
}

fn sample(source: &Pixels, x: f32, y: f32) -> [u8; 4] {
    let x = x.clamp(0., (source.width - 1) as f32);
    let y = y.clamp(0., (source.height - 1) as f32);
    let (ix, iy) = (x.floor() as usize, y.floor() as usize);
    let (dx, dy) = (x - ix as f32, y - iy as f32);
    let at = |x: usize, y: usize, channel| {
        source.rgba[(y * source.width as usize + x) * 4 + channel] as f32
    };
    let nx = (ix + 1).min(source.width as usize - 1);
    let ny = (iy + 1).min(source.height as usize - 1);
    std::array::from_fn(|channel| {
        let upper = at(ix, iy, channel) * (1. - dx) + at(nx, iy, channel) * dx;
        let lower = at(ix, ny, channel) * (1. - dx) + at(nx, ny, channel) * dx;
        (upper * (1. - dy) + lower * dy).round() as u8
    })
}
