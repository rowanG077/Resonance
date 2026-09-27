//! Two counter-rotating trails in the field's horizontal plane.
use crate::effect::{BillboardEffect, Fade, NEUTRAL_TINT};

const FIRST_RANDOM_PALETTE: i16 = 105;
const RANDOM_PALETTES: [u16; 7] = [101, 85, 73, 89, 77, 97, 93];
const ANGLE_SCALE: f32 = 100.;
const ALPHA_SCALE: f32 = 16.;
const LIFETIME: u32 = 61;

#[derive(Debug, Clone, Copy)]
enum Palette {
    Fixed(u16),
    Random(u16),
}

impl Palette {
    fn new(value: i16) -> Result<Self, String> {
        match value {
            0..=104 => Ok(Self::Fixed(value as u16)),
            105..=108 => Ok(Self::Random((value - FIRST_RANDOM_PALETTE) as u16)),
            _ => Err("unsupported orbit palette".into()),
        }
    }
    fn native(self) -> i32 {
        match self {
            Self::Fixed(value) => i32::from(value),
            Self::Random(offset) => i32::from(FIRST_RANDOM_PALETTE) + i32::from(offset),
        }
    }
    fn sample(self, random: &mut u32) -> u16 {
        match self {
            Self::Fixed(value) => value,
            Self::Random(offset) => {
                RANDOM_PALETTES[crate::world::random(random) as usize % RANDOM_PALETTES.len()]
                    + offset
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Orbit {
    palette: Palette,
    radius: i16,
    size: i16,
    alpha: i16,
    fade: f32,
    speed: f32,
    angle: f32,
    phase: u16,
}
impl Orbit {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        Ok(Self {
            palette: Palette::new(a[8] as i16)?,
            radius: a[9] as i16,
            size: a[10] as i16,
            alpha: a[11] as i16,
            fade: a[12] as f32,
            speed: a[7] as f32,
            angle: 0.,
            phase: 0,
        })
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property == 113 {
            let previous = self.palette.native();
            if let Some(value) = value {
                self.palette = Palette::new(value as i16)?;
            }
            return Ok(previous);
        }
        properties!(self, property, value; super::PHASE_PROPERTY => phase, 114 => radius, 115 => size, 116 => alpha, 117 => fade)
    }
    pub(super) fn particles(
        &mut self,
        position: [f32; 3],
        born: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        if self.phase == 0 {
            self.angle = self.speed;
            self.phase = 1;
            return;
        }
        for direction in [1., -1.] {
            let (sin, cos) = (direction * self.angle / ANGLE_SCALE)
                .to_radians()
                .sin_cos();
            out.push(BillboardEffect {
                recipe: super::MOTE_SPRITE,
                palette: Some(self.palette.sample(random)),
                lifetime: LIFETIME,
                position: [
                    position[0] + cos * f32::from(self.radius),
                    position[1] + sin * f32::from(self.radius),
                    position[2],
                ],
                size: [f32::from(self.size); 2],
                rgba: [
                    NEUTRAL_TINT,
                    NEUTRAL_TINT,
                    NEUTRAL_TINT,
                    (self.alpha / ALPHA_SCALE as i16) as u8,
                ],
                fade: Fade::Linear((self.fade as i32 as i16) as f32 / ALPHA_SCALE),
                ..super::particle([0.; 3], born, 0, 1)
            });
        }
        self.angle += self.speed;
    }
}
