//! Long streaks descending through a seal's center.
use crate::effect::{BillboardEffect, Fade, NEUTRAL_TINT, SpriteOrientation};

pub(super) fn particle(center: [f32; 3], born: u32, random: &mut u32) -> BillboardEffect {
    let theta = (crate::world::random(random) % 60 + 60) as f32;
    let phi = 25. - (crate::world::random(random) % 50) as f32;
    let (sin_y, cos_y) = theta.to_radians().sin_cos();
    let (sin_x, cos_x) = phi.to_radians().sin_cos();
    // Rx(phi) * Ry(-theta) * [1, 0, 0], from a radius of 1000 units.
    let direction = [cos_y, -sin_x * sin_y, cos_x * sin_y];
    let alpha = (crate::world::random(random) % 50 + 100) as u8;
    let size = [
        (crate::world::random(random) % 10 + 10) as f32,
        (crate::world::random(random) % 300 + 200) as f32,
    ];
    let speed = (crate::world::random(random) % 20 + 30) as f32;
    const LIFETIME: u32 = 121;
    BillboardEffect {
        orientation: SpriteOrientation::World,
        anchor: resonance_content::effect::VerticalAnchor::Bottom,
        palette: Some(33),
        lifetime: LIFETIME,
        position: std::array::from_fn(|i| center[i] + direction[i] * 1000.),
        velocity: direction.map(|v| -v * speed),
        rotation: [90., 90. - theta, 0.],
        size,
        rgba: [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, alpha],
        fade: Fade::tail(LIFETIME),
        ..super::particle([0.; 3], born, 0, 1)
    }
}
