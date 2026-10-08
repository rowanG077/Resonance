//! Stationary tiles that make distortion visible in effect comparisons.
use anyhow::{Result, ensure};
use resonance_events::{
    GameWorld,
    effect::{BillboardEffect, Blend},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Backdrop {
    pub position: [f32; 3],
    pub size: [f32; 2],
    pub shade: u8,
}

impl Backdrop {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            self.position
                .iter()
                .chain(&self.size)
                .all(|v| v.is_finite())
                && self.size.iter().all(|v| *v > 0.),
            "invalid backdrop tile"
        );
        Ok(())
    }

    pub(super) fn apply(&self, world: &mut GameWorld, index: usize) {
        let mut tile = BillboardEffect::default();
        tile.draw_order = index;
        // Sample inside the atlas's opaque white patch.
        tile.recipe = 4;
        tile.uv = Some([32. / 256.; 4]);
        tile.born = world.tick;
        tile.lifetime = u32::MAX;
        tile.position = self.position;
        tile.size = self.size;
        tile.rgba = [self.shade, self.shade, self.shade, 255];
        tile.blend = Some(Blend::Alpha);
        world.billboards.insert(i32::MIN + index as i32, tile);
    }
}
