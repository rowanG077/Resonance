//! Seal layers finish independently after their source stops emitting.
use super::{particle, rotated};
use crate::{
    effect::{BillboardEffect, Fade, SEAL_SPARK_SPRITE, STAR_SPRITE},
    world::random,
};

const RELEASE_SPARKS: usize = 250;
const IDLE_SPARKS: usize = 15;
const IDLE_BURST_INTERVAL: u32 = 30;
const PULSE_INTERVAL: u32 = 5;
const GLOW_GROWTH: f32 = 10.;
const WHITE_PALETTE: u16 = 33;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Release,
    Pulsing,
    Collapse,
    Done,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Seal {
    pub palette: u16,
    pub size: f32,
    pub spark_size: f32,
    pub spark_lifetime: u32,
    pub phase: Phase,
}

impl Seal {
    pub fn emit(
        &mut self,
        center: [f32; 3],
        born: u32,
        clock: u32,
        rng: &mut u32,
        out: &mut super::Births,
    ) {
        random(rng);
        let glow = |color, size, lifetime: u32, alpha, growth, fade| {
            let mut p = particle(center, born, color, lifetime + 1);
            p.size = [size; 2];
            p.rgba[3] = alpha;
            p.size_delta = growth;
            p.fade = fade;
            p
        };
        match self.phase {
            Phase::Idle => {
                out.push(glow(
                    self.palette,
                    self.size + (random(rng) & 15) as f32,
                    4,
                    143,
                    0.,
                    Fade::Linear(0.),
                ));
                out.push(glow(
                    self.palette + 3,
                    self.size + 30. + (clock as f32).to_radians().sin() * 20.,
                    1,
                    32,
                    0.,
                    Fade::Linear(0.),
                ));
                // The recurring spark has its own birth sample before color and motion.
                random(rng);
                out.push(
                    self.spark(center, born, STAR_SPRITE, self.spark_lifetime, rng)
                        .0,
                );
                if clock.is_multiple_of(IDLE_BURST_INTERVAL) {
                    self.burst(center, born, false, rng, out);
                }
            }
            Phase::Release => {
                for color in [WHITE_PALETTE, self.palette] {
                    out.push(glow(
                        color,
                        self.size,
                        180,
                        200,
                        GLOW_GROWTH,
                        Fade::Linear(-2.),
                    ));
                }
                self.burst(center, born, true, rng, out);
                self.phase = Phase::Pulsing;
            }
            Phase::Pulsing if clock.is_multiple_of(PULSE_INTERVAL) => {
                for color in [WHITE_PALETTE, self.palette] {
                    out.push(glow(color, self.size, 30, 100, GLOW_GROWTH, Fade::tail(30)));
                }
            }
            Phase::Collapse => {
                for color in [WHITE_PALETTE, self.palette] {
                    out.push(glow(color, self.size * 5., 300, 150, -3., Fade::Linear(0.)));
                }
                self.phase = Phase::Done;
            }
            _ => {}
        }
    }

    fn spark(
        &self,
        center: [f32; 3],
        born: u32,
        sprite: u16,
        lifetime: u32,
        rng: &mut u32,
    ) -> (BillboardEffect, [f32; 3]) {
        let color = 65 + (random(rng) & 31) as u16;
        let spin = if random(rng).is_multiple_of(2) {
            -3.
        } else {
            3.
        };
        let mut direction = [1.; 3];
        for axis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
            direction = rotated(direction, axis, random(rng) as f32);
        }
        let mut p = particle(center, born, color, lifetime + 1);
        p.recipe = sprite;
        p.size = [self.spark_size; 2];
        p.rotation[2] = if sprite == STAR_SPRITE { 45. } else { 0. };
        p.angular_velocity[2] = spin;
        (p, direction)
    }

    fn burst(
        &self,
        center: [f32; 3],
        born: u32,
        release: bool,
        rng: &mut u32,
        out: &mut super::Births,
    ) {
        for _ in 0..if release { RELEASE_SPARKS } else { IDLE_SPARKS } {
            let sprite = if random(rng).is_multiple_of(2) {
                SEAL_SPARK_SPRITE
            } else {
                STAR_SPRITE
            };
            let (mut p, direction) = self.spark(
                center,
                born,
                sprite,
                if release { self.spark_lifetime } else { 30 },
                rng,
            );
            let distance = super::stream::spread(rng, (self.size / 2.) as u32) as f32;
            for i in 0..3 {
                p.position[i] += direction[i] * distance;
            }
            out.push(p);
        }
    }
}
