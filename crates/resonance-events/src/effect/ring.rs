//! Ring visual presets, independent of targeting and casting choreography.
use super::{
    BillboardEffect, Fade, RefractionImage, RefractionPulse, SpriteOrientation, emission::sprite,
};
use crate::{
    GameWorld, Operation,
    ring::{LightningColor, SorcerersRing},
};

fn variation(random: &mut u32, choices: u32) -> f32 {
    (crate::world::random(random) % choices) as f32
}

const PROJECTILE_GLOW_UV: [f32; 4] = [0., 64. / 256., 64. / 256., 128. / 256.];

pub(crate) struct Visuals {
    position: [f32; 3],
    velocity: [f32; 3],
    operation: Operation,
    tick: u32,
    pub particles: Vec<BillboardEffect>,
    pub ripples: Vec<RefractionPulse>,
}
impl Visuals {
    pub fn new(shot: &crate::projectile::Shot, operation: &Operation, tick: u32) -> Self {
        Self {
            position: shot.position,
            velocity: shot.velocity,
            operation: operation.clone(),
            tick,
            particles: Vec::new(),
            ripples: Vec::new(),
        }
    }
    fn add(&mut self, mut particle: BillboardEffect) {
        particle.position = std::array::from_fn(|i| self.position[i] + particle.position[i]);
        particle.operation = Some(self.operation.clone());
        particle.born = self.tick;
        self.particles.push(particle);
    }
    pub fn shot(&mut self, kind: SorcerersRing, age: i32, impact: bool, random: &mut u32) {
        use SorcerersRing::*;
        match kind {
            Fire | Water | LongRangeFire | Ice => self.projectile(kind, age, impact, random),
            Lightning(color) if !impact && age == 1 => {
                let color = match color {
                    LightningColor::Blue => [32, 32, 255],
                    LightningColor::Yellow => [255, 255, 32],
                    LightningColor::Red => [255, 32, 32],
                };
                for (turn, rgb) in [(0., [64; 3]), (0., color), (90., color)] {
                    let mut bolt = sprite(super::ELECTRIC_SPARK_SPRITE, 76., 15);
                    bolt.size[1] = 500.;
                    bolt.orientation = SpriteOrientation::World;
                    bolt.anchor = resonance_content::effect::VerticalAnchor::Top;
                    bolt.rotation = [0., turn + 4.2, self.heading()];
                    bolt.angular_velocity[1] = 4.2;
                    bolt.rgba = [rgb[0], rgb[1], rgb[2], 247];
                    self.add(bolt);
                }
            }
            Wind if !impact => self.wind(age, random),
            Mana | Darkness => self.mist(kind == Darkness, age, impact, random),
            _ => {}
        }
    }
    fn heading(&self) -> f32 {
        self.velocity[0].atan2(-self.velocity[1]).to_degrees()
    }
    fn projectile(&mut self, kind: SorcerersRing, age: i32, impact: bool, random: &mut u32) {
        let water = kind == SorcerersRing::Water;
        let ice = kind == SorcerersRing::Ice;
        let large = ice || kind == SorcerersRing::LongRangeFire;
        let outer = if water {
            [0, 255, 255]
        } else if ice {
            [10, 10, 255]
        } else {
            [255, 10, 10]
        };
        let inner = if water {
            outer
        } else if ice {
            [255; 3]
        } else {
            [255, 255, 10]
        };
        if impact && ice {
            self.ice_burst(random);
            return;
        }
        if !impact {
            if large {
                self.helix(ice, age, random);
            }
            let sizes = if large {
                [(62., 9, -6., outer), (33., 5, -5., inner)]
            } else {
                [(29., 9, -3., outer), (13., 5, -2., inner)]
            };
            for (size, lifetime, shrink, rgb) in sizes {
                let mut glow = sprite(super::GLOW_SPRITE, size + variation(random, 16), lifetime);
                glow.uv = Some(PROJECTILE_GLOW_UV);
                glow.rgba = [rgb[0], rgb[1], rgb[2], 247];
                glow.blend = Some(crate::effect::Blend::Additive);
                glow.rotation[2] = variation(random, 256) - 3.;
                glow.angular_velocity[2] = -3.;
                glow.size_delta = shrink;
                self.add(glow);
            }
        }
        for _ in 0..if impact { 16 } else { 2 } {
            let size = if impact {
                if water { 16. } else { 4. }
            } else if large {
                4.
            } else {
                1.
            };
            let mut spark = sprite(
                super::GLOW_SPRITE,
                size + variation(random, 4),
                if impact { 21 } else { 31 },
            );
            spark.rgba = [inner[0], inner[1], inner[2], 247];
            spark.uv = Some(PROJECTILE_GLOW_UV);
            spark.blend = Some(crate::effect::Blend::Additive);
            spark.velocity = if impact {
                std::array::from_fn(|i| {
                    let spread = if i == 2 {
                        if large { 4. } else { 1. }
                    } else {
                        2.
                    };
                    let bias = if water { -1. / 6. } else { 0.5 };
                    (crate::world::random_unit(random) * 2. - 1.) * spread + self.velocity[i] * bias
                })
            } else {
                [
                    crate::world::random_unit(random) - 0.25,
                    crate::world::random_unit(random) - 0.25,
                    -1. - crate::world::random_unit(random) * 0.5,
                ]
            };
            spark.position = spark.velocity;
            self.add(spark);
        }
    }
    fn helix(&mut self, ice: bool, age: i32, random: &mut u32) {
        const SAMPLES: i32 = 8;
        const RADIUS: f32 = 25.;
        const TURN_PER_SAMPLE: f32 = 8.;
        let (sin, cos) = self.heading().to_radians().sin_cos();
        for sample in 0..SAMPLES {
            let angle = -(((age - 1) * SAMPLES + sample + 1) as f32 * TURN_PER_SAMPLE).to_radians();
            let mut dot = sprite(super::GLOW_SPRITE, 8. + variation(random, 16), 16);
            dot.uv = Some(PROJECTILE_GLOW_UV);
            dot.blend = Some(crate::effect::Blend::Additive);
            dot.position =
                std::array::from_fn(|i| self.velocity[i] * sample as f32 / SAMPLES as f32);
            dot.position[0] += RADIUS * angle.cos() * cos;
            dot.position[1] += RADIUS * angle.cos() * sin;
            dot.position[2] += RADIUS * angle.sin();
            dot.rgba = if ice {
                [10, 10, 80, 238]
            } else {
                [252, 20, 20, 238]
            };
            dot.fade = Fade::Linear(-17.);
            dot.rotation[2] = variation(random, 256) - 3.;
            dot.angular_velocity[2] = 3.;
            self.add(dot);
        }
        if ice {
            let mut star = sprite(super::STAR_SPRITE, 30., 3);
            star.rgba = [255, 255, 255, 184];
            star.rotation[2] = 90.;
            self.add(star);
        }
    }
    fn ice_burst(&mut self, random: &mut u32) {
        for index in 0..16 {
            let mut star = sprite(super::STAR_SPRITE, 16. + variation(random, 4), 61);
            star.fade = Fade::tail(60);
            star.position = std::array::from_fn(|_| variation(random, 32) - 16.);
            star.velocity[2] = -crate::world::random_unit(random) * 2.;
            star.position[2] += star.velocity[2];
            star.rgba = if index < 8 {
                [64, 64, 255, 192]
            } else {
                [255, 255, 255, 192]
            };
            star.rotation[2] = variation(random, 256);
            star.angular_velocity[2] = 3.;
            self.add(star);
        }
        let mut orb = sprite(super::ORB_SPRITE, 64., 61);
        orb.rgba = [64, 64, 255, 250];
        orb.fade = Fade::Linear(-255. / 60.);
        self.add(orb);
    }
    fn mist(&mut self, dark: bool, age: i32, impact: bool, random: &mut u32) {
        if age % 4 != 0 && !impact {
            return;
        }
        let lifetime = if dark { 120 } else { 60 };
        if !impact || dark {
            for _ in 0..4 {
                let size = age as f32 * 2.;
                let mut dot = sprite(super::GLOW_SPRITE, size, lifetime + 1);
                dot.blend = Some(crate::effect::Blend::Alpha);
                dot.rgba = if dark {
                    [0, 0, 0, 252]
                } else {
                    [
                        20 + variation(random, 64) as u8,
                        20 + variation(random, 64) as u8,
                        20 + variation(random, 64) as u8,
                        126,
                    ]
                };
                dot.fade = Fade::Linear(if dark { -255. / 120. } else { -2. });
                dot.position = std::array::from_fn(|_| variation(random, 32) - 16.);
                dot.velocity[2] = -0.2 - crate::world::random_unit(random) * 0.5;
                dot.position[2] += dot.velocity[2];
                dot.size_delta = -size / (lifetime as f32 * 16.);
                dot.size = [size + dot.size_delta; 2];
                dot.angular_velocity[2] = (variation(random, 17) - 8.) / 8.;
                dot.rotation[2] = dot.angular_velocity[2];
                self.add(dot);
            }
        }
        if !dark {
            for _ in 0..4 {
                let mut star = sprite(
                    super::STAR_SPRITE,
                    4. + variation(random, 16),
                    if impact { 61 } else { 21 },
                );
                star.position = std::array::from_fn(|_| variation(random, 16) - 8.);
                star.rgba[3] = if impact { 255 } else { 247 };
                star.angular_velocity[2] = variation(random, 16) - 8.;
                star.rotation[2] = star.angular_velocity[2];
                if impact {
                    star.fade = Fade::tail(60);
                    star.velocity[2] = -0.2 - crate::world::random_unit(random) * 0.5;
                }
                self.add(star);
            }
        }
    }
    fn wind(&mut self, age: i32, random: &mut u32) {
        for _ in 0..2 {
            let mut streak = sprite(resonance_content::effect::STREAK_SPRITE, 64., 31);
            streak.size[1] = 4.;
            streak.orientation = SpriteOrientation::World;
            streak.rgba = [64, 64, 64, 59];
            streak.fade = Fade::Linear(-64. / 15.);
            streak.rotation[2] = 90. + self.heading();
            streak.position = std::array::from_fn(|_| variation(random, 64) - 32.);
            streak.velocity = self.velocity.map(|v| -v * 0.5);
            for i in 0..3 {
                streak.position[i] += streak.velocity[i];
            }
            self.add(streak);
        }
        if age % 4 == 0 {
            self.ripples.push(RefractionPulse {
                operation: Some(self.operation.clone()),
                owner: None,
                image: RefractionImage::Air,
                palette: 0,
                orientation: SpriteOrientation::Camera,
                rotation: [0., 0., 90.],
                position: self.position,
                born: self.tick,
                lifetime: 15,
                size: 96.,
                growth: 0.,
                alpha: 120.,
                fade: Fade::Linear(-8.),
            });
        }
        if age == 5 {
            for rgba in [[64, 64, 64, 27], [64, 255, 64, 55]] {
                let mut wave = sprite(super::WORLD_GLOW_SPRITE, 80., 8);
                wave.rgba = rgba;
                wave.orientation = SpriteOrientation::World;
                wave.rotation[0] = 90.;
                wave.size_delta = 32.;
                wave.fade = Fade::Linear(if rgba[3] == 27 { -32. / 7. } else { -64. / 7. });
                self.add(wave);
            }
        }
    }
    pub fn electric(&mut self, flying: bool, age: u32, random: &mut u32) {
        let neutral = [64, 64, 64];
        let blue = [1, 1, 63];
        let layers: &[_] = if flying {
            &[
                (true, super::ORB_SPRITE, 37., 16, 2, neutral, 247),
                (true, super::ORB_SPRITE, 77., 16, 4, blue, 216),
                (
                    age % 2 == 1,
                    super::ELECTRIC_ARC_SPRITE,
                    29.,
                    32,
                    5,
                    blue,
                    247,
                ),
            ]
        } else {
            &[
                (
                    age.is_multiple_of(4),
                    super::ORB_SPRITE,
                    77.,
                    16,
                    3,
                    neutral,
                    216,
                ),
                (
                    age.is_multiple_of(2),
                    super::ELECTRIC_ARC_SPRITE,
                    97.,
                    32,
                    9,
                    blue,
                    216,
                ),
            ]
        };
        for &(emit, recipe, size, jitter, lifetime, rgb, alpha) in layers {
            if !emit {
                continue;
            }
            let mut particle = sprite(recipe, size + variation(random, jitter), lifetime);
            particle.rgba = [rgb[0], rgb[1], rgb[2], alpha];
            particle.size_delta = -3.;
            particle.rotation[2] = variation(random, 256) - 3.;
            particle.angular_velocity[2] = -3.;
            self.add(particle);
        }
    }

