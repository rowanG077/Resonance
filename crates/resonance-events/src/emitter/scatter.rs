//! Drifting lights, rising lights, then five spiral arms.
use super::particle;
use crate::effect::{BillboardController, BillboardEffect, Fade};

parameters! { palette = 113, star_size = 114, star_variation = 115, mote_size = 116, mote_variation = 117, star_lifetime = 118, mote_lifetime = 119 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum Phase {
    Drifting,
    Rising,
    Spiraling,
}

#[derive(Debug, Clone)]
pub(crate) struct Scatter {
    parameters: Parameters,
    phase: Phase,
    angle: f32,
}
impl Scatter {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        let result = Self {
            parameters: Parameters::read(a),
            phase: Phase::Drifting,
            angle: 0.,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), String> {
        let p = self.parameters;
        if !(0..=104).contains(&p.palette)
            || p.star_variation <= 0
            || p.mote_variation <= 0
            || !(0..=32767).contains(&p.star_lifetime)
            || !(0..=32767).contains(&p.mote_lifetime)
        {
            return Err("invalid scatter emitter palette, variation or lifetime".into());
        }
        Ok(())
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == super::PHASE_PROPERTY {
            let previous = self.phase as i32;
            if let Some(value) = value {
                self.phase = match value as u16 {
                    0 => Phase::Drifting,
                    1 => Phase::Rising,
                    2 => Phase::Spiraling,
                    _ => return Err("unsupported scatter phase".into()),
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn particles(
        &mut self,
        center: [f32; 3],
        speed: f32,
        blend: Option<crate::model_particle::Blend>,
        born: u32,
        tick: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        let p = self.parameters;
        if self.phase != Phase::Spiraling {
            let mut star = particle(center, born, p.palette as u16, p.star_lifetime as u32 + 1);
            star.size = [(p.star_size + draw(random, p.star_variation)) as f32; 2];
            star.rgba[3] = 125;
            star.fade = Fade::Linear(-10. / 16.);
            let x = 90. - draw(random, 180) as f32;
            let z = 90. - draw(random, 180) as f32;
            let y = if self.phase == Phase::Drifting {
                90. - draw(random, 180) as f32
            } else {
                0.
            };
            let speed = if self.phase == Phase::Drifting {
                speed / 10.
            } else {
                0.8
            };
            star.size_delta = if draw(random, 2) != 0 { 0.25 } else { -0.25 };
            star.rotation[2] = draw(random, 360) as f32;
            star.blend_mode = Some(blend.map_or(1, |b| b as u8));
            let direction = [x, y, z];
            let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
            star.velocity = direction.map(|v| if length == 0. { 0. } else { v / length * speed });
            if self.phase == Phase::Rising {
                star.controller = Some(BillboardController::Directed {
                    direction,
                    speed,
                    gravity: 0.98,
                });
            }
            out.push(star);
        } else if tick.is_multiple_of(3) {
            for arm in 0..5 {
                let mut star = particle(center, born, p.palette as u16, p.star_lifetime as u32 + 1);
                star.size = [(p.star_size + draw(random, p.star_variation)) as f32; 2];
                star.rgba[3] = 50;
                star.size_delta = 0.25;
                star.rotation[2] = draw(random, 360) as f32;
                let angle = (arm as f32 * 72. + self.angle - 1.).to_radians();
                star.position[0] += angle.cos();
                star.position[2] += angle.sin();
                star.blend_mode = Some(blend.map_or(1, |b| b as u8));
                star.controller = Some(BillboardController::Spiral { center, radius: 1. });
                // Native callbacks execute once when the particle is first drawn.
                star.advance(tick, random);
                out.push(star);
            }
            self.angle += 4.;
        }
        if tick.is_multiple_of(3) {
            let palette = if blend.is_some() {
                if self.phase == Phase::Drifting {
                    [104, 88, 76, 92, 80, 100, 96][draw(random, 7) as usize]
                } else {
                    36
                }
            } else {
                (p.palette + draw(random, 4) * if p.palette >= 65 { 1 } else { 7 }) as u16
            };
            let mut mote = particle(center, born, palette, p.mote_lifetime as u32 + 1);
            mote.recipe = 10;
            mote.size = [(p.mote_size + draw(random, p.mote_variation)) as f32; 2];
            mote.rgba[3] = 150;
            let x = 90. - draw(random, 180) as f32;
            let y = if self.phase == Phase::Drifting {
                90. - draw(random, 180) as f32
            } else {
                0.
            };
            let z = 90. - draw(random, 180) as f32;
            let drift = if self.phase == Phase::Rising {
                0.8 + (draw(random, 12) / 10) as f32
            } else {
                if self.phase == Phase::Spiraling {
                    draw(random, 12);
                }
                draw(random, 10);
                speed / 10.
            };
            mote.controller = Some(BillboardController::Wander {
                direction: [x, y, z],
                speed: drift,
                gravity: if self.phase == Phase::Rising {
                    0.98
                } else {
                    0.
                },
            });
            if blend.is_some() {
                mote.blend_mode = Some(2);
            }
            mote.advance(tick, random);
            out.push(mote);
        }
    }
}
fn draw(random: &mut u32, divisor: i32) -> i32 {
    (crate::world::random(random) % divisor as u32) as i32
}
