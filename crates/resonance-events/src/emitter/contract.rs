//! Four colored arms contract, wait, and release an expanding glow.
use super::particle;
use crate::{Actor, effect::Fade};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) enum Phase {
    #[default]
    Contracting,
    Waiting,
    Release,
    Done,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Contract {
    pub palette: u16,
    pub radius: f32,
    pub lifetime: u32,
    pub interval: u32,
    pub angular_step: f32,
    pub size: [f32; 2],
    pub alpha: i32,
    pub fade: f32,
    pub growth: f32,
    pub phase: Phase,
    angle: f32,
    batches: u32,
}

impl Contract {
    pub fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
        if matches!(phase, Phase::Contracting) {
            self.batches = 0;
        }
    }

    pub fn emit(
        &mut self,
        center: [f32; 3],
        actor: &mut Actor,
        born: u32,
        clock: u32,
        random: &mut u32,
        out: &mut super::Births,
    ) {
        crate::world::random(random);
        match self.phase {
            Phase::Contracting if clock.is_multiple_of(self.interval) => {
                const CONTRACTION_PER_BATCH: f32 = 4.;
                const COLORS: [[u8; 3]; 4] =
                    [[64, 64, 64], [255, 64, 64], [64, 64, 255], [64, 255, 255]];
                let radius = (self.radius - self.batches as f32 * CONTRACTION_PER_BATCH).max(0.);
                for (arm, rgb) in COLORS.into_iter().enumerate() {
                    self.angle += self.angular_step;
                    let angle = (self.angle + arm as f32 * 90.).to_radians();
                    let mut p = particle(actor.position, born, 0, self.lifetime + 1);
                    p.palette = None;
                    p.position[0] += angle.cos() * radius;
                    p.position[1] += angle.sin() * radius;
                    p.size = self.size;
                    p.rgba = [rgb[0], rgb[1], rgb[2], self.alpha as u8];
                    p.fade = Fade::Linear(self.fade);
                    out.push(p);
                    actor.position[2] += actor.movement_speed();
                }
                self.batches += 1;
                if radius == 0. {
                    self.phase = Phase::Waiting;
                }
            }
            Phase::Release => {
                let mut sphere = particle(center, born, self.palette, self.lifetime + 1);
                sphere.recipe = crate::effect::STATION_GLOW_SPRITE;
                sphere.size_delta = self.growth;
                sphere.fade = Fade::Linear(-10.);
                out.push(sphere.clone());
                out.push(sphere);
                self.phase = Phase::Done;
            }
            _ => {}
        }
    }
}
