//! Clustered light shafts and a contracting flash with inward rays.
use super::{palette, particle};
use crate::{
    effect::{BillboardEffect, Fade, SpriteOrientation},
    world::random,
};
use resonance_content::effect::STREAK_SPRITE;

#[derive(Debug, Clone)]
pub(super) struct Shafts {
    pub palette: i32,
    pub interval: u32,
    pub radius: f32,
    pub size: [f32; 2],
    pub variation: [u32; 2],
    pub tilt: f32,
    pub cluster: u32,
}

impl Shafts {
    pub fn emit(&self, center: [f32; 3], born: u32, rng: &mut u32, out: &mut Vec<BillboardEffect>) {
        const FADE_TICKS: u32 = 40;
        const PEAK_ALPHA: f32 = 50.;
        const HORIZONTAL_JITTER: i32 = 15;
        let angle = (random(rng) % 360) as f32;
        let (sin, cos) = angle.to_radians().sin_cos();
        let count = 1 + random(rng) % self.cluster;
        for _ in 0..count {
            let mut p = particle(center, born, palette(self.palette, rng), 2 * FADE_TICKS + 1);
            p.recipe = STREAK_SPRITE;
            p.orientation = SpriteOrientation::World;
            p.position[0] += sin * self.radius
                + (random(rng) % (2 * HORIZONTAL_JITTER + 1) as u32) as f32
                - HORIZONTAL_JITTER as f32;
            p.position[1] += cos * self.radius;
            p.rotation = [90. + cos * self.tilt, -sin * self.tilt, 0.];
            p.size =
                std::array::from_fn(|i| self.size[i] + (random(rng) % self.variation[i]) as f32);
            p.fade = Fade::RiseFall {
                rise_ticks: FADE_TICKS,
                step: PEAK_ALPHA / FADE_TICKS as f32,
            };
            out.push(p);
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Convergence {
    pub palette: i32,
    pub size: f32,
    pub growth: f32,
    pub radius: u32,
    pub spread: u32,
}

impl Convergence {
    pub fn emit(&self, center: [f32; 3], born: u32, rng: &mut u32, out: &mut Vec<BillboardEffect>) {
        const RAYS: usize = 100;
        const SPEED: f32 = 20.;
        let mut flash = particle(center, born, self.palette as u16, 600);
        flash.recipe = crate::effect::WORLD_GLOW_SPRITE;
        flash.size = [self.size; 2];
        flash.size_delta = self.growth;
        flash.rgba[3] = 100;
        flash.fade = Fade::Linear(0.);
        out.push(flash);
        for _ in 0..RAYS {
            let angle = (random(rng) % 360) as f32;
            let (sin, cos) = angle.to_radians().sin_cos();
            let radius = (self.radius + random(rng) % self.spread) as f32;
            let mut p = particle(center, born, palette(105, rng), (radius / SPEED) as u32 + 1);
            p.recipe = STREAK_SPRITE;
            p.orientation = SpriteOrientation::World;
            p.position[0] -= sin * radius;
            p.position[2] -= cos * radius;
            p.velocity = [sin * SPEED, 0., cos * SPEED];
            p.rotation = [90., angle, 0.];
            p.size = [
                (5 + random(rng) % 5) as f32,
                (50 + random(rng) % 100) as f32,
            ];
            p.rgba[3] = (25 + random(rng) % 25) as u8;
            p.fade = Fade::Linear(0.);
            out.push(p);
        }
    }
}
