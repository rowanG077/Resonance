//! Native emitter 22: descending rings and a ground ripple for station scenes.
use crate::effect::{
    BillboardEffect, Fade, NEUTRAL_PALETTE, NEUTRAL_TINT, RefractionImage, RefractionPulse,
    SpriteOrientation,
};
use resonance_content::effect::VerticalAnchor;

const RING: u16 = 41;
const PALETTE: u16 = 30;
const COLUMN_UPDATES: u16 = 20;
const PULSE_UPDATES: u16 = 120;
const RING_SPACING: u16 = 6;
const RING_LIFETIME: u32 = 21;
const RING_SIZE: f32 = 150.;
const RIPPLE_LIFETIME: u32 = 41;
const SHAKE_DIVISOR: f32 = 10.;

#[derive(Debug, Clone, Copy, Default)]
#[repr(u16)]
enum Phase {
    #[default]
    Initial,
    Column,
    Pulse,
    Finished,
}
#[derive(Debug, Clone, Default)]
pub(crate) struct Quake {
    phase: Phase,
    remaining: u16,
}
impl Quake {
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        if property != super::PHASE_PROPERTY {
            return Err("unsupported quake emitter property".into());
        }
        let previous = self.phase as i32;
        if let Some(value) = value {
            self.phase = match value as u16 {
                0 => Phase::Initial,
                1 => Phase::Column,
                2 => Phase::Pulse,
                3 => Phase::Finished,
                _ => return Err("invalid quake emitter phase".into()),
            };
        }
        Ok(previous)
    }
    pub(super) fn step(
        &mut self,
        position: [f32; 3],
        born: u32,
        out: &mut Vec<BillboardEffect>,
    ) -> (Option<RefractionPulse>, Option<f32>) {
        if matches!(self.phase, Phase::Initial) {
            self.phase = Phase::Column;
            self.remaining = COLUMN_UPDATES;
        }
        if matches!(self.phase, Phase::Column) {
            out.push(BillboardEffect {
                operation: None,
                owner: None,
                field_lighting: true,
                field_fog: true,
                recipe: RING,
                orientation: SpriteOrientation::World,
                anchor: VerticalAnchor::Center,
                palette: Some(PALETTE),
                born,
                lifetime: RING_LIFETIME,
                position: [
                    position[0],
                    position[1],
                    position[2] + f32::from(self.remaining * RING_SPACING),
                ],
                velocity: [0.; 3],
                controller: None,
                acceleration: None,
                gravity: 0.,
                rotation: [0.; 3],
                angular_velocity: [0.; 3],
                size: [RING_SIZE; 2],
                size_delta: 0.,
                rgba: [
                    NEUTRAL_TINT,
                    NEUTRAL_TINT,
                    NEUTRAL_TINT,
                    (255 - self.remaining * RING_SPACING) as u8,
                ],
                fade: Fade::tail(RING_LIFETIME),
                blend_mode: None,
            });
            self.remaining = self.remaining.saturating_sub(1);
            if self.remaining == 0 {
                self.phase = Phase::Pulse;
                self.remaining = PULSE_UPDATES;
            }
        }
        if !matches!(self.phase, Phase::Pulse) {
            return (None, None);
        }
        let ripple = (self.remaining == PULSE_UPDATES).then_some(RefractionPulse {
            operation: None,
            image: RefractionImage::Ripple,
            palette: NEUTRAL_PALETTE,
            orientation: SpriteOrientation::World,
            rotation: [0., 0., 90.],
            position: [position[0], position[1], position[2] + 4.],
            born,
            lifetime: RIPPLE_LIFETIME - 1,
            size: 30.,
            growth: 20.,
            alpha: 192.,
            fade: Fade::tail(RIPPLE_LIFETIME),
        });
        if self.remaining == 0 {
            self.phase = Phase::Finished;
        }
        let shake = f32::from(self.remaining) / SHAKE_DIVISOR;
        self.remaining = self.remaining.saturating_sub(1);
        // The station continuously replaces an indefinite, non-decaying hold.
        (ripple, Some(shake))
    }
}
