//! Pulsing descending shafts and a converging burst.
use crate::effect::{BillboardEffect, Fade, SpriteOrientation};
use resonance_content::effect::VerticalAnchor;

parameters! { Shafts { palette = 113, interval = 114, radius = 115, width = 116, width_variation = 117, height = 118, height_variation = 119, tilt = 120, count_variation = 121 } }
parameters! { Burst { palette = 113, size = 114, growth = 115, radius = 116, radius_variation = 117 } }

#[derive(Debug, Clone, Copy)]
enum Parameters {
    Shafts(Shafts),
    Burst(Burst),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Shafts,
    Burst,
}

#[derive(Debug, Clone)]
pub(crate) struct Beams {
    parameters: Parameters,
    phase: u16,
}
impl Beams {
    pub(super) fn from_native(a: &[i32], kind: Kind) -> Result<Self, String> {
        let result = Self {
            parameters: match kind {
                Kind::Shafts => Parameters::Shafts(Shafts::read(a)),
                Kind::Burst => Parameters::Burst(Burst::read(a)),
            },
            phase: 0,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), String> {
        let valid = match self.parameters {
            Parameters::Burst(p) => (0..=104).contains(&p.palette) && p.radius_variation > 0,
            Parameters::Shafts(p) => {
                (0..=104).contains(&p.palette)
                    && [
                        p.interval,
                        p.width,
                        p.width_variation,
                        p.height_variation,
                        p.count_variation,
                    ]
                    .iter()
                    .all(|v| *v > 0)
            }
        };
        if !valid {
            return Err("invalid beam palette, interval or variation".into());
        }
        Ok(())
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == super::PHASE_PROPERTY {
            return Ok(super::phase(&mut self.phase, value));
        }
        let previous = match &mut self.parameters {
            Parameters::Shafts(p) => p.property(property, value)?,
            Parameters::Burst(p) => p.property(property, value)?,
        };
        if value.is_some() {
            self.validate()?;
        }
        Ok(previous)
    }
    pub(super) fn particles(
        &mut self,
        center: [f32; 3],
        born: u32,
        tick: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        if let Parameters::Burst(p) = self.parameters {
            if self.phase != 0 {
                return;
            }
            let mut glow = super::particle(center, born, p.palette as u16, 601);
            glow.recipe = 5;
            glow.field_lighting = true;
            glow.size = [p.size as f32; 2];
            glow.size_delta = p.growth as f32;
            glow.rgba[3] = 100;
            glow.fade = Fade::Linear(0.);
            out.push(glow);
            for _ in 0..100 {
                let palette = [104, 88, 76, 92, 80, 100, 96][draw(random, 7) as usize];
                let mut beam = super::particle(center, born, palette, 1);
                beam.recipe = 23;
                beam.rgba[3] = (25 + draw(random, 25)) as u8;
                beam.size = [
                    (5 + draw(random, 5)) as f32,
                    (50 + draw(random, 100)) as f32,
                ];
                let theta = draw(random, 360) as f32;
                let (sin, cos) = theta.to_radians().sin_cos();
                let radius = (p.radius + draw(random, p.radius_variation)) as f32;
                beam.position[0] += cos * radius;
                beam.position[2] -= sin * radius;
                beam.velocity = [-cos * 20., 0., sin * 20.];
                beam.rotation = [90., theta - 90., 0.];
                beam.lifetime = (radius / 20.) as u32 + 1;
                beam.fade = Fade::Linear(0.);
                beam.orientation = SpriteOrientation::World;
                beam.anchor = VerticalAnchor::Bottom;
                out.push(beam);
            }
            self.phase = 1;
        } else if let Parameters::Shafts(p) = self.parameters
            && tick.is_multiple_of(p.interval as u32)
        {
            let width = p.width + draw(random, p.width_variation);
            let height = p.height + draw(random, p.height_variation);
            let angle = (draw(random, 360) as f32).to_radians();
            let direction = [angle.sin(), -angle.cos()];
            let position = [
                center[0] + direction[0] * p.radius as f32,
                center[1] + direction[1] * p.radius as f32,
                center[2],
            ];
            let rotation = [
                90. + direction[1] * p.tilt as f32,
                -direction[0] * p.tilt as f32,
                0.,
            ];
            out.push(shaft(
                position,
                rotation,
                born,
                p.palette as u16,
                [width as f32, height as f32],
            ));
            for _ in 0..draw(random, p.count_variation) {
                let offset = width / 2 - draw(random, width);
                let width = p.width + draw(random, p.width_variation);
                let height = p.height + draw(random, p.height_variation);
                out.push(shaft(
                    [position[0] + offset as f32, position[1], position[2]],
                    rotation,
                    born,
                    p.palette as u16,
                    [width as f32, height as f32],
                ));
            }
        }
    }
}
fn draw(random: &mut u32, range: i32) -> i32 {
    (crate::world::random(random) % range as u32) as i32
}
fn shaft(
    position: [f32; 3],
    rotation: [f32; 3],
    born: u32,
    palette: u16,
    size: [f32; 2],
) -> BillboardEffect {
    let mut beam = super::particle(position, born, palette, 301);
    beam.recipe = 23;
    beam.field_lighting = true;
    beam.orientation = SpriteOrientation::World;
    beam.anchor = VerticalAnchor::Top;
    beam.size = size;
    beam.rotation = rotation;
    beam.fade = Fade::RiseFall {
        rise_ticks: 40,
        step: 20. / 16.,
    };
    beam
}
