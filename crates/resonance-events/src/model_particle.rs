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
    pub(crate) fn streak(resource: u32, position: [f32; 3], heading: f32) -> Self {
        Self {
            position,
            rotation: [0., 0., heading],
            scale: [1.5; 3],
            rgba: [32, 32, 126, 75],
            blend: Blend::Additive,
            remaining: Some(5),
            scale_delta: [0., 0., 0.3],
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
    Lifetime(Option<u32>),
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
            // Native450 writes the same remaining-life field as constructor arg1.
            450 => Self::Lifetime((value != i32::from(i16::MAX)).then_some(value.max(0) as u32)),
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
            Self::Lifetime(value) => particle.remaining = value,
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
        born: u32,
    ) -> Result<i32, String> {
        if self.model_particles.len() >= POOL_CAPACITY {
            return Ok(0);
        }
        let handle = self.allocate_effect()?;
        particle.born = born;
        self.model_particles.insert(handle, particle);
        Ok(handle)
    }
    pub(crate) fn step_model_particles(&mut self) {
        self.model_particles
            .retain(|_, particle| particle.step(self.tick));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, EventRuntime, GameWorld, ResourceKind, ResourceLibrary};
    use std::sync::Arc;
    use symphonia_script::{NativeCall, Program};

    #[test]
    fn native_lifetime_setter_retires_live_particles_and_keeps_other_owners() {
        fn native(code: &mut Vec<u16>, op: NativeCall, args: &[i32]) {
            for &arg in args {
                code.extend([
                    0x0200,
                    arg as u16,
                    (arg as u32 >> 16) as u16,
                    0x3000,
                    0x4000,
                ]);
            }
            code.push(0x2000 | u16::from(op as u8));
        }
        let mut code = Vec::new();
        for lifetime in [i32::from(i16::MAX), 4, i32::from(i16::MAX)] {
            native(
                &mut code,
                NativeCall::CreateModelParticle,
                &[9, lifetime, 10, 20, 30, 0, 0, 0, 100, 255, 0],
            );
        }
        native(&mut code, NativeCall::YieldCommand, &[0, 2]);
        native(
            &mut code,
            NativeCall::SetModelParticleProperty,
            &[1, 450, 2],
        );
        native(
            &mut code,
            NativeCall::SetModelParticleProperty,
            &[2, 450, i32::from(i16::MAX)],
        );
        native(&mut code, NativeCall::YieldCommand, &[0, 8]);
        native(
            &mut code,
            NativeCall::SetModelParticleProperty,
            &[2, 450, 0],
        );
        code.push(0x20ff);
        let bytes = [4, 0, 0, 0]
            .into_iter()
            .chain(code)
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        let program = Arc::new(Program::decode(&bytes).unwrap());
        let mut world = GameWorld::default();
        world.insert_actor(1, Actor::new(9, [99.; 3]));
        let mut events = EventRuntime::with_state(
            program,
            Arc::new(ResourceLibrary {
                bindings: [(9, (ResourceKind::Model, 9))].into(),
                ..Default::default()
            }),
            world,
            Default::default(),
        )
        .unwrap();
        events.step().unwrap();
        assert_eq!(events.world.model_particles.len(), 3);
        assert_eq!(events.world.model_particles[&1].position, [10., 20., 30.]);
        for _ in 0..6 {
            events.step().unwrap();
        }
        assert!(!events.world.model_particles.contains_key(&1));
        assert!(events.world.model_particles.contains_key(&2)); // Outlives original finite4.
        assert!(events.world.model_particles.contains_key(&3));
        for _ in 0..6 {
            events.step().unwrap();
        }
        assert!(!events.world.model_particles.contains_key(&2)); // Explicit lifetime0 cleanup.
        assert_eq!(events.world.model_particles.len(), 1);
        assert_eq!(events.world.model_particles[&3].position, [10., 20., 30.]);
        assert_eq!(events.world.actors[&1].position, [99.; 3]);
        assert!(Property::decode(451, 0).is_err());
        let mut negative = ModelParticle::new(9);
        Property::decode(450, -1)
            .unwrap()
            .unwrap()
            .apply(&mut negative);
        assert!(!negative.step(1));
    }
}
