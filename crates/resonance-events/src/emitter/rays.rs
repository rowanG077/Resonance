//! Clustered light shafts and a contracting flash with inward rays.
use super::{palette, particle};
use crate::{
    effect::{BillboardEffect, Fade, SpriteOrientation},
    world::random,
};
use resonance_content::effect::STREAK_SPRITE;

#[allow(clippy::too_many_arguments)]
pub(super) fn aura(
    center: [f32; 3],
    born: u32,
    age: u32,
    owner: i32,
    camera: [f32; 3],
    color: i32,
    offset: f32,
    rng: &mut u32,
    out: &mut Vec<BillboardEffect>,
) {
    const GROWTH: f32 = 3.;
    const SPARK_THRESHOLD: f32 = 100.;
    random(rng);
    let mut glow = particle(center, born, palette(color, rng), 6);
    glow.field_lighting = false;
    glow.size = [age as f32 * GROWTH; 2];
    glow.rgba[3] = 70;
    glow.fade = Fade::Linear(-5.);
    glow.controller = Some(crate::effect::BillboardController::CameraOffset {
        emitter: owner,
        center,
        distance: offset,
    });
    let sparks = glow.size[0] > SPARK_THRESHOLD;
    out.push(glow);
    if !sparks {
        return;
    }
    let size = (25 + random(rng) % (age * 3 / 40).max(1)) as f32;
    let angle = (random(rng) % 360) as f32;
    let axis = super::normalized(camera);
    let direction = super::rotated([axis[1], -axis[0], -axis[2]], axis, -angle);
    let mut spark = particle(center, born, 0, 63);
    let horizontal = super::normalized([camera[0], camera[1], 0.]);
    spark.position = std::array::from_fn(|i| center[i] - horizontal[i] * offset);
    spark.palette = None;
    spark.size = [size; 2];
    spark.rgba = [63, 63, 63, 255];
    spark.velocity = direction.map(|v| v * age as f32 * 0.024);
    spark.fade = Fade::Tail { after: 31 };
    out.push(spark);
}

pub(super) fn crown(
    center: [f32; 3],
    born: u32,
    color: i32,
    radius: f32,
    spread: f32,
    rng: &mut u32,
    out: &mut Vec<BillboardEffect>,
) {
    const SPOKES: u32 = 72;
    let variation = (spread as u32 / 2).max(1);
    for spoke in 1..=SPOKES {
        let color = palette(color, rng);
        let size = [
            (8 + random(rng) % 5) as f32,
            (100 + random(rng) % 100) as f32,
        ];
        let (sin, cos) = (spoke as f32 * 360. / SPOKES as f32).to_radians().sin_cos();
        let direction = [
            cos * (spread + (random(rng) % variation) as f32),
            sin * (spread + (random(rng) % variation) as f32),
            100.,
        ];
        let speed = (20 + random(rng) % 20) as f32;
        let mut p = particle(center, born, color, 301);
        p.recipe = STREAK_SPRITE;
        p.velocity = super::normalized(direction).map(|v| v * speed);
        p.position[0] += cos * radius;
        p.position[1] += sin * radius;
        p.rotation = [-direction[1], 0., -direction[0]];
        p.size = size;
        p.rgba[3] = 30;
        p.fade = Fade::Linear(-2.);
        out.push(p);
    }
}

#[derive(Debug, Clone)]
pub(super) struct RisingOrbs {
    pub palette: i32,
    pub radius: f32,
    pub size: f32,
    pub variation: u32,
    pub lighting: bool,
    pub speed_variation: u32,
    pub alpha: u8,
    pub fade: f32,
    pub interval: u32,
    pub preserve_particles: bool,
    pub drifting: bool,
}
impl RisingOrbs {
    pub fn emit(
        &self,
        center: [f32; 3],
        born: u32,
        tick: u32,
        (owner, actor): (i32, &crate::Actor),
        rng: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        random(rng);
        if !tick.is_multiple_of(self.interval) {
            return;
        }
        const MOTE_SPRITES: [u16; 3] = [crate::effect::ORB_SPRITE, 12, 69];
        const WHITE_MOTE: usize = 2;
        let style = if self.drifting {
            random(rng) as usize % MOTE_SPRITES.len()
        } else {
            0
        };
        let color = if style == WHITE_MOTE {
            u16::from(crate::effect::NEUTRAL_PALETTE)
        } else {
            palette(self.palette, rng)
                + if self.drifting {
                    (random(rng) % 4) as u16
                } else {
                    0
                }
        };
        let size = (self.size + (random(rng) % self.variation) as f32)
            * if style == WHITE_MOTE { 1.2 } else { 1. };
        let rise = (actor.movement_speed() + (random(rng) % self.speed_variation) as f32) / 100.;
        let spin = if self.drifting {
            if random(rng).is_multiple_of(2) {
                -1.
            } else {
                1.
            }
        } else {
            0.
        };
        let angle = (random(rng) % 360) as f32;
        let (sin, cos) = angle.to_radians().sin_cos();
        let radius = if self.drifting {
            (random(rng) % (self.radius as u32).max(1)) as f32
        } else {
            self.radius
        };
        let mut p = particle(center, born, color, 301);
        p.recipe = MOTE_SPRITES[style];
        p.position[0] += sin * radius;
        p.position[1] -= cos * radius;
        p.velocity[2] = rise;
        if self.drifting {
            p.controller = Some(crate::effect::BillboardController::Drift {
                direction: [0., 0., 100.],
                speed: rise,
            });
            p.angular_velocity[2] = spin;
            p.rotation[2] = spin;
            super::inherit(&mut p, actor);
        } else {
            p.owner = Some(owner);
        }
        p.size = [size; 2];
        p.rgba[3] = self.alpha;
        p.fade = Fade::Linear(self.fade);
        p.field_lighting = self.lighting;
        out.push(p);
    }
}

