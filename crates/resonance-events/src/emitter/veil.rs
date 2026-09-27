//! Expanding glows offset toward the camera.
use crate::effect::{BillboardController, BillboardEffect, Fade};

parameters! { palette = 113, camera_offset = 114, lifetime = 115, size = 116, alpha = 117, fade = 118, growth = 119, interval = 120, orientation = 121 }

#[derive(Debug, Clone)]
pub(crate) struct Veil {
    parameters: Parameters,
    phase: u16,
}
impl Veil {
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
        if !(0..=104).contains(&p.palette)
            || !(0..=32767).contains(&p.lifetime)
            || p.interval <= 0
            || !(0..=1).contains(&p.orientation)
        {
            return Err("invalid veil palette, lifetime, interval or orientation".into());
        }
        Ok(())
    }
    pub(crate) fn distance(&self) -> f32 {
        self.parameters.camera_offset as i16 as f32
    }
    pub(super) fn interval(&self) -> u32 {
        self.parameters.interval as u32
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
    pub(super) fn particle(
        &mut self,
        emitter: i32,
        center: [f32; 3],
        born: u32,
        random: &mut u32,
    ) -> BillboardEffect {
        self.phase = 1;
        let p = self.parameters;
        let mut particle = super::particle(center, born, p.palette as u16, p.lifetime as u32 + 1);
        particle.recipe = 10;
        particle.field_lighting = true;
        particle.rgba[3] = p.alpha as u8;
        particle.size = [p.size as f32; 2];
        particle.size_delta = p.growth as f32;
        particle.fade = if p.fade == 0 {
            Fade::Linear(0.)
        } else {
            Fade::tail(particle.lifetime)
        };
        if p.orientation == 1 {
            particle.orientation = crate::effect::SpriteOrientation::World;
            particle.rotation =
                std::array::from_fn(|_| (crate::world::random(random) % 360) as f32);
        }
        particle.controller = Some(BillboardController::CameraOffset {
            emitter,
            center,
            distance: self.distance(),
        });
        particle.advance(born, random);
        particle
    }
}
