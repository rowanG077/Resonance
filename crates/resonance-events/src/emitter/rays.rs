//! Clustered light shafts and a contracting flash with inward rays.
use super::{palette, particle};
use crate::{
    effect::{Fade, SpriteOrientation},
    world::random,
};
use resonance_content::effect::sprite::STREAK_SPRITE;

pub(super) fn awakening(
    phase: &mut u8,
    size: &mut f32,
    center: [f32; 3],
    born: u32,
    clock: u32,
    rng: &mut u32,
    out: &mut super::Births,
) {
    use crate::effect::{BillboardEffect, STAR_SPRITE};
    const CHARGE_LIMIT: f32 = 700.;
    const CHARGE_STEP: f32 = 50.;
    let variation = random(rng) % 4;
    let mut star = particle(center, born, 2, 5);
    star.recipe = STAR_SPRITE;
    star.size = [20. + variation as f32; 2];
    star.rotation[2] = 45.;
    star.fade = Fade::Linear(-48.);
    out.push(star);
    let glow = |color, size, alpha, lifetime, fade| {
        let mut p = particle(center, born, color, lifetime);
        p.size = [size; 2];
        p.rgba[3] = alpha;
        p.fade = Fade::Linear(fade);
        p
    };
    let spark = |position| {
        let mut p = BillboardEffect::rising_spark(
            position,
            8.,
            -(1. + variation as f32 / 32.),
            born,
            clock,
        );
        p.palette = Some(0);
        p.fade = Fade::Linear(-4.);
        p
    };
    if *phase == 1 {
        if *size < CHARGE_LIMIT {
            out.push(glow(26, *size, 32, 3, -9.));
            *size += CHARGE_STEP;
        } else {
            out.push(glow(26, *size, 255, 121, -2.));
            *phase = 2;
        }
        let spread = (*size as u32 / 2).max(1);
        for _ in 0..20 {
            let position = std::array::from_fn(|i| {
                center[i] + (random(rng) % spread) as f32 - (spread / 2) as f32
            });
            out.push(spark(position));
        }
    }
    if *phase >= 1 {
        out.push(glow(
            26,
            80. + 20. * (clock as f32).to_radians().sin(),
            32,
            2,
            0.,
        ));
        out.push(glow(2, 50. + (random(rng) % 16) as f32, 143, 5, 0.));
        if clock.is_multiple_of(4) {
            let mut position = center;
            for value in &mut position[..2] {
                *value += (random(rng) % 16) as f32 - 7.;
            }
            out.push(spark(position));
        }
    }
}

pub(super) fn flash_sparks(
    center: [f32; 3],
    born: u32,
    camera: [f32; 3],
    rng: &mut u32,
    out: &mut super::Births,
) {
    for _ in 0..8 {
        let angle = (random(rng) % 360) as f32;
        for offset in [0., 90., 270., 180.] {
            let mut spark = particle(center, born, 72, 36);
            spark.recipe = crate::effect::SEAL_SPARK_SPRITE;
            spark.field_fog = false;
            spark.size = [75.; 2];
            spark.rgba[3] = 200;
            let speed = (9 + random(rng) % 5) as f32;
            spark.velocity = screen_radial(camera.map(|v| -v), angle + offset).map(|v| v * speed);
            out.push(spark);
        }
    }
}

fn screen_radial(camera: [f32; 3], angle: f32) -> [f32; 3] {
    let axis = super::normalized(camera);
    let radial = super::normalized([-axis[1], axis[0], axis[2]]);
    super::rotated(radial, axis, angle)
}

