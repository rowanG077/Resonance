//! Mixed rising sprites for seal and angel scenes.
use crate::effect::{BillboardController, BillboardEffect, Fade};

parameters! { palette = 113, spread = 114, size = 115, size_variation = 116, lighting = 117, speed_variation = 118, fade = 119, lifetime = 120, interval = 121 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Drifting,
    Ascending,
}

#[derive(Debug, Clone)]
pub(crate) struct Rising {
    parameters: Parameters,
    kind: Kind,
    alpha: u8,
}
impl Rising {
    pub(super) fn from_native(a: &[i32], kind: Kind) -> Result<Self, String> {
        let result = Self {
            parameters: Parameters::read(a),
            kind,
            alpha: a[13].clamp(0, 255) as u8,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), String> {
        let p = self.parameters;
        if !(0..=108).contains(&p.palette)
            || p.spread <= 0
            || p.size_variation <= 0
            || p.speed_variation <= 0
            || p.interval <= 0
            || (self.kind == Kind::Ascending && !(0..=32767).contains(&p.lifetime))
        {
            return Err("invalid rising sprite palette, variation, interval or lifetime".into());
        }
        Ok(())
    }
    pub(super) fn interval(&self) -> u32 {
        self.parameters.interval as u32
    }
    pub(super) fn inherits_appearance(&self) -> bool {
        self.kind == Kind::Drifting
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        let previous = self.parameters.property(property, value)?;
        if let Some(value) = value {
            if property == 118 {
                self.alpha = value.clamp(0, 255) as u8;
            }
            self.validate()?;
        }
        Ok(previous)
    }
    pub(super) fn particle(
        &self,
        emitter: i32,
        center: [f32; 3],
        speed: f32,
        born: u32,
        random: &mut u32,
    ) -> BillboardEffect {
        let p = self.parameters;
        let kind = crate::world::random(random) % 3;
        let recipe = if self.kind == Kind::Drifting {
            [10, 12, 69][kind as usize]
        } else {
            [10, 7, 68][kind as usize]
        };
        let palette = if self.kind == Kind::Drifting {
            if p.palette >= 105 {
                [101, 85, 73, 89, 77, 97, 93][crate::world::random(random) as usize % 7]
                    + (p.palette - 105) as u16
            } else if kind == 2 {
                33
            } else {
                p.palette as u16 + (crate::world::random(random) % 4) as u16
            }
        } else {
            p.palette as u16
        };
        let lifetime = if self.kind == Kind::Drifting {
            301
        } else {
            p.lifetime as u32 + 1
        };
        let mut particle = super::particle(center, born, palette, lifetime);
        particle.recipe = recipe;
        particle.field_lighting = p.lighting & 1 != 0;
        let size = p.size as f32 + (crate::world::random(random) % p.size_variation as u32) as f32;
        particle.size = [size
            * if self.kind == Kind::Drifting && kind == 2 {
                1.2
            } else {
                1.
            }; 2];
        let rise =
            (speed + (crate::world::random(random) % p.speed_variation as u32) as f32) / 100.;
        let spin = if crate::world::random(random) & 1 != 0 {
            1.
        } else {
            -1.
        };
        particle.angular_velocity[2] = spin * if self.kind == Kind::Drifting { 1. } else { 2. };
        if recipe == 7 {
            particle.rotation[2] = 45.;
        }
        let (sin, cos) = ((crate::world::random(random) % 360) as f32)
            .to_radians()
            .sin_cos();
        let radius = if self.kind == Kind::Drifting {
            (crate::world::random(random) % p.spread as u32) as f32
        } else {
            p.spread as f32
        };
        particle.position[0] += sin * radius;
        particle.position[1] -= cos * radius;
        if self.kind == Kind::Drifting {
            particle.fade = Fade::Linear(-1.);
            particle.controller = Some(BillboardController::RisingWander {
                direction: [0., 0., 100.],
                speed: rise,
            });
            particle.advance(born, random);
        } else {
            particle.owner = Some(emitter);
            particle.velocity[2] = rise;
            particle.rgba[3] = self.alpha;
            particle.fade = if p.fade == 0 {
                Fade::Linear(0.)
            } else {
                Fade::tail(lifetime)
            };
        }
        particle
    }
}
