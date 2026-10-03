//! A stream of sparks aimed at a scripted destination.
use crate::effect::{BillboardEffect, Fade};

parameters! { palette = 113, spread = 114, size = 115, lifetime = 116, target_x = 117, target_y = 118, target_z = 119, size_variation = 120 }

#[derive(Debug, Clone)]
pub(crate) struct Directed {
    parameters: Parameters,
    phase: u16,
}

impl Directed {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        let result = Self {
            parameters: Parameters::read(a),
            phase: 0,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), String> {
        let p = self.parameters;
        if !(0..=101).contains(&p.palette)
            || p.spread <= 0
            || p.lifetime < 0
            || p.size_variation <= 0
        {
            return Err("invalid directed spark palette, spread or lifetime".into());
        }
        Ok(())
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == super::PHASE_PROPERTY {
            return Ok(super::phase(&mut self.phase, value));
        }
        let previous = self.parameters.property(property, value)?;
        self.validate()?;
        Ok(previous)
    }
    pub(super) fn particle(
        &self,
        position: [f32; 3],
        speed: f32,
        born: u32,
        random: &mut u32,
    ) -> BillboardEffect {
        let p = self.parameters;
        let kind = crate::world::random(random) % 3;
        let palette = if p.palette == 32 {
            32
        } else {
            p.palette as u16 + (crate::world::random(random) % 4) as u16
        };
        let mut particle = super::particle(position, born, palette, p.lifetime as u32 + 1);
        particle.recipe = match kind {
            0 => 10,
            2 if p.palette == 32 => 12,
            _ => 69,
        };
        let size =
            (p.size + (crate::world::random(random) % p.size_variation as u32) as i32) as f64;
        particle.size = [(size * if kind == 1 { 1.2 } else { 1. }) as f32; 2];
        particle.field_lighting = true;
        particle.field_fog = false;
        particle.blend_mode = (p.palette == 32).then_some(0);
        particle.rgba[3] = 150;
        particle.fade = Fade::tail(particle.lifetime);
        let direction: [f32; 3] = std::array::from_fn(|i| {
            [p.target_x, p.target_y, p.target_z][i] as f32 - position[i]
                + (p.spread - (crate::world::random(random) % (p.spread * 2) as u32) as i32) as f32
        });
        let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
        particle.velocity = direction.map(|v| {
            if length == 0. {
                0.
            } else {
                v / length * speed / 100.
            }
        });
        particle.angular_velocity[2] = if crate::world::random(random) & 1 != 0 {
            3.
        } else {
            -3.
        };
        particle
    }
}