#[derive(Debug, Clone, Default)]
pub(super) struct ChargedRay {
    pub palette: u16,
    pub size: f32,
    pub variation: u32,
    pub interval: u32,
    pub direction: [f32; 3],
    pub radius: f32,
    pub radius_variation: u32,
    pub growth: f32,
}
impl ChargedRay {
    pub fn emit(
        &mut self,
        phase: &mut u8,
        (owner, center): (i32, [f32; 3]),
        born: u32,
        clock: u32,
        speed: f32,
        rng: &mut u32,
        out: &mut super::Births,
    ) -> Result<(), String> {
        random(rng);
        if clock.is_multiple_of(2) {
            return Ok(());
        }
        if *phase == 0 {
            self.direction = std::array::from_fn(|i| self.direction[i] - center[i]);
            *phase = 1;
        }
        if *phase <= 2 && speed <= 0. {
            return Err("charging ray needs positive particle speed".into());
        }
        if *phase == 1 && clock.is_multiple_of(self.interval) {
            let color = self.palette + (random(rng) % 4) as u16;
            let size = self.size + (random(rng) % self.variation) as f32;
            let radial = screen_radial(self.direction, (random(rng) % 360) as f32);
            let radius = self.radius + (random(rng) % self.radius_variation) as f32;
            let mut p = particle(center, born, color, (radius / speed) as u32 + 1);
            p.field_fog = false;
            p.size = [size; 2];
            p.position = std::array::from_fn(|i| center[i] + radial[i] * radius);
            p.velocity = radial.map(|v| -v * speed);
            out.push(p);
        } else if *phase == 2 {
            use crate::effect::{ELECTRIC_ARC_SPRITE, ORB_SPRITE};
            let distance = self.direction.iter().map(|v| v * v).sum::<f32>().sqrt();
            let velocity = super::normalized(self.direction).map(|v| v * speed);
            for (recipe, scale) in [(ORB_SPRITE, 1.), (ELECTRIC_ARC_SPRITE, 0.5)] {
                let mut p = particle(center, born, self.palette, (distance / speed) as u32 + 1);
                p.recipe = recipe;
                p.owner = Some(owner);
                p.field_fog = false;
                p.size = [self.size * scale; 2];
                p.size_delta = self.growth;
                p.velocity = velocity;
                if recipe == ELECTRIC_ARC_SPRITE {
                    p.angular_velocity[2] = if random(rng).is_multiple_of(2) {
                        -100.
                    } else {
                        100.
                    };
                }
                out.push(p);
            }
        }
        Ok(())
    }
}

pub(super) fn impact_spheres(
    center: [f32; 3],
    born: u32,
    texture: (u32, u8),
    rng: &mut u32,
    out: &mut super::Births,
) {
    for _ in 0..5 {
        let mut p = particle(center, born, 33, 31);
        p.texture = Some(texture);
        p.uv = Some([0., 0., 254. / 256., 254. / 256.]);
        p.size = [0.; 2];
        p.size_delta = 25.;
        p.blend = Some(crate::effect::Blend::Additive);
        p.rotation = std::array::from_fn(|_| (random(rng) % 360) as f32);
        out.push(p);
    }
    let mut glow = particle(center, born, 33, 31);
    glow.size = [0.; 2];
    glow.size_delta = 50.;
    out.push(glow);
}

pub(super) fn spray(
    center: [f32; 3],
    born: u32,
    camera: [f32; 3],
    color: u16,
    rng: &mut u32,
    out: &mut super::Births,
) {
    use crate::effect::{ORB_SPRITE, SEAL_SPARK_SPRITE, STAR_SPRITE};
    random(rng);
    let mut p = particle(center, born, color, 36);
    p.recipe = [STAR_SPRITE, SEAL_SPARK_SPRITE, ORB_SPRITE][random(rng) as usize % 3];
    p.field_fog = false;
    p.size = [75.; 2];
    p.rgba[3] = 200;
    let speed = (9 + random(rng) % 5) as f32;
    if p.recipe == STAR_SPRITE {
        p.rotation[2] = 45.;
    }
    p.angular_velocity[2] = if random(rng).is_multiple_of(2) {
        -3.
    } else {
        3.
    };
    p.velocity = screen_radial(camera.map(|v| -v), (random(rng) % 360) as f32).map(|v| v * speed);
    out.push(p);
}