#[derive(Debug, Clone)]
pub(super) struct Bloom {
    pub palette: i32,
    pub lifetime: u32,
    pub count: u32,
    pub size: [f32; 2],
    pub variation: [u32; 2],
}
impl Bloom {
    pub fn emit(
        &self,
        center: [f32; 3],
        born: u32,
        speed: f32,
        rng: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        for _ in 0..self.count {
            let color = palette(self.palette, rng);
            let size =
                std::array::from_fn(|i| self.size[i] + (random(rng) % self.variation[i]) as f32);
            let angle = 90. - (random(rng) % 360) as f32;
            let (sin, cos) = angle.to_radians().sin_cos();
            let mut p = particle(center, born, color, self.lifetime + 1);
            p.recipe = STREAK_SPRITE;
            p.orientation = SpriteOrientation::World;
            p.rotation = [90., angle, 0.];
            p.velocity = [sin * speed, 0., cos * speed];
            p.size = size;
            p.rgba[3] = 150;
            p.field_lighting = false;
            out.push(p);
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Rising {
    pub palette: i32,
    pub radius: f32,
    pub size: [f32; 2],
    pub variation: [u32; 2],
    pub alpha: u8,
    pub rise: u32,
    pub world: bool,
    pub lifetime: u32,
}
impl Rising {
    pub fn emit(
        &self,
        center: [f32; 3],
        born: u32,
        camera: [f32; 3],
        rng: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        random(rng);
        let size = std::array::from_fn(|i| self.size[i] + (random(rng) % self.variation[i]) as f32);
        let speed = (random(rng) % self.rise) as f32 / 100.;
        let angle = (random(rng) % 360) as f32;
        let (sin, cos) = angle.to_radians().sin_cos();
        let mut p = particle(center, born, self.palette as u16, self.lifetime);
        p.recipe = STREAK_SPRITE;
        p.position[0] += sin * self.radius;
        p.position[1] -= cos * self.radius;
        p.position[2] += speed;
        p.velocity[2] = speed;
        p.size = size;
        p.rgba[3] = self.alpha;
        if self.world {
            p.orientation = SpriteOrientation::World;
            p.rotation = [90., 0., camera[0].atan2(-camera[1]).to_degrees()];
        }
        out.push(p);
    }
}

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
        let size = |rng: &mut u32| {
            std::array::from_fn(|i| self.size[i] + (random(rng) % self.variation[i]) as f32)
        };
        random(rng);
        let first_color = palette(self.palette, rng);
        let first_size: [f32; 2] = size(rng);
        let angle = 180. - (random(rng) % 360) as f32;
        let (sin, cos) = angle.to_radians().sin_cos();
        let count = 1 + random(rng) % self.cluster;
        for index in 0..count {
            let (color, jitter, size) = if index == 0 {
                (first_color, 0., first_size)
            } else {
                let color = palette(self.palette, rng);
                let jitter = (first_size[0] / 2.).trunc()
                    - (random(rng) % (first_size[0] as u32).max(1)) as f32;
                (color, jitter, size(rng))
            };
            let mut p = particle(center, born, color, 2 * FADE_TICKS + 2);
            p.recipe = STREAK_SPRITE;
            p.orientation = SpriteOrientation::World;
            p.anchor = resonance_content::effect::VerticalAnchor::Top;
            p.position[0] += sin * self.radius + jitter;
            p.position[1] += cos * self.radius;
            p.rotation = [90. + cos * self.tilt, -sin * self.tilt, 0.];
            p.size = size;
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
            let color = palette(108, rng);
            let alpha = (25 + random(rng) % 25) as u8;
            let size = [
                (5 + random(rng) % 5) as f32,
                (50 + random(rng) % 100) as f32,
            ];
            let angle = (random(rng) % 360) as f32 - 90.;
            let (sin, cos) = angle.to_radians().sin_cos();
            let radius = (self.radius + random(rng) % self.spread) as f32;
            let mut p = particle(center, born, color, (radius / SPEED) as u32 + 1);
            p.recipe = STREAK_SPRITE;
            p.orientation = SpriteOrientation::World;
            p.anchor = resonance_content::effect::VerticalAnchor::Bottom;
            p.position[0] -= sin * radius;
            p.position[2] -= cos * radius;
            p.velocity = [sin * SPEED, 0., cos * SPEED];
            p.rotation = [90., angle, 0.];
            p.size = size;
            p.rgba[3] = alpha;
            p.fade = Fade::Linear(0.);
            out.push(p);
        }
    }
}
