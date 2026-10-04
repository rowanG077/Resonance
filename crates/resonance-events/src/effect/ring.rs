//! Ring visual presets, independent of targeting and casting choreography.
use super::{
    BillboardEffect, Fade, RefractionImage, RefractionPulse, SpriteOrientation,
    emission::{Emission, sprite},
};
use crate::{
    GameWorld, Operation,
    ring::{LightningColor, SorcerersRing},
};

fn color(kind: SorcerersRing) -> [u8; 3] {
    use SorcerersRing::*;
    match kind {
        Fire | LongRangeFire => [255, 64, 10],
        Water => [10, 255, 255],
        Wind => [160, 255, 180],
        Mana => [200, 100, 255],
        Lightning(LightningColor::Yellow) => [255, 255, 32],
        Lightning(LightningColor::Red) => [255, 32, 32],
        Lightning(_) | Ice => [64, 128, 255],
        Darkness => [24, 0, 40],
        _ => [255; 3],
    }
}

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
    fn cloud(
        &mut self,
        particle: BillboardEffect,
        count: usize,
        spread: f32,
        speed: f32,
        random: &mut u32,
    ) {
        let mut births = Vec::new();
        Emission {
            particle,
            count,
            spread,
            speed,
            size_variation: 4.,
        }
        .emit(random, &mut births);
        for particle in births {
            self.add(particle);
        }
    }
    pub fn shot(&mut self, kind: SorcerersRing, age: i32, impact: bool, random: &mut u32) {
        if matches!(kind, SorcerersRing::Lightning(_)) {
            if !impact && age == 1 {
                let color = color(kind);
                for turn in [0., 90.] {
                    let mut bolt = sprite(super::ELECTRIC_SPARK_SPRITE, 76., 15);
                    bolt.size[1] = 500.;
                    bolt.orientation = SpriteOrientation::World;
                    bolt.anchor = resonance_content::effect::VerticalAnchor::LowerHalf;
                    bolt.rotation = [
                        0.,
                        turn,
                        self.velocity[0].atan2(-self.velocity[1]).to_degrees(),
                    ];
                    bolt.rgba = [color[0], color[1], color[2], 255];
                    bolt.palette = Some(2);
                    self.add(bolt);
                }
            }
            return;
        }
        if kind == SorcerersRing::Wind && impact {
            return;
        }
        let mut spark = sprite(
            super::STATION_GLOW_SPRITE,
            if impact { 12. } else { 30. },
            if impact { 30 } else { 12 },
        );
        let color = color(kind);
        spark.rgba = [color[0], color[1], color[2], 224];
        spark.palette = None;
        spark.size_delta = -1.;
        if matches!(kind, SorcerersRing::Mana | SorcerersRing::Ice) {
            spark.recipe = super::STAR_SPRITE;
        }
        if kind == SorcerersRing::Darkness {
            spark.blend_mode = Some(2);
        }
        if kind == SorcerersRing::Wind {
            spark.recipe = resonance_content::effect::STREAK_SPRITE;
            spark.size = [48., 12.];
            spark.rotation[2] = self.velocity[0].atan2(-self.velocity[1]).to_degrees();
            if age % 4 == 0 {
                self.ripple(false, RefractionImage::Air);
            }
        }
        if impact {
            spark.velocity = self.velocity.map(|v| {
                v * if kind == SorcerersRing::Water {
                    -0.15
                } else {
                    0.3
                }
            });
            spark.gravity = if kind == SorcerersRing::Water {
                -0.1
            } else {
                0.
            };
            self.cloud(spark, 20, 4., 2., random);
        } else {
            let spread = if matches!(kind, SorcerersRing::Mana | SorcerersRing::Darkness) {
                20.
            } else {
                6.
            };
            if kind == SorcerersRing::LongRangeFire {
                spark.size = [60.; 2];
            }
            self.cloud(spark, 2, spread, 0.5, random);
        }
    }
    pub fn electric(&mut self, flying: bool, random: &mut u32) {
        let mut orb = sprite(super::ORB_SPRITE, if flying { 60. } else { 90. }, 4);
        orb.palette = Some(1);
        orb.rgba[3] = 224;
        self.cloud(orb, 1, 0., 0., random);
        if self.tick.is_multiple_of(2) {
            let mut arc = sprite(super::ELECTRIC_ARC_SPRITE, 80., 6);
            arc.palette = Some(1);
            arc.rgba[3] = 224;
            arc.rotation[2] = crate::world::random_unit(random) * 360.;
            self.add(arc);
        }
    }
    pub fn bomb(&mut self, burst: bool, random: &mut u32) {
        if burst {
            let mut flame = sprite(super::FLAME_SPRITE, 50., 70);
            flame.palette = Some(24);
            flame.position[2] = 30.;
            flame.velocity[2] = 10.;
            flame.gravity = -0.3;
            flame.rgba[3] = 192;
            self.cloud(flame, 80, 10., 6., random);
        }
        let mut glow = sprite(
            super::STATION_GLOW_SPRITE,
            if burst { 100. } else { 60. },
            20,
        );
        glow.position[2] = 30.;
        glow.rgba = [255, 80, 16, 192];
        glow.size_delta = 5.;
        self.add(glow);
    }
    pub fn pulse(&mut self, color: [u8; 4]) {
        for image in [super::ORB_SPRITE, super::WORLD_GLOW_SPRITE] {
            let mut wave = sprite(image, 1., 30);
            wave.rgba = color;
            wave.size_delta = 20.;
            self.add(wave);
        }
        self.ripple(false, RefractionImage::Ripple);
    }
    pub fn ground_ring(&mut self, height: f32) {
        let mut ring = sprite(super::RING_SPRITE, 150., 21);
        ring.orientation = SpriteOrientation::World;
        ring.palette = Some(30);
        ring.position[2] = height;
        ring.rgba[3] = 192;
        self.add(ring);
    }
    pub fn ripple(&mut self, ground: bool, image: RefractionImage) {
        self.ripples.push(RefractionPulse {
            operation: Some(self.operation.clone()),
            owner: None,
            image,
            palette: 0,
            orientation: if ground {
                SpriteOrientation::World
            } else {
                SpriteOrientation::Camera
            },
            rotation: [0., 0., 90.],
            position: self.position,
            born: self.tick,
            lifetime: 40,
            size: 30.,
            growth: 15.,
            alpha: 160.,
            fade: Fade::tail(40),
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
