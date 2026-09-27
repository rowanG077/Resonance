//! Independent model particles (native 0xDD/0xDE), with no gameplay actor identity.
const POOL_CAPACITY: usize = 512;
const PERCENT: f32 = 100.;
const ALPHA_FRACTION: i16 = 16;
const MAX_ALPHA: i16 = 0x0fff;
const FADE_TICKS: u16 = 32;
const FADE_STEP: u8 = 8;
const GRAVITY: f32 = 0.98;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Alpha,
    Additive,
    Subtractive,
}
impl TryFrom<i32> for Blend {
    type Error = String;
    fn try_from(value: i32) -> Result<Self, String> {
        match value & 3 {
            0 => Ok(Self::Alpha),
            1 => Ok(Self::Additive),
            2 => Ok(Self::Subtractive),
            _ => Err("inherited model-particle blend is not implemented".into()),
        }
    }
}
#[derive(Debug, Clone, Copy)]
enum Lifetime {
    Frames(u16),
    Indefinite,
    Expired,
}
impl From<i16> for Lifetime {
    fn from(value: i16) -> Self {
        match value {
            i16::MAX => Self::Indefinite,
            0.. => Self::Frames(value as u16),
            _ => Self::Expired,
        }
    }
}
#[derive(Debug, Clone, Copy)]
enum Motion {
    Velocity,
    Direction,
}
#[derive(Debug, Clone, Copy)]
enum Fade {
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
    pub field_lighting: bool,
    lifetime: Lifetime,
    velocity: [f32; 3],
    angular_velocity: [f32; 3],
    scale_delta: [f32; 3],
    motion: Motion,
    speed: f32,
    gravity: f32,
    fade: Fade,
    alpha: i16,
    alpha_delta: i16,
}
impl ModelParticle {
    pub(crate) fn scoped(resource: u32, operation: crate::Operation) -> Self {
        Self {
            operation: Some(operation),
            resource,
            position: [0.; 3],
            rotation: [0.; 3],
            orientation: crate::effect::SpriteOrientation::World,
            scale: [1.; 3],
            rgba: [super::effect::NEUTRAL_TINT; 4],
            blend: Blend::Alpha,
            field_lighting: false,
            lifetime: Lifetime::Indefinite,
            velocity: [0.; 3],
            angular_velocity: [0.; 3],
            scale_delta: [0.; 3],
            motion: Motion::Velocity,
            speed: 0.,
            gravity: 0.,
            fade: Fade::Tail,
            alpha: 0,
            alpha_delta: 0,
        }
    }
    pub(crate) fn from_native(resource: u32, a: &[i32]) -> Self {
        Self {
            operation: None,
            resource,
            position: std::array::from_fn(|i| a[2 + i] as f32),
            rotation: std::array::from_fn(|i| a[5 + i] as f32),
            orientation: crate::effect::SpriteOrientation::World,
            scale: [a[8] as f32 / PERCENT; 3],
            rgba: [
                super::effect::NEUTRAL_TINT,
                super::effect::NEUTRAL_TINT,
                super::effect::NEUTRAL_TINT,
                a[9] as u8,
            ],
            blend: Blend::Alpha,
            field_lighting: true,
            lifetime: (a[1] as i16).into(),
            velocity: [0.; 3],
            angular_velocity: [0.; 3],
            scale_delta: [0.; 3],
            motion: Motion::Velocity,
            speed: 0.,
            gravity: 0.,
            fade: Fade::Tail,
            alpha: (a[9] as i16).wrapping_mul(ALPHA_FRACTION),
            alpha_delta: (a[10] as i16).wrapping_mul(ALPHA_FRACTION),
        }
    }
    pub(crate) fn set_property(&mut self, property: i32, value: i32) -> Result<(), String> {
        // Encodings stay at the legacy boundary; updates below use typed state.
        let scaled = value as f32 / PERCENT;
        match property {
            420..=422 => self.position[(property - 420) as usize] = value as f32,
            423..=425 => self.velocity[(property - 423) as usize] = scaled,
            426..=428 => self.rotation[(property - 426) as usize] = scaled,
            429..=431 => self.angular_velocity[(property - 429) as usize] = scaled,
            432..=434 => self.scale[(property - 432) as usize] = scaled,
            435..=437 => self.scale_delta[(property - 435) as usize] = scaled,
            438..=441 => self.rgba[(property - 438) as usize] = value as u8,
            442 => self.speed = scaled,
            443 => {
                self.motion = if value & 1 == 0 {
                    Motion::Velocity
                } else {
                    Motion::Direction
                }
            }
            444..=446 => self.velocity[(property - 444) as usize] = value as f32,
            447 => self.blend = value.try_into()?,
            448 => {
                self.gravity = (f32::from(value & 4 != 0) - f32::from(value & 1 != 0)) * GRAVITY;
                self.fade = if value & 8 != 0 {
                    Fade::Proportional
                } else if value & 2 != 0 {
                    Fade::Linear
                } else {
                    Fade::Tail
                };
            }
            449 => self.field_lighting = value & 1 != 0,
            _ => return Err(format!("unsupported model-particle property {property}")),
        }
        Ok(())
    }
    fn step(&mut self) -> bool {
        if let Some(operation) = &self.operation {
            return operation.is_pending();
        }
        let remaining = match &mut self.lifetime {
            Lifetime::Frames(0) | Lifetime::Expired => return false,
            Lifetime::Frames(ticks) => {
                let previous = *ticks;
                *ticks -= 1;
                Some(previous)
            }
            Lifetime::Indefinite => None,
        };
        match self.fade {
            Fade::Linear => {
                self.alpha = self.alpha.wrapping_add(self.alpha_delta).min(MAX_ALPHA);
                if self.alpha < 0 {
                    return false;
                }
                self.rgba[3] = (self.alpha / ALPHA_FRACTION) as u8;
            }
            Fade::Proportional => {
                if let Some(ticks) = remaining.filter(|ticks| *ticks < u16::from(self.rgba[3])) {
                    let delta = if u16::from(self.rgba[3]) - ticks > 1 {
                        u16::from(self.rgba[3]) / ticks
                    } else {
                        1
                    };
                    let next = u16::from(self.rgba[3]).saturating_sub(delta);
                    if next > 0 {
                        self.rgba[3] = next as u8;
                    }
                }
            }
            Fade::Tail if remaining.is_some_and(|ticks| ticks < FADE_TICKS) => {
                if self.rgba[3] > FADE_STEP {
                    self.rgba[3] -= FADE_STEP;
                }
            }
            Fade::Tail => {}
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
impl crate::GameWorld {
    pub(crate) fn emit_model_particle(&mut self, particle: ModelParticle) -> Result<i32, String> {
        if self.model_particles.len() >= POOL_CAPACITY {
            return Ok(0);
        }
        let handle = self.allocate_effect()?;
        if !matches!(particle.lifetime, Lifetime::Expired) {
            self.model_particles.insert(handle, particle);
        }
        Ok(handle)
    }
    pub(crate) fn step_model_particles(&mut self) {
        self.model_particles.retain(|_, particle| particle.step());
    }
}
