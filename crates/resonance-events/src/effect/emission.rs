//! Shared particle births. Presets describe appearance; particles own motion and expiry.
use super::{BillboardEffect, Fade};

pub(crate) fn sprite(image: u16, size: f32, lifetime: u32) -> BillboardEffect {
    BillboardEffect {
        recipe: image,
        size: [size; 2],
        lifetime,
        rgba: [super::NEUTRAL_TINT; 4],
        fade: Fade::tail(lifetime),
        ..Default::default()
    }
}

pub(crate) fn normalized(v: [f32; 3]) -> [f32; 3] {
    let length = v.iter().map(|v| v * v).sum::<f32>().sqrt();
    v.map(|v| if length == 0. { 0. } else { v / length })
}
