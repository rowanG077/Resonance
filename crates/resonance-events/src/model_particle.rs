//! Independent model particles, with no gameplay actor identity.
const POOL_CAPACITY: usize = 512;
const PERCENT: f32 = 100.;
const GRAVITY: f32 = 0.98;

use crate::effect::Blend;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Motion {
    Velocity,
    Direction,
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum Fade {
    Tail,
    Linear,
    Proportional,
}

#[derive(Debug, Clone)]
pub struct ModelParticle {
    pub(crate) operation: Option<crate::Operation>,
    pub resource: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub orientation: crate::effect::SpriteOrientation,
    pub scale: [f32; 3],
    pub rgba: [u8; 4],
    pub blend: Blend,
    pub born: u32,
    remaining: Option<u32>,
    velocity: [f32; 3],
    angular_velocity: [f32; 3],
    scale_delta: [f32; 3],
    motion: Motion,
    speed: f32,
    gravity: f32,
    fade: Fade,
    alpha_delta: i32,
}
impl ModelParticle {
    pub fn new(resource: u32) -> Self {
        Self {
            operation: None,
            resource,
            position: [0.; 3],
            rotation: [0.; 3],
            orientation: crate::effect::SpriteOrientation::World,
            scale: [1.; 3],
            rgba: [
                super::effect::NEUTRAL_TINT,
                super::effect::NEUTRAL_TINT,
                super::effect::NEUTRAL_TINT,
                255,
            ],
            blend: Blend::Alpha,
            born: 0,
            remaining: None,
            velocity: [0.; 3],
            angular_velocity: [0.; 3],
            scale_delta: [0.; 3],
            motion: Motion::Velocity,
            speed: 0.,
            gravity: 0.,
            fade: Fade::Tail,
            alpha_delta: 0,
        }
    }
    pub(crate) fn afterimage(resource: u32, position: [f32; 3], heading: f32) -> Self {
        Self {
            position,
            rotation: [0., -90., heading],
            scale: [0.5, 0.5, 1.5],
            rgba: [16, 63, 63, 200],
            blend: Blend::Additive,
            remaining: Some(30),
            scale_delta: [0.01, 0.01, 0.],
            ..Self::new(resource)
        }
    }
    pub(crate) fn scoped(resource: u32, operation: crate::Operation) -> Self {
        Self {
            operation: Some(operation),
            ..Self::new(resource)
        }
    }
    pub(crate) fn from_native(resource: u32, a: &[i32]) -> Self {
        let alpha = a[9].clamp(0, 255);
        Self {
            position: std::array::from_fn(|i| a[2 + i] as f32),
            rotation: std::array::from_fn(|i| a[5 + i] as f32),
            scale: [a[8] as f32 / PERCENT; 3],
            rgba: [
                super::effect::NEUTRAL_TINT,
                super::effect::NEUTRAL_TINT,
                super::effect::NEUTRAL_TINT,
                alpha as u8,
            ],
            remaining: (a[1] != i32::from(i16::MAX)).then_some(a[1].max(0) as u32),
            alpha_delta: a[10],
            ..Self::new(resource)
        }
    }
    fn step(&mut self, tick: u32) -> bool {
        if let Some(operation) = &self.operation {
            return operation.is_pending();
        }
        // Present the birth pose before advancing motion or opacity.
        if tick <= self.born {
            return true;
        }
        if self.remaining == Some(0) {
            return false;
        }
        let alpha = i32::from(self.rgba[3]);
        let alpha = match self.fade {
            Fade::Linear => alpha.saturating_add(self.alpha_delta).min(255),
            Fade::Proportional => self
                .remaining
                .filter(|ticks| *ticks > 1)
                .map_or(alpha, |ticks| alpha - (alpha as u32 / ticks) as i32),
            Fade::Tail
                if self
                    .remaining
                    .is_some_and(|ticks| ticks < crate::effect::Fade::TAIL_UPDATES) =>
            {
                crate::effect::Fade::tail_alpha(alpha as f32, 1) as i32
            }
            Fade::Tail => alpha,
        };
        if alpha < 0 {
            return false;
        }
        self.rgba[3] = alpha as u8;
        if let Some(ticks) = &mut self.remaining {
            *ticks -= 1;
        }
        let factor = match self.motion {
            Motion::Velocity => 1.,
            Motion::Direction => {
                let length = self.velocity.iter().map(|v| v * v).sum::<f32>().sqrt();
                if length > 0. { self.speed / length } else { 0. }
            }
        };
        for axis in 0..3 {
            self.position[axis] += self.velocity[axis] * factor;
            self.rotation[axis] += self.angular_velocity[axis];
            self.scale[axis] += self.scale_delta[axis];
        }
        self.velocity[2] += self.gravity;
        self.scale.iter().all(|scale| *scale >= 0.)
    }
}
#[derive(Debug, Clone)]
pub(crate) enum Property {
    Position(usize, f32),
    Velocity(usize, f32),
    Rotation(usize, f32),
    Spin(usize, f32),
    Scale(usize, f32),
    Growth(usize, f32),
    Color(usize, u8),
    Speed(f32),
    Motion(Motion),
    Blend(Blend),
    GravityAndFade(f32, Fade),
}
impl Property {
    fn decode(property: i32, value: i32) -> Result<Option<Self>, String> {
        let scaled = value as f32 / PERCENT;
        Ok(Some(match property {
            420..=422 => Self::Position((property - 420) as usize, value as f32),
            423..=425 => Self::Velocity((property - 423) as usize, scaled),
            426..=428 => Self::Rotation((property - 426) as usize, scaled),
            429..=431 => Self::Spin((property - 429) as usize, scaled),
            432..=434 => Self::Scale((property - 432) as usize, scaled),
            435..=437 => Self::Growth((property - 435) as usize, scaled),
            438..=441 => Self::Color((property - 438) as usize, value.clamp(0, 255) as u8),
            442 => Self::Speed(scaled),
            443 => Self::Motion(if value & 1 == 0 {
                Motion::Velocity
            } else {
                Motion::Direction
            }),
            444..=446 => Self::Velocity((property - 444) as usize, value as f32),
            447 => Self::Blend(value.try_into()?),
            448 => Self::GravityAndFade(
                (f32::from(value & 4 != 0) - f32::from(value & 1 != 0)) * GRAVITY,
                if value & 8 != 0 {
                    Fade::Proportional
                } else if value & 2 != 0 {
                    Fade::Linear
                } else {
                    Fade::Tail
                },
            ),
            // Particle shading uses its explicit tint; this flag does not change it.
            449 => return Ok(None),
            _ => return Err(format!("unsupported model-particle property {property}")),
        }))
    }
    pub(crate) fn apply(self, particle: &mut ModelParticle) {
        match self {
            Self::Position(axis, value) => particle.position[axis] = value,
            Self::Velocity(axis, value) => particle.velocity[axis] = value,
            Self::Rotation(axis, value) => particle.rotation[axis] = value,
            Self::Spin(axis, value) => particle.angular_velocity[axis] = value,
            Self::Scale(axis, value) => particle.scale[axis] = value,
            Self::Growth(axis, value) => particle.scale_delta[axis] = value,
            Self::Color(channel, value) => particle.rgba[channel] = value,
            Self::Speed(value) => particle.speed = value,
            Self::Motion(value) => particle.motion = value,
            Self::Blend(value) => particle.blend = value,
            Self::GravityAndFade(gravity, fade) => {
                particle.gravity = gravity;
                particle.fade = fade;
            }
        }
    }
}
impl crate::GameWorld {
    pub(crate) fn set_model_particle_property(
        &mut self,
        handle: i32,
        property: i32,
        value: i32,
    ) -> Result<(), String> {
        if let Some(particle) = self.model_particles.get(&handle)
            && let Some(change) = Property::decode(property, value)?
        {
            self.queue_effect_change(
                handle,
                particle.born,
                crate::effect::property::Change::Model(change),
            );
        }
        Ok(())
    }

    pub(crate) fn emit_model_particle(
        &mut self,
        mut particle: ModelParticle,
    ) -> Result<i32, String> {
        if self.model_particles.len() >= POOL_CAPACITY {
            return Ok(0);
        }
        let handle = self.allocate_effect()?;
        particle.born = self.tick + 1;
        self.model_particles.insert(handle, particle);
        Ok(handle)
    }
    pub(crate) fn step_model_particles(&mut self) {
        self.model_particles
            .retain(|_, particle| particle.step(self.tick));
    }
}
