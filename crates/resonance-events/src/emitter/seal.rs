//! The seal teleporter's staged spark burst.
use crate::effect::{BillboardEffect, Fade};

parameters! { palette = 113, opening_size = 114, pulse_size = 115, spark_size = 116 }

#[derive(Debug, Clone)]
pub(crate) struct Seal {
    parameters: Parameters,
    phase: u16,
}

impl Seal {
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
        if !(0..=108).contains(&p.palette)
            || p.opening_size < 0
            || p.pulse_size < 0
            || p.spark_size < 0
        {
            return Err("invalid seal emitter parameters".into());
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
        owner: i32,
        center: [f32; 3],
        tick: u32,
        effect_tick: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        let p = self.parameters;
        match self.phase {
            0 => {
                // The opening pulse is refreshed by the native actor each update.
                out.push(glow(
                    owner,
                    center,
                    p.palette,
                    p.opening_size + (*random & 15) as i32,
                    4,
                    255,
                    tick,
                ));
                out.push(glow(
                    owner,
                    center,
                    p.palette + 3,
                    p.pulse_size + 30,
                    1,
                    32,
                    tick,
                ));
                if effect_tick.is_multiple_of(30) {
                    for _ in 0..15 {
                        out.push(spark(owner, center, p.spark_size, 30, random, tick));
                    }
                }
            }
            1 => {
                out.push(glow(owner, center, 33, p.pulse_size, 180, 200, tick));
                out.push(glow(owner, center, p.palette, p.pulse_size, 180, 200, tick));
                for _ in 0..250 {
                    out.push(spark(owner, center, p.spark_size, 180, random, tick));
                }
                self.phase = 2;
            }
            2 if effect_tick.is_multiple_of(5) => {
                out.push(glow(owner, center, 33, p.pulse_size, 30, 100, tick));
                out.push(glow(owner, center, p.palette, p.pulse_size, 30, 100, tick));
            }
            3 => {
                out.push(glow(owner, center, 33, p.pulse_size * 5, 300, 150, tick));
                out.push(glow(
                    owner,
                    center,
                    p.palette,
                    p.pulse_size * 5,
                    300,
                    150,
                    tick,
                ));
                self.phase = 4;
            }
            _ => {}
        }
    }
}

fn glow(
    owner: i32,
    position: [f32; 3],
    palette: i32,
    size: i32,
    lifetime: u32,
    alpha: u8,
    born: u32,
) -> BillboardEffect {
    let mut effect = super::particle(position, born, palette.max(0) as u16, lifetime);
    effect.owner = Some(owner);
    effect.recipe = 10;
    effect.field_lighting = true;
    effect.rgba[3] = alpha;
    effect.size = [size.max(0) as f32; 2];
    effect.fade = Fade::tail(lifetime);
    effect
}

fn spark(
    owner: i32,
    position: [f32; 3],
    size: i32,
    lifetime: u32,
    random: &mut u32,
    born: u32,
) -> BillboardEffect {
    let palette = 65 + (crate::world::random(random) % 32) as u16;
    let mut effect = super::particle(position, born, palette, lifetime);
    effect.owner = Some(owner);
    effect.recipe = if crate::world::random(random) & 1 != 0 {
        7
    } else {
        69
    };
    effect.field_lighting = true;
    effect.size = [size.max(0) as f32; 2];
    effect.angular_velocity[2] = if crate::world::random(random) & 1 != 0 {
        3.
    } else {
        -3.
    };
    effect.fade = Fade::tail(lifetime);
    effect
}
