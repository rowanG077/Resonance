//! A shrinking red flame and a smaller yellow core rise at different speeds.
use crate::{
    effect::{BillboardEffect, Blend, Fade, GLOW_SPRITE},
    world::random,
};

pub(super) fn emit(
    center: [f32; 3],
    born: u32,
    clock: u32,
    size: f32,
    rng: &mut u32,
    out: &mut Vec<BillboardEffect>,
) {
    random(rng);
    if !clock.is_multiple_of(4) {
        return;
    }
    let red_size = size + (random(rng) % 16) as f32;
    let red_rise = 2. + (random(rng) % 16) as f32 / 16.;
    let drift = (random(rng) % 16) as f32 / 32.;
    let yellow_size = size / 2. + (random(rng) % 16) as f32;
    let yellow_rise = 2. + (random(rng) % 16) as f32 / 32.;
    for (diameter, velocity, rgba, lifetime, growth) in [
        (red_size, [drift, 0., red_rise], [255, 10, 10, 255], 61, -1.),
        (
            yellow_size,
            [0., 0., yellow_rise],
            [255, 255, 10, 255],
            31,
            0.,
        ),
    ] {
        out.push(BillboardEffect {
            recipe: GLOW_SPRITE,
            uv: Some([0., 0.25, 0.25, 0.5]),
            born,
            lifetime,
            position: center,
            velocity,
            size: [diameter; 2],
            size_delta: growth,
            rgba,
            fade: Fade::tail(lifetime),
            angular_velocity: [0., 0., -3.],
            blend: Some(Blend::Additive),
            ..Default::default()
        });
    }
}
