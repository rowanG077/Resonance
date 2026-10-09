//! Stacked discs release upward one at a time, or expand together.
use super::{inherit, particle};
use crate::{
    Actor,
    effect::{BillboardController, Fade, SpriteOrientation},
};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) enum Phase {
    #[default]
    Stacked,
    Releasing(u32),
    Done,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Column {
    pub palette: u16,
    pub size: f32,
    pub layers: i32,
    pub alpha: i32,
    pub spacing: f32,
    pub lifetime: u32,
    pub growth: f32,
    pub expands: bool,
    pub phase: Phase,
}

impl Column {
    pub fn emit(
        &mut self,
        center: [f32; 3],
        actor: &Actor,
        born: u32,
        random: &mut u32,
        out: &mut super::Births,
    ) {
        const ACCELERATION: f32 = 1.15;
        crate::world::random(random);
        let released = match self.phase {
            Phase::Stacked => None,
            Phase::Releasing(age) => Some(age),
            Phase::Done => return,
        };
        let remaining = if self.expands {
            self.layers
        } else {
            self.layers.saturating_sub(released.unwrap_or(0) as i32)
        };
        let count = remaining + i32::from(released.is_some() && !self.expands);
        for layer in 0..count.max(0) {
            let releasing = released.is_some() && (self.expands || layer == remaining);
            let lifetime = if releasing {
                if self.expands { self.lifetime + 1 } else { 301 }
            } else {
                2
            };
            let mut p = particle(center, born, self.palette, lifetime.max(1));
            p.recipe = crate::effect::WORLD_GLOW_SPRITE;
            p.orientation = SpriteOrientation::World;
            p.position[2] += layer as f32 * self.spacing;
            p.size = [self.size; 2];
            p.rgba[3] =
                (self.alpha * if releasing && !self.expands { 3 } else { 1 }).clamp(0, 255) as u8;
            if releasing {
                if self.expands {
                    p.size_delta = self.growth;
                } else {
                    p.position[2] += ACCELERATION;
                    p.velocity[2] = ACCELERATION;
                    p.controller = Some(BillboardController::Accelerate {
                        multiplier: ACCELERATION,
                        delta: [0.; 3],
                    });
                }
            } else {
                p.fade = Fade::Linear(0.);
            }
            inherit(&mut p, actor);
            out.push(p);
        }
        if let Some(age) = released {
            self.phase = if self.expands || remaining <= 0 {
                Phase::Done
            } else {
                Phase::Releasing(age.saturating_add(1))
            };
        }
    }
}