    pub fn bomb(&mut self, burst: bool, random: &mut u32) {
        const GRAVITY: f32 = -0.98;
        const HEIGHT: f32 = 30.;
        if burst {
            for _ in 0..128 {
                let mut flame = sprite(super::FLAME_SPRITE, 50., 71);
                flame.palette = Some(24 + variation(random, 7) as u16);
                flame.fade = Fade::tail(70);
                flame.rgba[3] = 192;
                flame.velocity = [
                    (variation(random, 64) - 32.) / 4.,
                    (variation(random, 64) - 32.) / 4.,
                    10. + variation(random, 64) / 4.,
                ];
                flame.position = flame.velocity;
                flame.position[2] += HEIGHT;
                flame.gravity = GRAVITY;
                flame.velocity[2] += GRAVITY;
                flame.angular_velocity[2] = variation(random, 16) - 8.;
                flame.rotation[2] = flame.angular_velocity[2];
                self.add(flame);
            }
            for rgba in [[64, 64, 64, 184], [63, 1, 1, 120]] {
                let mut pulse = sprite(super::WORLD_GLOW_SPRITE, 6., 31);
                pulse.uv = Some([128. / 256., 64. / 256., 191. / 256., 127. / 256.]);
                pulse.rgba = rgba;
                pulse.position[2] = HEIGHT;
                pulse.gravity = GRAVITY;
                pulse.velocity[2] = GRAVITY;
                pulse.size_delta = 5.;
                self.add(pulse);
            }
        }
        for (size, spread, rgb) in [(200., 64, [255, 40, 40]), (40., 32, [255, 255, 40])] {
            let mut glow = sprite(
                super::GLOW_SPRITE,
                size + variation(random, spread),
                2 + variation(random, 16) as u32,
            );
            glow.blend = Some(crate::effect::Blend::Additive);
            glow.position[2] = HEIGHT;
            glow.rgba = [rgb[0], rgb[1], rgb[2], 184];
            glow.size_delta = if size == 200. {
                variation(random, 8)
            } else {
                6.
            };
            glow.size = glow.size.map(|v| v + glow.size_delta);
            if size == 200. {
                glow.angular_velocity[2] = variation(random, 8) - 3.;
                glow.rotation[2] = glow.angular_velocity[2];
            }
            self.add(glow);
        }
    }
    pub fn pulse(&mut self, color: Option<[u8; 3]>) {
        for (image, rgb) in [
            (super::ORB_SPRITE, color.unwrap_or([255, 64, 64])),
            (super::WORLD_GLOW_SPRITE, color.unwrap_or([255; 3])),
        ] {
            let mut wave = sprite(image, 21., 31);
            wave.rgba = [rgb[0], rgb[1], rgb[2], 200];
            wave.blend = Some(if image == super::WORLD_GLOW_SPRITE {
                super::Blend::Additive
            } else {
                super::Blend::Alpha
            });
            wave.size_delta = 20.;
            self.add(wave);
        }
        self.sound_ripple();
    }
    pub fn ground_ring(&mut self, height: f32) {
        let mut ring = sprite(super::RING_SPRITE, 150., 21);
        ring.orientation = SpriteOrientation::World;
        ring.position[2] = height;
        ring.rgba = [235, 255, 64, (247. - height) as u8];
        self.add(ring);
    }
    fn sound_ripple(&mut self) {
        self.ripples.push(RefractionPulse {
            operation: Some(self.operation.clone()),
            owner: None,
            image: RefractionImage::Ripple,
            palette: 0,
            orientation: SpriteOrientation::Camera,
            rotation: [0., 0., 90.],
            position: self.position,
            born: self.tick,
            lifetime: 60,
            size: 11.,
            growth: 10.,
            alpha: 128.,
            fade: Fade::tail(60),
        });
    }
    pub fn publish(self, world: &mut GameWorld) -> Result<(), String> {
        for particle in self.particles {
            world.emit_billboard(particle)?;
        }
        for ripple in self.ripples {
            world.emit_refraction(ripple)?;
        }
        Ok(())
    }
}
