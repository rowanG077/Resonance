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
const PULSE_INTERVAL: u32 = 4;

enum Visual {
    Sprite(BillboardEffect),
    Ripple(RefractionPulse),
}

pub(crate) struct Visuals {
    position: [f32; 3],
    velocity: [f32; 3],
    operation: Operation,
    tick: u32,
    clock: u32,
    effects: Vec<Visual>,
}
impl Visuals {
    pub fn new(shot: &crate::projectile::Shot, operation: &Operation, world: &GameWorld) -> Self {
        Self {
            position: shot.position,
            velocity: shot.velocity,
            operation: operation.clone(),
            tick: world.tick,
            clock: world.effect_tick,
            effects: Vec::new(),
        }
    }
    fn spin(&self) -> f32 {
        if self.clock & 1 == 0 { -3. } else { 3. }
    }
    fn add(&mut self, mut particle: BillboardEffect) {
        particle.position = std::array::from_fn(|i| self.position[i] + particle.position[i]);
        particle.operation = Some(self.operation.clone());
        particle.born = self.tick;
        self.effects.push(Visual::Sprite(particle));
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
                    let mut bolt = sprite(super::ELECTRIC_SPARK_SPRITE, 76., 16);
                    bolt.texture_phase = 0;
                    bolt.size[1] = 500.;
                    bolt.orientation = SpriteOrientation::World;
                    bolt.anchor = resonance_content::effect::VerticalAnchor::Top;
                    bolt.rotation = [0., turn, self.heading()];
                    bolt.angular_velocity[1] = 4.2;
                    bolt.rgba = [rgb[0], rgb[1], rgb[2], 255];
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
                [(68., 9, -6., outer), (38., 5, -5., inner)]
            } else {
                [(30., 9, -3., outer), (15., 5, -2., inner)]
            };
            for (size, lifetime, shrink, rgb) in sizes {
                let mut glow = sprite(super::GLOW_SPRITE, size + variation(random, 16), lifetime);
                glow.uv = Some(PROJECTILE_GLOW_UV);
                glow.rgba = [rgb[0], rgb[1], rgb[2], 255];
                glow.blend = Some(crate::effect::Blend::Additive);
                glow.rotation[2] = variation(random, 256);
                glow.angular_velocity[2] = self.spin();
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
            spark.rgba = [inner[0], inner[1], inner[2], 255];
            spark.uv = Some(PROJECTILE_GLOW_UV);
            spark.blend = Some(crate::effect::Blend::Additive);
            spark.velocity = if impact {
                std::array::from_fn(|i| {
                    let divisor = if i == 2 {
                        if large { 8. } else { 32. }
                    } else {
                        16.
                    };
                    let bias = if water { -1. / 6. } else { 0.5 };
                    (variation(random, 64) - 31.) / divisor + self.velocity[i] * bias
                })
            } else {
                [
                    (variation(random, 32) - 7.) / 32.,
                    (variation(random, 32) - 7.) / 32.,
                    -1. - variation(random, 16) / 32.,
                ]
            };
            self.add(spark);
        }
        if ice && !impact {
            let mut star = sprite(super::STAR_SPRITE, 30., 3);
            star.rgba = [255, 255, 255, 192];
            star.rotation[2] = 90.;
            self.add(star);
        }
    }
    fn helix(&mut self, ice: bool, age: i32, random: &mut u32) {
        const SAMPLES: i32 = 8;
        const RADIUS: f32 = 25.;
        const TURN_PER_SAMPLE: f32 = 8.;
        for sample in 0..SAMPLES {
            let angle = -(((age - 1) * SAMPLES + sample + 1) as f32 * TURN_PER_SAMPLE).to_radians();
            let mut dot = sprite(super::GLOW_SPRITE, 8. + variation(random, 16), 16);
            dot.uv = Some(PROJECTILE_GLOW_UV);
            dot.blend = Some(crate::effect::Blend::Additive);
            dot.position =
                std::array::from_fn(|i| self.velocity[i] * sample as f32 / SAMPLES as f32);
            dot.position[0] += RADIUS * angle.cos();
            dot.position[2] += RADIUS * angle.sin();
            dot.rgba = if ice {
                [10, 10, 80, 255]
            } else {
                [252, 20, 20, 255]
            };
            dot.fade = Fade::Linear(-17.);
            dot.rotation[2] = variation(random, 256);
            dot.angular_velocity[2] = self.spin();
            self.add(dot);
        }
    }
    fn ice_burst(&mut self, random: &mut u32) {
        for index in 0..16 {
            let position = std::array::from_fn(|_| variation(random, 64) - 31.);
            let phase = variation(random, 256);
            let spin = if variation(random, 2) == 1. { 3. } else { -3. };
            let mut star = sprite(super::STAR_SPRITE, 16. + variation(random, 4), 61);
            star.position = position;
            star.velocity[2] = -phase / 128.;
            star.rgba = if index % 2 == 0 {
                [64, 64, 255, 192]
            } else {
                [255, 255, 255, 192]
            };
            star.rotation[2] = phase;
            star.angular_velocity[2] = spin;
            self.add(star);
        }
        let mut orb = sprite(super::ORB_SPRITE, 64., 61);
        orb.rgba = [64, 64, 255, 255];
        orb.fade = Fade::Linear(-255. / 60.);
        self.add(orb);
    }
    fn mist(&mut self, dark: bool, age: i32, impact: bool, random: &mut u32) {
        if !self.clock.is_multiple_of(PULSE_INTERVAL) && !impact {
            return;
        }
        let lifetime = match (dark, impact) {
            (true, true) => 240,
            (false, false) => 60,
            _ => 120,
        };
        for _ in 0..4 {
            let size = age as f32 * 2.;
            let mut dot = sprite(super::GLOW_SPRITE, size, lifetime + 1);
            dot.blend = Some(crate::effect::Blend::Alpha);
            dot.position = std::array::from_fn(|_| variation(random, 32) - 15.);
            dot.angular_velocity[2] = (variation(random, 16) - 7.) / 8.;
            if !impact {
                dot.velocity[2] = -0.2 - variation(random, 16) / 32.;
            }
            dot.rgba = if dark {
                [0, 0, 0, 255]
            } else {
                [
                    22 + variation(random, 64) as u8,
                    22 + variation(random, 64) as u8,
                    22 + variation(random, 64) as u8,
                    128,
                ]
            };
            dot.fade = Fade::Linear(match (dark, impact) {
                (true, true) => -1.,
                (true, false) => -255. / 120.,
                _ => -2.,
            });
            dot.size_delta = -size / (lifetime as f32 * 16.);
            self.add(dot);
        }
        if !dark {
            for _ in 0..4 {
                let position =
                    std::array::from_fn(|_| variation(random, age as u32 * 2) - age as f32);
                let fall = if impact {
                    -0.2 - variation(random, 16) / 32.
                } else {
                    0.
                };
                let rotation = variation(random, 16) - 7.;
                let mut star = sprite(
                    super::STAR_SPRITE,
                    4. + variation(random, 16),
                    if impact { 61 } else { 21 },
                );
                star.position = position;
                star.velocity[2] = fall;
                star.rgba[3] = 255;
                star.rotation[2] = rotation;
                self.add(star);
            }
        }
    }
    fn wind(&mut self, age: i32, random: &mut u32) {
        if self.clock.is_multiple_of(PULSE_INTERVAL) {
            self.effects.push(Visual::Ripple(RefractionPulse {
                draw_order: 0,
                operation: Some(self.operation.clone()),
                owner: None,
                image: RefractionImage::Air,
                palette: 0,
                orientation: SpriteOrientation::Camera,
                rotation: [0., 0., 90.],
                position: self.position,
                velocity: [0.; 3],
                born: self.tick,
                lifetime: 16,
                size: 96.,
                growth: 0.,
                alpha: 128.,
                fade: Fade::Linear(-8.),
            }));
        }
        if age == 5 {
            for rgba in [[64, 64, 64, 32], [64, 255, 64, 64]] {
                let mut wave = sprite(super::WORLD_GLOW_SPRITE, 48., 8);
                wave.rgba = rgba;
                wave.orientation = SpriteOrientation::World;
                wave.rotation = [90., 0., self.heading()];
                wave.size_delta = 32.;
                wave.fade = Fade::Linear(-(rgba[3] as f32) / 7.);
                self.add(wave);
            }
        }
        for _ in 0..2 {
            let mut streak = sprite(resonance_content::effect::sprite::STREAK_SPRITE, 64., 31);
            streak.size[1] = 4.;
            streak.orientation = SpriteOrientation::World;
            streak.rgba = [64, 64, 64, 64];
            streak.fade = Fade::Linear(-64. / 15.);
            streak.rotation[2] = 90. + self.heading();
            streak.position = std::array::from_fn(|_| variation(random, 64) - 31.);
            streak.velocity = self.velocity.map(|v| -v * 0.5);
            self.add(streak);
        }
        // Each gust also advances the shared visual random stream.
        crate::world::random(random);
    }
    pub fn electric(&mut self, flying: bool, age: u32, random: &mut u32) {
        let neutral = [64, 64, 64];
        let blue = [1, 1, 63];
        let layers: &[_] = if flying {
            &[
                (true, super::ORB_SPRITE, 40., 16, 2, neutral, 255),
                (true, super::ORB_SPRITE, 80., 16, 4, blue, 224),
                (
                    age % 2 == 1,
                    super::ELECTRIC_ARC_SPRITE,
                    30.,
                    32,
                    5,
                    blue,
                    255,
                ),
            ]
        } else {
            &[
                (
                    self.clock.is_multiple_of(PULSE_INTERVAL),
                    super::ORB_SPRITE,
                    80.,
                    16,
                    3,
                    neutral,
                    224,
                ),
                (
                    age.is_multiple_of(2),
                    super::ELECTRIC_ARC_SPRITE,
                    100.,
                    32,
                    9,
                    blue,
                    224,
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
            particle.rotation[2] = variation(random, 256);
            particle.angular_velocity[2] = if flying && recipe == super::ORB_SPRITE && rgb == blue {
                0.
            } else {
                self.spin()
            };
            self.add(particle);
        }
    }

    pub fn bomb(&mut self, burst: bool, random: &mut u32) {
        const GRAVITY: f32 = -0.98;
        const HEIGHT: f32 = 30.;
        const FLAME_TICKS: u32 = 71;
        if burst {
            for _ in 0..128 {
                let mut flame = sprite(super::FLAME_SPRITE, 50., FLAME_TICKS);
                flame.palette = Some(24 + variation(random, 7) as u16);
                flame.rgba[3] = 192;
                flame.velocity = [
                    (variation(random, 64) - 31.) / 4.,
                    (variation(random, 64) - 31.) / 4.,
                    10. + variation(random, 64) / 4.,
                ];
                flame.position[2] = HEIGHT;
                flame.gravity = GRAVITY;
                flame.angular_velocity[2] = variation(random, 16) - 7.;
                self.add(flame);
            }
            for rgba in [[64, 64, 64, 192], [63, 1, 1, 128]] {
                let mut pulse = sprite(super::WORLD_GLOW_SPRITE, 1., 31);
                pulse.uv = Some([128. / 256., 64. / 256., 191. / 256., 127. / 256.]);
                pulse.rgba = rgba;
                pulse.position[2] = HEIGHT;
                pulse.gravity = GRAVITY;
                pulse.size_delta = 5.;
                self.add(pulse);
            }
        }
        for (size, spread, rgb, flickers) in [
            (200., 64, [255, 40, 40], true),
            (40., 32, [255, 255, 40], false),
        ] {
            let lifetime = 2 + variation(random, 16) as u32;
            let (growth, spin) = if flickers {
                (variation(random, 8), variation(random, 8) - 3.)
            } else {
                (6., 0.)
            };
            let mut glow = sprite(
                super::GLOW_SPRITE,
                size + variation(random, spread),
                lifetime,
            );
            glow.blend = Some(crate::effect::Blend::Additive);
            glow.position[2] = HEIGHT;
            glow.rgba = [rgb[0], rgb[1], rgb[2], 192];
            glow.size_delta = growth;
            glow.angular_velocity[2] = spin;
            self.add(glow);
        }
    }
    pub fn pulse(&mut self, color: Option<[u8; 3]>) {
        self.sound_ripple();
        for (image, rgb) in [
            (super::ORB_SPRITE, color.unwrap_or([255, 64, 64])),
            (super::WORLD_GLOW_SPRITE, color.unwrap_or([255; 3])),
        ] {
            let mut wave = sprite(image, 1., 31);
            wave.rgba = [rgb[0], rgb[1], rgb[2], 208];
            wave.blend = Some(if image == super::WORLD_GLOW_SPRITE {
                super::Blend::Additive
            } else {
                super::Blend::Alpha
            });
            wave.size_delta = 20.;
            self.add(wave);
        }
    }
    pub fn ground_ring(&mut self, height: f32) {
        let mut ring = sprite(super::RING_SPRITE, 150., 21);
        ring.orientation = SpriteOrientation::World;
        ring.position[2] = height;
        ring.rgba = [235, 255, 64, (255. - height) as u8];
        self.add(ring);
    }
    pub fn ground_ripple(&mut self) {
        self.effects.push(Visual::Ripple(RefractionPulse {
            draw_order: 0,
            operation: Some(self.operation.clone()),
            owner: None,
            image: RefractionImage::Ripple,
            palette: 0,
            orientation: SpriteOrientation::World,
            rotation: [0., 0., 90.],
            position: [self.position[0], self.position[1], 4.],
            velocity: [0.; 3],
            born: self.tick,
            lifetime: 41,
            size: 30.,
            growth: 20.,
            alpha: 192.,
            fade: Fade::tail(41),
        }));
    }
    fn sound_ripple(&mut self) {
        const LIFETIME: u32 = 61;
        self.effects.push(Visual::Ripple(RefractionPulse {
            draw_order: 0,
            operation: Some(self.operation.clone()),
            owner: None,
            image: RefractionImage::Ripple,
            palette: 0,
            orientation: SpriteOrientation::Camera,
            rotation: [0., 0., 90.],
            position: self.position,
            velocity: [0.; 3],
            born: self.tick,
            lifetime: LIFETIME,
            size: 1.,
            growth: 10.,
            alpha: 128.,
            fade: Fade::tail(LIFETIME),
        }));
    }
    pub fn publish(self, world: &mut GameWorld) -> Result<(), String> {
        for effect in self.effects {
            match effect {
                Visual::Sprite(particle) => world.emit_billboard(particle)?,
                Visual::Ripple(ripple) => world.emit_refraction(ripple)?,
            };
        }
        Ok(())
    }
}
