//! Orbiting particles retain their destination and can be released together.
use super::{Births, normalized, palette, particle, rotated};
use crate::{
    Actor,
    effect::{BillboardController, Fade, SEAL_SPARK_SPRITE, STAR_SPRITE},
    world::random,
};

#[derive(Debug, Clone)]
pub(crate) struct Guided {
    center: [f32; 3],
    target: [f32; 3],
    turn: f32,
    expansion: Option<(f32, f32, i32)>,
    gate: Option<[f32; 2]>,
    speed: f32,
    released: bool,
    travelling: bool,
}
impl Guided {
    pub fn release(&mut self) {
        self.released = true;
    }

    pub fn step(&mut self, position: &mut [f32; 3]) -> Option<([f32; 3], u32)> {
        if self.travelling {
            return None;
        }
        let ready = self.gate.is_none_or(|gate| {
            (0..2).all(|i| {
                position[i] > (gate[i] - 50.).trunc() && position[i] < (gate[i] + 50.).trunc()
            })
        });
        if self.released && ready {
            let delta = std::array::from_fn(|i| self.target[i] - position[i]);
            let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
            let lifetime = (distance / self.speed) as u32 + if self.gate.is_some() { 6 } else { 1 };
            self.travelling = true;
            return Some((normalized(delta).map(|v| v * self.speed), lifetime));
        }
        let mut radial = std::array::from_fn(|i| position[i] - self.center[i]);
        if let Some((radius, limit, vertical)) = &mut self.expansion {
            if *radius <= *limit {
                *radius += 2.;
            }
            let rise = vertical.signum();
            *vertical -= rise;
            position[2] += rise as f32;
            radial[2] = position[2] - self.center[2];
            radial = normalized(radial).map(|v| v * *radius);
        }
        radial = rotated(radial, [0., 0., 1.], self.turn);
        for i in 0..2 {
            position[i] = self.center[i] + radial[i];
        }
        None
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct Orbiting {
    pub expanding: bool,
    pub radius: f32,
    pub count: u32,
    pub size: f32,
    pub variation: u32,
    pub direction_target: [f32; 3],
    pub target: [f32; 3],
}
impl Orbiting {
    pub fn emit(
        &mut self,
        (owner, center): (i32, [f32; 3]),
        actor: &Actor,
        phase: &mut u8,
        born: u32,
        clock: u32,
        rng: &mut u32,
        out: &mut Births,
    ) -> Result<(), String> {
        random(rng);
        if *phase == 2 {
            out.release = Some(owner);
            *phase = 3;
        }
        if *phase != 0 || self.expanding && clock.is_multiple_of(2) {
            return Ok(());
        }
        let count = if self.expanding {
            self.count.min(1)
        } else {
            self.count
        };
        for _ in 0..count {
            let star = !random(rng).is_multiple_of(2);
            let mut p = particle(center, born, palette(108, rng), u32::MAX);
            p.owner = Some(owner);
            p.recipe = if star { STAR_SPRITE } else { SEAL_SPARK_SPRITE };
            p.size = [self.size + super::stream::spread(rng, self.variation) as f32; 2];
            p.rgba[3] = 150;
            p.fade = Fade::Linear(0.);
            if star {
                p.rotation[2] = 45.;
            }
            p.angular_velocity[2] = if random(rng).is_multiple_of(2) {
                -2.
            } else {
                2.
            };
            let (expansion, gate, speed) = if self.expanding {
                let direction = normalized(std::array::from_fn(|i| {
                    self.direction_target[i] - center[i]
                }));
                p.position = std::array::from_fn(|i| center[i] + direction[i]);
                let limit = (215 - random(rng) % 30) as f32;
                let vertical = 15 - (random(rng) % 30) as i32;
                let speed = (actor.movement_speed() + (random(rng) % 5) as f32).trunc();
                if speed <= 0. {
                    return Err("orbit release needs positive speed".into());
                }
                (Some((1., limit, vertical)), None, speed)
            } else {
                p.position[2] += 25. - (random(rng) % 50) as f32;
                let (sin, cos) = ((random(rng) % 360) as f32).to_radians().sin_cos();
                p.position[0] += cos * (self.radius + (random(rng) % 50) as f32);
                p.position[1] += sin * (self.radius + (random(rng) % 50) as f32);
                let (sin, cos) = 135f32.to_radians().sin_cos();
                (
                    None,
                    Some([center[0] + cos * self.radius, center[1] + sin * self.radius]),
                    3.,
                )
            };
            let mut guided = Guided {
                center,
                target: self.target,
                expansion,
                gate,
                speed,
                turn: if random(rng).is_multiple_of(2) {
                    2.
                } else {
                    -2.
                },
                released: false,
                travelling: false,
            };
            // The first visible pose already includes one orbit update.
            guided.step(&mut p.position);
            p.step();
            p.controller = Some(BillboardController::Guided(guided));
            out.push(p);
        }
        if self.expanding {
            self.count -= count;
        } else {
            *phase = 1;
        }
        Ok(())
    }
}
