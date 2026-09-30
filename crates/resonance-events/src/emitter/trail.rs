//! Endpoint bursts, a moving glow and model afterimages.
use crate::{
    effect::{BillboardEffect, Fade},
    model_particle::ModelParticle,
};

parameters! { palette = 113, glow_size = 114, burst_size = 115, fade = 116, target_x = 117, target_y = 118, target_z = 119 }

#[derive(Debug, Clone)]
pub(crate) struct Trail {
    parameters: Parameters,
    phase: u16,
    velocity: [f32; 3],
    heading: f32,
}

impl Trail {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        let result = Self {
            parameters: Parameters::read(a),
            phase: 0,
            velocity: [0.; 3],
            heading: 0.,
        };
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> Result<(), String> {
        if !(0..resonance_content::effect::FIELD_PALETTE_COLORS as i32)
            .contains(&self.parameters.palette)
        {
            return Err("invalid moving-trail palette".into());
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

    pub(super) fn particles(
        &mut self,
        position: &mut [f32; 3],
        resource: u32,
        speed: f32,
        born: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) -> Result<Option<ModelParticle>, String> {
        if self.phase == 0 {
            if speed <= 0. {
                return Err("moving-trail speed must be positive".into());
            }
            self.burst(*position, born, random, out);
            let target = self.target();
            let delta: [f32; 3] = std::array::from_fn(|i| target[i] - position[i]);
            let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
            self.heading = 90. - delta[0].atan2(delta[1]).to_degrees();
            self.velocity = delta.map(|v| v / distance.max(speed) * speed);
            self.phase = 1;
        }
        let mut model = None;
        if self.phase == 1 {
            let mut glow = self.glow(*position, born, self.parameters.glow_size as f32);
            glow.fade = Fade::Linear(self.parameters.fade as f32);
            glow.size_delta = -0.75;
            out.push(glow);
            let target = self.target();
            let distance = target
                .iter()
                .zip(*position)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                .sqrt();
            if distance <= speed {
                *position = target;
                self.phase = 2;
            } else {
                for (position, velocity) in position.iter_mut().zip(self.velocity) {
                    *position += velocity;
                }
            }
            if born.is_multiple_of(3) && resource != 0 {
                model = Some(ModelParticle::afterimage(resource, *position, self.heading));
            }
        }
        if self.phase == 2 {
            self.burst(*position, born, random, out);
            self.phase = 3;
        }
        Ok(model)
    }

    fn target(&self) -> [f32; 3] {
        [
            self.parameters.target_x,
            self.parameters.target_y,
            self.parameters.target_z,
        ]
        .map(|v| v as f32)
    }

    fn glow(&self, position: [f32; 3], born: u32, size: f32) -> BillboardEffect {
        let mut glow = super::particle(position, born, self.parameters.palette as u16, 61);
        glow.recipe = 10;
        glow.size = [size; 2];
        glow
    }

    fn burst(
        &self,
        position: [f32; 3],
        born: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        for _ in 0..20 {
            let mut mote = self.glow(position, born, self.parameters.burst_size as f32);
            mote.rgba[3] = 200;
            let direction: [f32; 3] =
                std::array::from_fn(|_| 50. - (crate::world::random(random) % 100) as f32);
            let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
            mote.velocity = direction.map(|v| if length == 0. { 0. } else { v / length * 3. });
            out.push(mote);
        }
    }
}
