//! One ring of 72 rising light streaks.
use crate::effect::{BillboardEffect, Fade};

parameters! { palette = 113, radius = 114, speed_variation = 115 }

#[derive(Debug, Clone)]
pub(crate) struct Burst {
    parameters: Parameters,
    phase: u16,
}
impl Burst {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        let result = Self {
            parameters: Parameters::read(a),
            phase: 0,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), String> {
        if !(0..resonance_content::effect::FIELD_PALETTE_COLORS as i32)
            .contains(&self.parameters.palette)
            || self.parameters.speed_variation < 2
        {
            return Err("invalid light burst palette or velocity variation".into());
        }
        Ok(())
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == super::PHASE_PROPERTY {
            return Ok(super::phase(&mut self.phase, value));
        }
        let previous = self.parameters.property(property, value)?;
        if value.is_some() {
            self.validate()?;
        }
        Ok(previous)
    }
    pub(super) fn particles(
        &mut self,
        center: [f32; 3],
        born: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        if self.phase != 0 {
            return;
        }
        let p = self.parameters;
        let mut draw = |range| (crate::world::random(random) % range as u32) as i32;
        for spoke in 0..72 {
            let (sin, cos) = (spoke as f32 * 5.).to_radians().sin_cos();
            let direction = [cos, sin];
            let mut streak = super::particle(center, born, p.palette as u16, 301);
            streak.recipe = 23;
            streak.field_lighting = true;
            streak.position[0] += direction[0] * p.radius as f32;
            streak.position[1] += direction[1] * p.radius as f32;
            streak.size = [(8 + draw(5)) as f32, (100 + draw(100)) as f32];
            streak.rgba[3] = 30;
            streak.fade = Fade::Linear(-2.);
            let velocity = [
                direction[0] * (p.speed_variation + draw(p.speed_variation / 2)) as f32,
                direction[1] * (p.speed_variation + draw(p.speed_variation / 2)) as f32,
                100.,
            ];
            let speed = (20 + draw(20)) as f32;
            let length = velocity.iter().map(|v| v * v).sum::<f32>().sqrt();
            streak.velocity = velocity.map(|v| v / length * speed);
            streak.rotation = [-velocity[1], 0., -velocity[0]];
            streak.step();
            out.push(streak);
        }
        self.phase = 1;
    }
}
