//! A ring of inward rays followed by an expanding ring and an outward spray.
use super::{Births, normalized, particle, rotated};
use crate::{
    effect::{RING_SPRITE, STREAK_SPRITE, SpriteOrientation},
    world::random,
};
use resonance_content::effect::VerticalAnchor;

#[derive(Debug, Clone, Default)]
pub(super) struct Burst {
    pub radius: f32,
    pub spread: u32,
    pub tilt: f32,
    pub target: [f32; 3],
}
impl Burst {
    pub fn emit(
        &self,
        center: [f32; 3],
        born: u32,
        phase: &mut u8,
        rng: &mut u32,
        out: &mut Births,
    ) {
        random(rng);
        const RAYS: usize = 500;
        const INWARD_ORBS: usize = 100;
        const OUTWARD_ORBS: usize = 250;
        const INWARD_SPEED: f32 = 15.;
        match *phase {
            0 => {
                let axis = normalized(std::array::from_fn(|i| self.target[i] - center[i]));
                for index in 0..RAYS + INWARD_ORBS {
                    let ray = index < RAYS;
                    let mut p = particle(center, born, (61 + random(rng) % 2) as u16, 1);
                    p.rgba[3] = (if ray { 50 } else { 100 } + random(rng) % 50) as u8;
                    if ray {
                        p.recipe = STREAK_SPRITE;
                        let height = (5 + random(rng) % 5) as f32;
                        p.size = [(100 + random(rng) % 100) as f32, height];
                        p.orientation = SpriteOrientation::World;
                        p.anchor = VerticalAnchor::Bottom;
                    } else {
                        p.size = [(25 + random(rng) % 50) as f32; 2];
                    }
                    let angle = (random(rng) % 360) as f32;
                    let distance = self.radius + super::stream::spread(rng, self.spread) as f32;
                    let offset = rotated([distance, 0., 0.], axis, angle);
                    p.position = std::array::from_fn(|i| center[i] + offset[i]);
                    p.velocity = normalized(offset).map(|v| -v * INWARD_SPEED);
                    p.lifetime = (distance.abs() / INWARD_SPEED + 10.) as u32 + 1;
                    p.fade = crate::effect::Fade::Linear(0.);
                    if ray {
                        p.rotation = [90., -angle, self.tilt];
                    }
                    out.push(p);
                }
                *phase = 1;
            }
            2 => {
                let mut ring = particle(center, born, 33, 301);
                ring.recipe = RING_SPRITE;
                ring.orientation = SpriteOrientation::World;
                ring.size = [0.; 2];
                ring.size_delta = 50.;
                ring.rgba[3] = 100;
                ring.fade = crate::effect::Fade::Linear(0.);
                out.push(ring);
                for _ in 0..OUTWARD_ORBS {
                    let mut p = particle(center, born, (61 + random(rng) % 4) as u16, 301);
                    p.rgba[3] = (100 + random(rng) % 50) as u8;
                    p.fade = crate::effect::Fade::Linear(0.);
                    p.size = [(15 + random(rng) % 50) as f32; 2];
                    let speed = (10 + random(rng) % 10) as f32;
                    let yaw = (random(rng) % 360) as f32;
                    let tilt = 35. - (random(rng) % 70) as f32;
                    p.velocity = rotated(
                        rotated([speed, 0., 0.], [0., 0., 1.], yaw),
                        [0., 1., 0.],
                        tilt,
                    );
                    out.push(p);
                }
                *phase = 3;
            }
            _ => {}
        }
    }
}
