//! Soft sparks surround brighter motes whose paths wander as they fade.
use super::{normalized, particle};
use crate::{
    effect::{BillboardController, BillboardEffect, Fade, GLOW_SPRITE},
    world::random,
};

#[derive(Debug, Clone)]
pub(super) struct Scatter {
    pub palette: u16,
    pub size: [f32; 2],
    pub variation: [u32; 2],
    pub lifetime: [u32; 2],
}
impl Scatter {
    pub fn emit(
        &self,
        center: [f32; 3],
        born: u32,
        clock: u32,
        speed: f32,
        rng: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        random(rng);
        let size = self.size[0] + (random(rng) % self.variation[0]) as f32;
        let x = 90. - (random(rng) % 180) as f32;
        let z = 90. - (random(rng) % 180) as f32;
        let y = 90. - (random(rng) % 180) as f32;
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
        spark.rgba[3] = 125;
        spark.fade = Fade::Linear(-0.625);
        spark.blend = Some(crate::effect::Blend::Additive);
        out.push(spark);
        if !clock.is_multiple_of(3) {
            return;
        }
        let intensity = 1 + random(rng) % 4;
        let size = self.size[1] + (random(rng) % self.variation[1]) as f32;
        let direction = std::array::from_fn(|_| 90. - (random(rng) % 180) as f32);
        random(rng);
        let mut mote = particle(center, born, self.palette, self.lifetime[1]);
        mote.size = [size; 2];
        mote.rgba[3] = 150;
        mote.intensity = intensity as f32;
        mote.controller = Some(BillboardController::Wander { direction, speed });
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
