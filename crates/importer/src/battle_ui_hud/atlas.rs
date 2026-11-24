use crate::{dol, world_texture::rgb5a3};
use anyhow::{Context, Result, ensure};
use resonance_content::{battle_ui::HudImage, font::UiTexture};

pub(super) const PATH: &str = "ui/battle/hud.ktx2";
const SIDE: usize = 512;

pub(super) struct Sheet {
    indices: Vec<u8>,
    colors: Vec<[u8; 4]>,
}

impl Sheet {
    pub(super) fn read(tpl: &[u8]) -> Result<Self> {
        ensure!(
            dol::u32(tpl, 0)? == 0x0020_af30 && dol::u32(tpl, 4)? == 1,
            "unexpected battle HUD TPL"
        );
        let table = dol::u32(tpl, 8)? as usize;
        let descriptor = dol::u32(tpl, table)? as usize;
        let palette = dol::u32(tpl, table + 4)? as usize;
        ensure!(
            dol::u16(tpl, descriptor)? == 512
                && dol::u16(tpl, descriptor + 2)? == 512
                && dol::u32(tpl, descriptor + 4)? == 8,
            "unsupported battle HUD image"
        );
        ensure!(
            dol::u32(tpl, palette + 4)? == 2,
            "unsupported battle HUD palette"
        );
        let entries = usize::from(dol::u16(tpl, palette)?);
        ensure!(entries == 96 * 16, "unexpected battle HUD palette pages");
        let palette_start = dol::u32(tpl, palette + 8)? as usize;
        let colors = (0..entries)
            .map(|i| dol::u16(tpl, palette_start + i * 2).map(rgb5a3))
            .collect::<Result<_>>()?;
        let start = dol::u32(tpl, descriptor + 8)? as usize;
        let blocks = tpl
            .get(start..start + SIDE * SIDE / 2)
            .context("truncated battle HUD image")?;
        let mut indices = vec![0; SIDE * SIDE];
        for (tile, block) in blocks.chunks_exact(32).enumerate() {
            for (row, packed) in block.chunks_exact(4).enumerate() {
                let at = (tile / 64 * 8 + row) * SIDE + tile % 64 * 8;
                for (pair, &byte) in packed.iter().enumerate() {
                    indices[at + pair * 2] = byte >> 4;
                    indices[at + pair * 2 + 1] = byte & 15;
                }
            }
        }
        Ok(Self { indices, colors })
    }

    /// All output sheets are 512 pixels wide. Copy only the requested artwork.
    pub(super) fn copy(
        &self,
        palette: usize,
        [u, v, w, h]: [usize; 4],
        pixels: &mut [u8],
        [x, y]: [usize; 2],
    ) -> Result<()> {
        ensure!(
            u + w <= SIDE && v + h <= SIDE && x + w <= SIDE && (y + h) * SIDE * 4 <= pixels.len(),
            "battle HUD sprite exceeds sheet"
        );
        let colors = self
            .colors
            .get(palette * 16..palette * 16 + 16)
            .context("invalid HUD palette")?;
        for row in 0..h {
            let from = (v + row) * SIDE + u;
            let to = ((y + row) * SIDE + x) * 4;
            for (&index, out) in self.indices[from..from + w]
                .iter()
                .zip(pixels[to..to + w * 4].chunks_exact_mut(4))
            {
                out.copy_from_slice(&colors[usize::from(index)]);
            }
        }
        Ok(())
    }
}

/// Pack each palette's sampled region with a surrounding block for filtering.
/// Matching the source block grid also preserves BC7 encoding at sprite edges.
pub(super) struct Atlas {
    pub(super) pixels: Vec<u8>,
    x: usize,
    y: usize,
    row_height: usize,
}

impl Atlas {
    pub(super) fn new() -> Self {
        Self {
            pixels: vec![0; SIDE * SIDE * 4],
            x: 0,
            y: 0,
            row_height: 0,
        }
    }

    pub(super) fn insert(
        &mut self,
        sheet: &Sheet,
        palette: usize,
        [u, v, w, h]: [usize; 4],
    ) -> Result<HudImage> {
        ensure!(
            w > 0 && h > 0 && u + w <= SIDE && v + h <= SIDE,
            "invalid HUD source region"
        );
        let left = (u / 4 * 4).saturating_sub(4);
        let top = (v / 4 * 4).saturating_sub(4);
        let width = ((u + w).next_multiple_of(4) + 4).min(SIDE) - left;
        let height = ((v + h).next_multiple_of(4) + 4).min(SIDE) - top;
        if self.x + width > SIDE {
            self.x = 0;
            self.y += self.row_height;
            self.row_height = 0;
        }
        sheet.copy(
            palette,
            [left, top, width, height],
            &mut self.pixels,
            [self.x, self.y],
        )?;
        let offset = [self.x as i16 - left as i16, self.y as i16 - top as i16];
        self.x += width;
        self.row_height = self.row_height.max(height);
        Ok(HudImage {
            texture: UiTexture {
                path: PATH.into(),
                width: SIDE as u32,
                height: SIDE as u32,
            },
            offset,
        })
    }
}
