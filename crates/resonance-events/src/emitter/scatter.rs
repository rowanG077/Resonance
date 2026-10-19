//! Soft sparks surround brighter motes whose paths wander as they fade.
use super::{Orbit, normalized, particle};
use crate::{
    effect::{BillboardController, Blend, Fade, GLOW_SPRITE},
    world::random,
};

pub(crate) const RISE_PER_TICK: f32 = 0.98;
const PALETTE_SHADE_STRIDE: u16 = 7;
const PALETTE_SHADES: u16 = 4;
const CONTIGUOUS_SHADES_START: u16 = 65;
pub(super) const MAX_PALETTE: i32 =
    resonance_content::effect::FIELD_PALETTE_COLORS as i32 - PALETTE_SHADES as i32;

#[derive(Debug, Clone, Default)]
pub(crate) struct Scatter {
    pub palette: u16,
    pub size: [f32; 2],
    pub variation: [u32; 2],
    pub lifetime: [u32; 2],
    turn: f32,
}
impl Scatter {
    #[allow(
        clippy::too_many_arguments,
        reason = "Keep independent timing and mutable emission state explicit at the call site"
    )]
    pub(super) fn emit(
        &mut self,
        center: [f32; 3],
        born: u32,
        clock: u32,
        speed: f32,
        phase: u8,
        blend: Option<Blend>,
        rng: &mut u32,
        out: &mut super::Births,
    ) {
        const PLANAR_SPEED: f32 = 0.8;
        random(rng);
        if phase > 2 {
            return;
        }
        let planar = phase == 1;
        let release = phase == 2;
        let periodic = clock.is_multiple_of(3);
        let speed = if planar { PLANAR_SPEED } else { speed };
        let glows = match (release, periodic) {
            (true, true) => 5,
            (true, false) => 0,
            (false, _) => 1,
        };
        for spoke in 0..glows {
            let size = self.size[0] + (random(rng) % self.variation[0]) as f32;
            let mut spark = particle(center, born, self.palette, self.lifetime[0]);
            spark.recipe = GLOW_SPRITE;
            spark.size = [size; 2];
            spark.blend = Some(blend.unwrap_or(Blend::Additive));
            if release {
                let mut orbit = Orbit::new(
                    center,
                    [0., 1., 0.],
                    1.,
                    1.,
                    1. - self.turn - spoke as f32 * 72.,
                    0.1,
                );
                orbit.radial = [1., 0., 0.];
                spark.position = orbit.position(0);
                spark.controller = Some(BillboardController::Orbit(orbit));
                spark.size_delta = 0.25;
                spark.rgba[3] = 50;
            } else {
                let x = 90. - (random(rng) % 180) as f32;
                let z = 90. - (random(rng) % 180) as f32;
                let y = if planar {
                    0.
                } else {
                    90. - (random(rng) % 180) as f32
                };
                random(rng);
                spark.size_delta = if random(rng).is_multiple_of(2) {
                    -0.25
                } else {
                    0.25
                };
                spark.velocity = normalized([x, y, z]).map(|v| v * speed);
                if planar {
                    spark.controller = Some(BillboardController::Scatter {
                        direction: [x, y, z],
                        speed,
                        planar,
                        wandering: false,
                    });
                }
                spark.rgba[3] = 125;
                spark.fade = Fade::Linear(-0.625);
            }
            spark.rotation[2] = (random(rng) % 360) as f32;
            out.push(spark);
        }
        if !periodic {
            return;
        }
        if release {
            self.turn += 4.;
        }
        let color = if blend.is_some() {
            if phase == 0 {
                super::palette(108, rng)
            } else {
                36
            }
        } else {
            let stride = if self.palette < CONTIGUOUS_SHADES_START {
                PALETTE_SHADE_STRIDE
            } else {
                1
            };
            self.palette + stride * (random(rng) % u32::from(PALETTE_SHADES)) as u16
        };
        let size = self.size[1] + (random(rng) % self.variation[1]) as f32;
        let direction = std::array::from_fn(|axis| {
            if phase != 0 && axis == 1 {
                0.
            } else {
                90. - (random(rng) % 180) as f32
            }
        });
        let speed_sample = random(rng);
        let mut mote = particle(center, born, color, self.lifetime[1]);
        mote.size = [size; 2];
        mote.rgba[3] = 150;
        mote.blend = blend.map(|_| Blend::Subtractive);
        mote.controller = Some(BillboardController::Scatter {
            direction,
            speed: if planar {
                PLANAR_SPEED + f32::from(speed_sample % 12 >= 10)
            } else {
                if release {
                    random(rng);
                }
                speed
            },
            planar,
            wandering: true,
        });
        out.push(mote);
    }
}

pub(crate) fn wander(direction: &mut [f32; 3], rng: &mut u32) {
    let axis = if random(rng).is_multiple_of(2) { 2 } else { 0 };
    if direction[2 - axis] == 0. {
        return;
    }
    direction[axis] += if random(rng).is_multiple_of(2) {
        -2.
    } else {
        2.
    };
}

pub(crate) fn drift(direction: &mut [f32; 3], rng: &mut u32) {
    for value in &mut direction[..2] {
        *value += if random(rng).is_multiple_of(2) {
            -2.5
        } else {
            2.5
        };
    }
}

pub(crate) fn diffuse(direction: &mut [f32; 3], rng: &mut u32) {
    for value in direction {
        *value += if random(rng).is_multiple_of(2) {
            -2.
        } else {
            2.
        };
    }
}
