//! Four descending spirals followed by two burst discs.
use crate::effect::{BillboardEffect, Fade};

parameters! { palette = 113, radius = 114, lifetime = 115, interval = 116, angular_step = 117, width = 118, height = 119, alpha = 120, fade = 121, burst_growth = 122 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum Phase {
    Contracting,
    Holding,
    Burst,
    Finished,
}

#[derive(Debug, Clone)]
pub(crate) struct Contracting {
    parameters: Parameters,
    phase: Phase,
    angle: f32,
}
impl Contracting {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        let result = Self {
            parameters: Parameters::read(a),
            phase: Phase::Contracting,
            angle: 0.,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), String> {
        let p = self.parameters;
        if !(0..=104).contains(&p.palette) || p.lifetime < 0 || p.interval <= 0 {
            return Err("invalid contracting emitter palette, lifetime or interval".into());
        }
        Ok(())
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == super::PHASE_PROPERTY {
            let previous = self.phase as i32;
            if let Some(value) = value {
                self.phase = match value as u16 {
                    0 => Phase::Contracting,
                    1 => Phase::Holding,
                    2 => Phase::Burst,
                    3 => Phase::Finished,
                    _ => return Err("unsupported contracting emitter phase".into()),
                };
            }
            return Ok(previous);
        }
        let previous = self.parameters.property(property, value)?;
        if value.is_some() {
            self.validate()?;
        }
        Ok(previous)
    }
    pub(super) fn particles(
        &mut self,
        center: &mut [f32; 3],
        speed: f32,
        born: u32,
        tick: u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        let p = self.parameters;
        if self.phase == Phase::Contracting && tick.is_multiple_of(p.interval as u32) {
            for (arm, palette) in [0, 56, 55, 59].into_iter().enumerate() {
                self.angle += p.angular_step as f32;
                let angle = ((self.angle as i32 + arm as i32 * 90) as f32).to_radians();
                let mut mote = super::particle(*center, born, palette, p.lifetime as u32 + 1);
                mote.recipe = 10;
                mote.position[0] += angle.cos() * p.radius as f32;
                mote.position[1] += angle.sin() * p.radius as f32;
                mote.size = [p.width as f32, p.height as f32];
                mote.rgba[3] = p.alpha as u8;
                mote.fade = Fade::Linear(p.fade as f32);
                out.push(mote);
                center[2] += speed;
            }
            self.parameters.radius = p.radius - 4;
            if self.parameters.radius < 0 {
                self.phase = Phase::Holding;
            }
        }
        if self.phase == Phase::Burst {
            for _ in 0..2 {
                let mut disc = super::particle(*center, born, p.palette as u16, 301);
                disc.recipe = 4;
                disc.field_lighting = true;
                disc.size_delta = p.burst_growth as f32;
                disc.fade = Fade::Linear(-10.);
                // The native callback grows the disc before its first draw.
                disc.step();
                out.push(disc);
            }
            self.phase = Phase::Finished;
        }
    }
}
