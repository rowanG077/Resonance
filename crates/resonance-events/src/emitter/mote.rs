//! A colored mote follows a sampled quadratic path for one flight.
use super::{normalized, particle};
use crate::{Actor, effect::Fade, world::random};

#[derive(Debug, Clone, Default)]
pub(crate) struct Mote {
    pub curvature: i32,
    pub size: f32,
    pub target: [f32; 3],
    // The scenario can label an active flight; only zero requests a restart.
    pub script_phase: u8,
    pub(super) path: Option<Path>,
}

#[derive(Debug, Clone)]
pub(super) struct Path {
    origin: [f32; 3],
    velocity: [f32; 3],
    bend: [f32; 3],
    duration: u32,
    age: u32,
}

impl Path {
    pub(super) fn new(
        origin: [f32; 3],
        target: [f32; 3],
        launch: [f32; 3],
        speed: f32,
        curvature: i32,
    ) -> Self {
        let delta = std::array::from_fn(|i| target[i] - origin[i]);
        let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
        let velocity = normalized(delta).map(|v| v * speed);
        let launch = normalized(launch).map(|v| v * speed);
        let curvature = if curvature == 0 {
            1.
        } else {
            curvature as f32 / 100.
        };
        Self {
            origin,
            velocity,
            bend: std::array::from_fn(|i| (launch[i] - velocity[i]) * curvature),
            // Equal outward and returning halves keep the flight symmetric.
            duration: (distance / (2. * speed)).max(1.) as u32 * 2,
            age: 0,
        }
    }

    pub(super) fn advance(&mut self) -> [f32; 3] {
        self.age += 1;
        let age = self.age as f32;
        let curve = age * (1. - (age + 1.) / self.duration as f32);
        std::array::from_fn(|i| self.origin[i] + self.velocity[i] * age + self.bend[i] * curve)
    }

    pub(super) fn arrived(&self) -> bool {
        self.age >= self.duration
    }

    pub(super) fn impact_rotation(&self) -> f32 {
        if self.bend[..2].iter().any(|v| *v != 0.) {
            -(-self.bend[0]).atan2(-self.bend[1]).to_degrees()
        } else {
            0.
        }
    }
}

impl Mote {
    pub fn emit(
        &mut self,
        actor: &mut Actor,
        born: u32,
        rng: &mut u32,
        out: &mut super::Births,
    ) -> Result<(), String> {
        random(rng);
        if self.path.is_none() {
            let speed = actor.movement_speed();
            if speed <= 0. {
                return Err("travelling mote needs positive speed".into());
            }
            let launch =
                std::array::from_fn(|_| (random(rng) % 100) as f32 - (random(rng) % 100) as f32);
            self.path = Some(Path::new(
                actor.position,
                self.target,
                launch,
                speed,
                self.curvature,
            ));
            self.script_phase = 1;
        }
        let path = self.path.as_mut().unwrap();
        actor.position = path.advance();
        if path.age <= path.duration {
            const FIRST_COLOR: u16 = 33;
            const COLORS: u32 = 32;
            let mut mote = particle(
                actor.position,
                born,
                FIRST_COLOR + (random(rng) % COLORS) as u16,
                2,
            );
            mote.size = [self.size; 2];
            mote.rgba[3] = 255;
            mote.fade = Fade::Linear(-10.);
            out.push(mote);
        }
        Ok(())
    }
}
