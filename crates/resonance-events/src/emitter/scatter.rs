//! Soft sparks surround brighter motes whose paths wander as they fade.
use super::{normalized, particle};
use crate::{
    effect::{BillboardController, Fade, GLOW_SPRITE},
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
}
impl Scatter {
    #[expect(
        clippy::too_many_arguments,
        reason = "Emission borrows actor state, clocks, randomness and its output batch."
    )]
    pub(super) fn emit(
        &self,
        center: [f32; 3],
        born: u32,
        clock: u32,
        speed: f32,
        planar: bool,
        rng: &mut u32,
        out: &mut super::Births,
    ) {
        const PLANAR_SPEED: f32 = 0.8;
        let speed = if planar { PLANAR_SPEED } else { speed };
        random(rng);
        let size = self.size[0] + (random(rng) % self.variation[0]) as f32;
        let x = 90. - (random(rng) % 180) as f32;
        let z = 90. - (random(rng) % 180) as f32;
        let y = if planar {
            0.
        } else {
            90. - (random(rng) % 180) as f32
        };
        random(rng);
        let growth = if random(rng).is_multiple_of(2) {
            -0.25
        } else {
            0.25
        };
        let angle = (random(rng) % 360) as f32;
        let mut spark = particle(center, born, self.palette, self.lifetime[0]);
        spark.recipe = GLOW_SPRITE;
        spark.size = [size; 2];
        spark.size_delta = growth;
        spark.rotation[2] = angle;
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
        spark.blend = Some(crate::effect::Blend::Additive);
        out.push(spark);
        if !clock.is_multiple_of(3) {
            return;
        }
        let stride = if self.palette < CONTIGUOUS_SHADES_START {
            PALETTE_SHADE_STRIDE
        } else {
            1
        };
        let color = self.palette + stride * (random(rng) % u32::from(PALETTE_SHADES)) as u16;
        let size = self.size[1] + (random(rng) % self.variation[1]) as f32;
        let direction = std::array::from_fn(|axis| {
            if planar && axis == 1 {
                0.
            } else {
                90. - (random(rng) % 180) as f32
            }
        });
        let speed_sample = random(rng);
        let mut mote = particle(center, born, color, self.lifetime[1]);
        mote.size = [size; 2];
        mote.rgba[3] = 150;
        mote.controller = Some(BillboardController::Scatter {
            direction,
            speed: if planar {
                PLANAR_SPEED + f32::from(speed_sample % 12 >= 10)
            } else {
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
