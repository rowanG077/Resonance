//! Rising cardinal motes.
use crate::effect::{BillboardController, BillboardEffect};

#[derive(Debug, Clone)]
pub(crate) struct Cardinal {
    count: i32,
    phase: u16,
}

impl Cardinal {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        let result = Self {
            count: a[8],
            phase: 0,
        };
        if !(0..=4).contains(&result.count) {
            return Err("invalid cardinal mote count".into());
        }
        Ok(result)
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == super::PHASE_PROPERTY {
            let previous = i32::from(self.phase);
            if let Some(value) = value {
                self.phase = value as u16;
            }
            return Ok(previous);
        }
        if property != 113 {
            return Err("unsupported cardinal emitter property".into());
        }
        let previous = self.count;
        if let Some(value) = value {
            self.count = value;
        }
        Ok(previous)
    }
    pub(super) fn particles(
        &mut self,
        owner: i32,
        center: [f32; 3],
        heading: f32,
        born: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        if self.phase != 0 {
            return;
        }
        let palettes = [101_u16, 77, 93, 32];
        let radii = [1.0_f32, -1.0, 0.0, 0.0];
        let vertical = [0.0_f32, 0.0, 1.0, -1.0];
        for index in 0..self.count as usize {
            let palette = if index == 3 {
                32
            } else {
                palettes[index] + (crate::world::random(random) % 4) as u16
            };
            let mut mote = super::particle(center, born, palette, 181);
            mote.owner = Some(owner);
            mote.recipe = 10;
            mote.field_lighting = true;
            mote.field_fog = false;
            mote.size = [25.; 2];
            mote.rgba[3] = 200;
            let (sin, cos) = (-heading.to_radians()).sin_cos();
            let x = radii[index] * 50.;
            let y = vertical[index] * 50.;
            mote.position[0] += x * cos - y * sin;
            mote.position[1] += x * sin + y * cos;
            mote.controller = Some(BillboardController::Cardinal(CardinalMotion {
                center,
                radius: 50.,
                angle: -heading,
                rise: 0.,
                axis: index,
            }));
            out.push(mote);
        }
        self.phase = 1;
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CardinalMotion {
    center: [f32; 3],
    radius: f32,
    angle: f32,
    rise: f32,
    axis: usize,
}

impl CardinalMotion {
    pub(crate) fn advance(&mut self, position: &mut [f32; 3]) {
        let angle = self.angle.to_radians();
        let (sin, cos) = angle.sin_cos();
        let (x, y) = match self.axis {
            0 => (self.radius, 0.),
            1 => (-self.radius, 0.),
            2 => (0., self.radius),
            _ => (0., -self.radius),
        };
        position[0] = self.center[0] + x * cos - y * sin;
        position[1] = self.center[1] + x * sin + y * cos;
        position[2] = self.center[2] + self.rise;
        self.angle += 3.;
        self.rise += 8.;
    }
}
