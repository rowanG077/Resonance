//! An expanding glow with radial camera-facing sparks.
use crate::effect::{BillboardEffect, Fade};

parameters! { palette = 113, camera_offset = 114 }

#[derive(Debug, Clone)]
pub(crate) struct Bloom {
    parameters: Parameters,
    size: f32,
}
impl Bloom {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        let result = Self {
            parameters: Parameters::read(a),
            size: 0.,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), String> {
        if !(0..=108).contains(&self.parameters.palette) {
            return Err("unsupported bloom palette".into());
        }
        Ok(())
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        let previous = self.parameters.property(property, value)?;
        if value.is_some() {
            self.validate()?;
        }
        Ok(previous)
    }
    pub(super) fn particles(
        &mut self,
        mut center: [f32; 3],
        camera: [f32; 3],
        born: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        let palette = self.parameters.palette;
        let palette = if palette >= 105 {
            [101, 85, 73, 89, 77, 97, 93][crate::world::random(random) as usize % 7]
                + (palette - 105) as u16
        } else {
            palette as u16
        };
        let horizontal = camera[0].hypot(camera[1]);
        if horizontal > 0. {
            for i in 0..2 {
                center[i] += camera[i] / horizontal * self.parameters.camera_offset as i16 as f32;
            }
        }
        let mut glow = super::particle(center, born, palette, 6);
        glow.recipe = 10;
        glow.field_lighting = true;
        glow.size = [self.size; 2];
        glow.rgba[3] = 70;
        glow.fade = Fade::Linear(-5.);
        out.push(glow);
        if self.size > 100. {
            let mut spark = super::particle(center, born, 33, 63);
            spark.recipe = 10;
            spark.field_lighting = true;
            spark.size =
                [25. + (crate::world::random(random) % (self.size / 40.) as u32) as f32; 2];
            let angle = ((crate::world::random(random) % 360) as f32).to_radians();
            let length = camera.iter().map(|v| v * v).sum::<f32>().sqrt();
            let axis = camera.map(|v| v / length);
            // Rotate a radial vector around the viewing axis.
            let radial = [-axis[1], axis[0], axis[2]];
            let cross = [
                axis[1] * radial[2] - axis[2] * radial[1],
                axis[2] * radial[0] - axis[0] * radial[2],
                axis[0] * radial[1] - axis[1] * radial[0],
            ];
            let dot = axis.iter().zip(radial).map(|(a, b)| a * b).sum::<f32>();
            let (sin, cos) = angle.sin_cos();
            spark.velocity = std::array::from_fn(|i| {
                (radial[i] * cos + cross[i] * sin + axis[i] * dot * (1. - cos)) * self.size / 125.
            });
            spark.step();
            out.push(spark);
        }
        self.size += 3.;
    }
}
