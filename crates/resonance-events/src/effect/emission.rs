//! Shared particle births. Presets describe appearance; particles own motion and expiry.
use super::{BillboardEffect, Fade};
use crate::world::random_unit;

#[derive(Debug, Clone)]
pub(crate) struct Emission {
    pub particle: BillboardEffect,
    pub count: usize,
    pub spread: f32,
    pub speed: f32,
    pub size_variation: f32,
}
impl Emission {
    pub fn emit(&self, random: &mut u32, out: &mut Vec<BillboardEffect>) {
        for _ in 0..self.count {
            let mut particle = self.particle.clone();
            let direction = normalized(std::array::from_fn(|_| random_unit(random) * 2. - 1.));
            let radius = random_unit(random) * self.spread;
            let size = random_unit(random) * self.size_variation;
            for (axis, direction) in direction.into_iter().enumerate() {
                particle.position[axis] += direction * radius;
                particle.velocity[axis] += direction * self.speed;
            }
            particle.size = particle.size.map(|v| (v + size).max(0.));
            out.push(particle);
        }
    }
}

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