#[derive(Debug, Clone, Default)]
pub(super) struct Explosion {
    pub palette: u16,
    pub size: f32,
    pub variation: u32,
    pub count: u32,
    pub color_group: i32,
}
impl Explosion {
    pub fn emit(
        &self,
        center: [f32; 3],
        born: u32,
        actor: &crate::Actor,
        rng: &mut u32,
        out: &mut super::Births,
    ) {
        use crate::effect::{SEAL_SPARK_SPRITE, STAR_SPRITE};
        let mut glow = particle(center, born, self.palette, 181);
        glow.field_fog = false;
        glow.size = [actor.heading + (random(rng) % 10) as f32; 2];
        glow.rgba[3] = 200;
        glow.size_delta = 10.;
        glow.fade = Fade::Linear(-2.);
        out.push(glow);
        for _ in 0..self.count {
            let star = random(rng).is_multiple_of(2);
            let color = if (1..=4).contains(&self.color_group) {
                palette(104 + self.color_group, rng)
            } else {
                self.palette
            };
            let mut p = particle(center, born, color, 61);
            p.recipe = if star { STAR_SPRITE } else { SEAL_SPARK_SPRITE };
            p.field_fog = false;
            p.size = [self.size + super::stream::spread(rng, self.variation) as f32; 2];
            p.fade = Fade::Linear(-5.);
            if star {
                p.rotation[2] = 45.;
            }
            p.angular_velocity[2] = if random(rng).is_multiple_of(2) {
                -3.
            } else {
                3.
            };
            let mut direction = [1., 0., 0.];
            for axis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
                direction = super::rotated(direction, axis, (random(rng) % 360) as f32);
            }
            p.velocity = direction.map(|v| v * actor.movement_speed());
            out.push(p);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn aura(
    center: [f32; 3],
    born: u32,
    age: u32,
    camera: [f32; 3],
    color: i32,
    offset: f32,
    rng: &mut u32,
    out: &mut super::Births,
) {
    const GROWTH: f32 = 3.;
    const SPARK_THRESHOLD: f32 = 100.;
    random(rng);
    let mut glow = particle(center, born, palette(color, rng), 6);
    glow.size = [age as f32 * GROWTH; 2];
    glow.rgba[3] = 70;
    glow.fade = Fade::Linear(-5.);
    glow.controller = Some(crate::effect::BillboardController::CameraOffset {
        emitter: None,
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
    color: u16,
    radius: f32,
    spread: f32,
    rng: &mut u32,
    out: &mut super::Births,
) {
    const SPOKES: u32 = 72;
    let variation = spread as u32 / 2;
    for spoke in 1..=SPOKES {
        let size = [
            (8 + random(rng) % 5) as f32,
            (100 + random(rng) % 100) as f32,
        ];
        let (sin, cos) = (spoke as f32 * 360. / SPOKES as f32).to_radians().sin_cos();
        let direction = [
            cos * (spread + super::stream::spread(rng, variation) as f32),
            sin * (spread + super::stream::spread(rng, variation) as f32),
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

#[derive(Debug, Clone, Default)]
pub(crate) struct RisingOrbs {
    pub palette: i32,
    pub radius: f32,
    pub size: f32,
    pub variation: u32,
    pub speed_variation: u32,
    pub interval: u32,
    pub preserve_particles: bool,
    pub drifting: bool,
    pub alpha: u8,
    pub fade: f32,
    pub field_fog: bool,
    pub destination: Option<[f32; 3]>,
}
impl RisingOrbs {
    pub fn emit(
        &self,
        center: [f32; 3],
        born: u32,
        tick: u32,
        (owner, actor): (i32, &crate::Actor),
        rng: &mut u32,
        out: &mut super::Births,
    ) {
        random(rng);
        if !tick.is_multiple_of(self.interval) {
            return;
        }
        const MOTE_SPRITES: [u16; 3] = [
            crate::effect::ORB_SPRITE,
            crate::effect::TRAIL_GLOW_SPRITE,
            crate::effect::SEAL_SPARK_SPRITE,
        ];
        const WHITE_MOTE: usize = 2;
        let style = if self.drifting {
            random(rng) as usize % MOTE_SPRITES.len()
        } else {
            0
        };
        let color = if !self.drifting {
            self.palette as u16
        } else if self.palette >= 105 {
            palette(self.palette, rng)
        } else if style == WHITE_MOTE {
            u16::from(crate::effect::NEUTRAL_PALETTE)
        } else {
            self.palette as u16 + (random(rng) % 4) as u16
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
            super::stream::spread(rng, self.radius as u32) as f32
        } else {
            self.radius
        };
        let mut p = particle(center, born, color, 301);
        p.recipe = MOTE_SPRITES[style];
        p.field_fog = self.field_fog;
        p.rgba[3] = self.alpha;
        if self.destination.is_some() {
            p.owner = Some(owner);
        }
        p.position[0] += sin * radius;
        p.position[1] -= cos * radius;
        p.velocity[2] = rise;
        if self.drifting {
            p.controller = Some(crate::effect::BillboardController::Drift {
                direction: [0., 0., 100.],
                speed: rise,
                spatial: false,
            });
            p.angular_velocity[2] = spin;
            p.rotation[2] = spin;
            super::inherit(&mut p, actor);
        } else {
            p.owner = Some(owner);
        }
        p.size = [size; 2];
        if self.fade != 0. {
            p.fade = Fade::Linear(self.fade);
        }
        out.push(p);
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Bloom {
    pub palette: u16,
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
        out: &mut super::Births,
    ) {
        for _ in 0..self.count {
            let size =
                std::array::from_fn(|i| self.size[i] + (random(rng) % self.variation[i]) as f32);
            let angle = 90. - (random(rng) % 360) as f32;
            let (sin, cos) = angle.to_radians().sin_cos();
            let mut p = particle(center, born, self.palette, self.lifetime + 1);
            p.recipe = STREAK_SPRITE;
            p.orientation = SpriteOrientation::World;
            p.rotation = [90., angle, 0.];
            p.velocity = [sin * speed, 0., cos * speed];
            p.size = size;
            p.rgba[3] = 150;
            out.push(p);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Rising {
    pub palette: i32,
    pub radius: f32,
    pub size: [f32; 2],
    pub variation: [u32; 2],
    pub alpha: u8,
    pub rise: u32,
    pub interval: u32,
    pub lifetime: u32,
}
impl Rising {
    pub fn emit(
        &self,
        center: [f32; 3],
        born: u32,
        clock: u32,
        camera: [f32; 3],
        rng: &mut u32,
        out: &mut super::Births,
    ) {
        random(rng);
        if self.interval == 0 || !clock.is_multiple_of(self.interval) {
            return;
        }
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
        p.orientation = SpriteOrientation::World;
        p.rotation = [90., 0., camera[0].atan2(-camera[1]).to_degrees()];
        out.push(p);
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Shafts {
    pub palette: i32,
    pub interval: u32,
    pub radius: f32,
    pub size: [f32; 2],
    pub variation: [u32; 2],
    pub tilt: f32,
    pub cluster: u32,
}

impl Shafts {
    pub fn emit(&self, center: [f32; 3], born: u32, rng: &mut u32, out: &mut super::Births) {
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

#[derive(Debug, Clone, Default)]
pub(crate) struct Convergence {
    pub palette: i32,
    pub size: f32,
    pub growth: f32,
    pub radius: u32,
    pub spread: u32,
}

impl Convergence {
    pub fn emit(&self, center: [f32; 3], born: u32, rng: &mut u32, out: &mut super::Births) {
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
            let mut p = particle(center, born, color, 1);
            p.recipe = STREAK_SPRITE;
            p.orientation = SpriteOrientation::World;
            p.anchor = resonance_content::effect::VerticalAnchor::Bottom;
            p.position[0] -= sin * radius;
            p.position[2] -= cos * radius;
            let path: [f32; 3] = std::array::from_fn(|i| center[i] - p.position[i]);
            let distance = path.iter().map(|v| v * v).sum::<f32>().sqrt();
            p.lifetime = (distance / SPEED) as u32 + 1;
            p.velocity = super::normalized(path).map(|v| v * SPEED);
            p.rotation = [90., angle, 0.];
            p.size = size;
            p.rgba[3] = alpha;
            p.fade = Fade::Linear(0.);
            out.push(p);
        }
    }
}
