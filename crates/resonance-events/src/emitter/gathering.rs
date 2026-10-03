//! Collect stars, hold a glow, then scatter its stored energy.
use crate::effect::{BillboardEffect, Fade};

const PALETTE: u16 = 9;
const SIZE_VARIATION: u32 = 10;
const GROWTH_PERIOD: u32 = 5;
const GROWTH: f32 = 4.;
const STAR_SIZE: f32 = 25.;
const SPREAD: f32 = 100.;
const INWARD_SPEED: f32 = 2.;
const OUTWARD_SPEED: f32 = 6.;
const BURST_COUNT: usize = 250;
const TURN: u32 = 360;

#[derive(Debug, Clone, Copy)]
#[repr(u16)]
enum Phase {
    Gathering,
    Holding,
    Burst,
    Spent,
}
impl TryFrom<i32> for Phase {
    type Error = String;
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value as u16 {
            0 => Ok(Self::Gathering),
            1 => Ok(Self::Holding),
            2 => Ok(Self::Burst),
            3 => Ok(Self::Spent),
            _ => Err("invalid gathering emitter phase".into()),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Gathering {
    phase: Phase,
    delay: i16,
    size: f32,
}
impl Gathering {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        if a[9..].iter().any(|p| *p != 0) {
            return Err("unsupported gathering emitter parameters".into());
        }
        Ok(Self {
            phase: Phase::Gathering,
            delay: a[8] as i16,
            size: 0.,
        })
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        const DELAY: i32 = 113;
        match property {
            super::PHASE_PROPERTY => {
                let previous = self.phase as i32;
                if let Some(value) = value {
                    self.phase = value.try_into()?;
                }
                Ok(previous)
            }
            DELAY => {
                let previous = i32::from(self.delay);
                if let Some(value) = value {
                    self.delay = value as i16;
                }
                Ok(previous)
            }
            _ => Err("unsupported gathering emitter property".into()),
        }
    }
    pub(super) fn particles(
        &mut self,
        position: [f32; 3],
        born: u32,
        tick: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        if matches!(self.phase, Phase::Spent) {
            return;
        }
        let mut glow = particle(position, born);
        glow.recipe = super::MOTE_SPRITE;
        glow.rotation = [0.; 3];
        glow.lifetime = 2;
        glow.rgba[3] = 100;
        glow.fade = Fade::Linear(-8.);
        if matches!(self.phase, Phase::Gathering | Phase::Holding) {
            glow.size = [self.size + (crate::world::random(random) % SIZE_VARIATION) as f32; 2];
            out.push(glow.clone());
            if tick.is_multiple_of(GROWTH_PERIOD) {
                self.size += GROWTH;
            }
        }
        match self.phase {
            Phase::Gathering => {
                let mut star = particle(position, born);
                star.angular_velocity[2] = spin(random);
                let direction = direction(random);
                let length = 3_f32.sqrt();
                star.position = std::array::from_fn(|i| position[i] + direction[i] * SPREAD);
                star.velocity = direction.map(|v| -v / length * INWARD_SPEED);
                star.lifetime = (length * SPREAD / INWARD_SPEED) as u32 + 1;
                star.fade = Fade::tail(star.lifetime);
                out.push(star);
            }
            Phase::Holding => {
                if self.delay == 0 {
                    self.phase = Phase::Burst;
                }
                self.delay = self.delay.wrapping_sub(1);
            }
            _ => {}
        }
        if matches!(self.phase, Phase::Burst) {
            // Reaching zero during the holding update also emits the burst now.
            glow.size = [self.size + (crate::world::random(random) % SIZE_VARIATION) as f32; 2];
            glow.field_fog = false;
            glow.lifetime = 181;
            glow.rgba[3] = 200;
            glow.fade = Fade::Linear(-2.);
            glow.size_delta = 7.;
            out.push(glow);
            for _ in 0..BURST_COUNT {
                let mut star = particle(position, born);
                star.field_fog = false;
                star.angular_velocity[2] = spin(random);
                star.velocity = direction(random).map(|v| v / 3_f32.sqrt() * OUTWARD_SPEED);
                star.lifetime = 61;
                star.fade = Fade::Linear(-5.);
                out.push(star);
            }
            self.phase = Phase::Spent;
        }
    }
}
fn spin(random: &mut u32) -> f32 {
    if crate::world::random(random) & 1 != 0 {
        3.
    } else {
        -3.
    }
}
/// Scatter directions are sampled independently around each axis.
fn direction(random: &mut u32) -> [f32; 3] {
    let mut v = [1.; 3];
    for (a, b) in [(1, 2), (2, 0), (0, 1)] {
        let (sin, cos) = ((crate::world::random(random) % TURN) as f32)
            .to_radians()
            .sin_cos();
        let [x, y] = [v[a], v[b]];
        v[a] = x * cos - y * sin;
        v[b] = x * sin + y * cos;
    }
    v
}
fn particle(position: [f32; 3], born: u32) -> BillboardEffect {
    BillboardEffect {
        field_lighting: true,
        recipe: crate::effect::STAR_SPRITE,
        palette: Some(PALETTE),
        lifetime: 1,
        position,
        rotation: [0., 0., 45.],
        size: [STAR_SIZE; 2],
        fade: Fade::Linear(0.),
        ..super::particle([0.; 3], born, 0, 1)
    }
}
