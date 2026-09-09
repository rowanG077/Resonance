//! Pixel coverage for translucent sliced frames, independent of scene depth.
use super::Batch;
use anyhow::{Result, ensure};
use bevy::{prelude::*, render::render_resource::ShaderType};
use resonance_content::font::DialogueArt;

const LIMIT: usize = 64;

#[derive(Clone, Copy, Default, Debug, PartialEq, ShaderType)]
pub(super) struct Quad {
    rect: Vec4,
    uv: Vec4,
    // Atlas index and the vertex alpha after the window's opening opacity.
    properties: Vec4,
}

#[derive(Clone, Debug, PartialEq, ShaderType)]
pub(super) struct Coverage {
    count: UVec4,
    quads: [Quad; LIMIT],
}
impl Default for Coverage {
    fn default() -> Self {
        Self {
            count: UVec4::ZERO,
            quads: [Quad::default(); LIMIT],
        }
    }
}
impl Coverage {
    pub(super) fn frame(batch: &Batch, art: &DialogueArt, opening: bool) -> Result<Self> {
        let mut result = Self::default();
        result.append(batch, art, 0, opening)?;
        Ok(result)
    }
    pub(super) fn with_color(
        &self,
        batch: &Batch,
        art: &DialogueArt,
        opening: bool,
    ) -> Result<Self> {
        let mut result = self.clone();
        result.append(batch, art, 1, opening)?;
        Ok(result)
    }
    pub(super) fn with_frame(
        &self,
        batch: &Batch,
        art: &DialogueArt,
        opening: bool,
    ) -> Result<Self> {
        let mut result = self.clone();
        result.append(batch, art, 0, opening)?;
        Ok(result)
    }
    fn append(
        &mut self,
        batch: &Batch,
        art: &DialogueArt,
        atlas: usize,
        opening: bool,
    ) -> Result<()> {
        let texture = &art.textures[atlas];
        for first in (0..batch.positions.len()).step_by(4) {
            let a = batch.positions[first];
            let b = batch.positions[first + 2];
            if a[0] == b[0] || a[1] == b[1] {
                continue;
            }
            let count = self.count.x as usize;
            ensure!(count < LIMIT, "dialogue frame exceeds coverage budget");
            let uv0 = batch.uv[first];
            let uv1 = batch.uv[first + 2];
            let alpha = batch.colors[first][3];
            let alpha = if opening {
                (alpha * 128.).trunc() / 255.
            } else {
                alpha
            };
            self.quads[count] = Quad {
                rect: Vec4::new(a[0] + 320., 240. - a[1], b[0] + 320., 240. - b[1]),
                uv: Vec4::new(
                    uv0[0] / texture.width as f32,
                    uv0[1] / texture.height as f32,
                    uv1[0] / texture.width as f32,
                    uv1[1] / texture.height as f32,
                ),
                properties: Vec4::new(atlas as f32, alpha, 0., 0.),
            };
            self.count.x += 1;
        }
        Ok(())
    }
}
