//! Inward stars charge a flickering glow, then disperse when released.
use super::{normalized, particle, rotated};
use crate::{
    effect::{Fade, STAR_SPRITE},
    world::random,
};

const PALETTE: u16 = 9;
const STAR_SIZE: f32 = 25.;
const INWARD_SPEED: f32 = 2.;
const RELEASE_SPEED: f32 = 6.;
const RELEASE_STARS: usize = 250;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) enum Phase {
    #[default]
    Gathering,
    Charging(u32),
    Release,
    Done,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Gathering {
    size: f32,
    pub phase: Phase,
}
impl Gathering {
    pub fn emit(
        &mut self,
        center: [f32; 3],
        born: u32,
        clock: u32,
        rng: &mut u32,
        out: &mut super::Births,
    ) {
        random(rng);
        let charged = matches!(self.phase, Phase::Charging(0));
        if let Phase::Charging(remaining) = &mut self.phase {
            *remaining = remaining.saturating_sub(1);
        }
        if matches!(self.phase, Phase::Done) {
            return;
        }
        let release = charged || matches!(self.phase, Phase::Release);
        let mut glow = |release| {
            let mut p = particle(center, born, PALETTE, if release { 181 } else { 2 });
            p.size = [self.size + (random(rng) % 10) as f32; 2];
            p.rgba[3] = if release { 200 } else { 100 };
            p.fade = Fade::Linear(if release { -2. } else { 0. });
            p.size_delta = if release { 7. } else { 0. };
            out.push(p);
        };
        if charged {
            glow(false);
        }
        glow(release);
        for _ in 0..if release {
            RELEASE_STARS
        } else {
            usize::from(matches!(self.phase, Phase::Gathering))
        } {
            let spin = if random(rng).is_multiple_of(2) {
                -3.
            } else {
                3.
            };
            let mut direction = [if release { 1. } else { 100. }; 3];
            for axis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
                direction = rotated(direction, axis, (random(rng) % 360) as f32);
            }
            let distance = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
            let mut star = particle(
                center,
                born,
                PALETTE,
                if release {
                    61
                } else {
                    (distance / INWARD_SPEED) as u32 + 1
                },
            );
            star.recipe = STAR_SPRITE;
            star.size = [STAR_SIZE; 2];
            star.rotation[2] = 45.;
            star.angular_velocity[2] = spin;
            star.rgba[3] = 255;
            star.fade = if release {
                Fade::Linear(-5.)
            } else {
                Fade::tail(star.lifetime)
            };
            star.velocity = normalized(direction).map(|v| {
                v * if release {
                    RELEASE_SPEED
                } else {
                    -INWARD_SPEED
                }
            });
            if !release {
                star.position = std::array::from_fn(|i| center[i] + direction[i]);
            }
            out.push(star);
        }
        if release {
            self.phase = Phase::Done;
        } else if clock.is_multiple_of(5) {
            self.size = (self.size + 4.).min(200.);
        }
    }
}
