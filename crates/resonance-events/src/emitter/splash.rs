//! Periodic rings or sprays of ballistic droplets.
use crate::effect::{BillboardEffect, Fade};

const DROPLET: u16 = 4;
const TURN: f32 = 360.;
const GRAVITY: f32 = -0.98;
const RISE_VARIATION: u32 = 5;
const TINT_RED: i32 = 42;
const OPACITY: i32 = 8;

#[derive(Debug, Clone)]
pub(crate) struct Splash {
    distribution: Distribution,
    palette: i16,
    radius: i16,
    duration: i16,
    interval: i16,
    angle: f32,
    size: f32,
    alpha: f32,
    rise: f32,
    spread: f32,
    size_variation: f32,
    phase: u16,
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Distribution {
    Ring,
    Spray,
}
impl Splash {
    pub(super) fn from_native(a: &[i32], distribution: Distribution) -> Result<Self, String> {
        let splash = Self {
            distribution,
            palette: a[8] as i16,
            radius: a[9] as i16,
            duration: a[10] as i16,
            interval: a[11] as i16,
            angle: a[12] as f32,
            size: a[13] as f32,
            alpha: a[14] as f32,
            rise: a[15] as f32,
            spread: a[16] as f32,
            size_variation: a[17] as f32,
            phase: 0,
        };
        splash.validate()?;
        Ok(splash)
    }
    fn validate(&self) -> Result<(), String> {
        if !(0..resonance_content::effect::FIELD_PALETTE_COLORS as i16).contains(&self.palette)
            || self.interval <= 0
            || self.duration < 0
            || self.angle < 1.
            || self.spread < 0.
            || (matches!(self.distribution, Distribution::Spray)
                && (self.radius <= 0 || self.size_variation < 1.))
        {
            return Err("invalid splash emitter parameters".into());
        }
        Ok(())
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        let previous = properties!(self, property, value;
            super::PHASE_PROPERTY => phase, 113 => palette, 114 => radius, 115 => duration, 116 => interval,
            117 => angle, 118 => size, 119 => alpha, 120 => rise, 121 => spread, 122 => size_variation)?;
        self.validate()?;
        Ok(previous)
    }
    pub(super) fn particles(
        &self,
        actor: &crate::Actor,
        born: u32,
        tick: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        if !tick.is_multiple_of(self.interval as u32) {
            return;
        }
        let mut rgba = std::array::from_fn(|i| {
            actor
                .properties
                .get(&(TINT_RED + i as i32))
                .copied()
                .unwrap_or(64) as u8
        });
        rgba[3] = match actor.properties.get(&OPACITY).copied().unwrap_or(255) as u8 {
            255 => self.alpha as i32 as u8,
            alpha => alpha,
        };
        for spoke in 1..=(TURN / self.angle).ceil() as usize {
            let (sin, cos) = (spoke as f32 * self.angle).to_radians().sin_cos();
            let direction = [cos, sin];
            let (size, radius) = match self.distribution {
                Distribution::Ring => (self.size, f32::from(self.radius)),
                Distribution::Spray => (
                    self.size + (crate::world::random(random) % self.size_variation as u32) as f32,
                    (crate::world::random(random) % self.radius as u32) as f32,
                ),
            };
            let horizontal = if self.radius == 0 {
                std::array::from_fn(|_| {
                    self.spread
                        - (crate::world::random(random) % (self.spread as u32 * 2 + 1)) as f32
                })
            } else {
                direction.map(|v| v * self.spread)
            };
            out.push(BillboardEffect {
                field_lighting: true,
                field_fog: matches!(self.distribution, Distribution::Spray),
                recipe: DROPLET,
                palette: Some(self.palette as u16),
                lifetime: self.duration as u32 + 1,
                position: [
                    actor.position[0] + direction[0] * radius,
                    actor.position[1] + direction[1] * radius,
                    actor.position[2],
                ],
                velocity: [
                    horizontal[0],
                    horizontal[1],
                    self.rise + (crate::world::random(random) % RISE_VARIATION) as f32,
                ],
                gravity: GRAVITY,
                size: [size; 2],
                rgba,
                // The constructor keeps fixed-point alpha enabled, with zero delta.
                fade: Fade::Linear(0.),
                blend_mode: actor.blend.map(|blend| blend as u8),
                ..super::particle([0.; 3], born, 0, 1)
            });
        }
    }
}
