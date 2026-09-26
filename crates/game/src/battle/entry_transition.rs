//! Ordinary entry fade and frozen-field screen break (117A0 / C020 / BDA8 / BBA0).
//!
//! These are the actual source triangle records. Their construction precedes
//! actor initialization and consumes the same battle random stream.
use super::entry::Random;
use anyhow::{Result, ensure};
use resonance_battle::{SoundBinding, TransitionFrame};
use resonance_content::battle_profile::ScreenBreak;

#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub points: [[f32; 3]; 3],
    pub uv: [[f32; 2]; 3],
    pub center: [f32; 2],
    pub velocity: [f32; 2],
    pub angle: f32,
    pub angular_velocity: f32,
}

#[derive(Debug, Clone)]
pub struct EntryTransition {
    pieces: Vec<Piece>,
    timer: u16,
    active: bool,
    break_sound: SoundBinding,
    viewport: [f32; 2],
    radians_per_degree: f32,
    secondary_rotation_scale: f32,
    draw_depth: f32,
    fade: TransitionFrame,
}

impl EntryTransition {
    pub fn new(
        source: &ScreenBreak,
        fade_color: [u8; 3],
        random: &mut Random,
        break_sound: SoundBinding,
    ) -> Result<Self> {
        ensure!(
            source.points.len() == 43,
            "invalid screen-break point count"
        );
        ensure!(
            source.triangles.len() == 62,
            "invalid screen-break triangle count"
        );
        ensure!(
            source.points.iter().flatten().all(|v| v.is_finite())
                && source
                    .triangles
                    .iter()
                    .flatten()
                    .all(|&i| usize::from(i) < source.points.len()),
            "invalid screen-break geometry"
        );
        ensure!(
            source
                .viewport
                .iter()
                .chain(&source.viewport_center)
                .all(|v| v.is_finite() && *v > 0.)
                && [
                    source.center_weight,
                    source.center_expansion,
                    source.velocity_scale,
                    source.angular_base,
                    source.angular_variation,
                    source.radians_per_degree,
                    source.secondary_rotation_scale,
                    source.draw_depth,
                ]
                .iter()
                .all(|v| v.is_finite()),
            "invalid screen-break operands"
        );
        let pieces = source
            .triangles
            .iter()
            .map(|indices| {
                let points = indices.map(|i| source.points[usize::from(i)]);
                let uv = points.map(|p| [p[0] / source.viewport[0], p[1] / source.viewport[1]]);
                let center = [0, 1].map(|axis| {
                    source.center_weight * (points[2][axis] + (points[0][axis] + points[1][axis]))
                });
                let points = points.map(|p| [p[0] - center[0], p[1] - center[1], 0.]);
                let center = [0, 1].map(|axis| {
                    source
                        .center_expansion
                        .mul_add(center[axis] - source.viewport_center[axis], center[axis])
                });
                // The original unsigned remainder feeds the retained +0x50 speed.
                let angular_velocity = source
                    .angular_variation
                    .mul_add(f32::from(random.next_u16() % 20), source.angular_base);
                let velocity = [0, 1].map(|axis| {
                    source.velocity_scale
                        * ((center[axis] - source.viewport_center[axis])
                            / source.viewport_center[axis])
                });
                Piece {
                    points,
                    uv,
                    center,
                    velocity,
                    angle: 0.,
                    angular_velocity,
                }
            })
            .collect();
        Ok(Self {
            pieces,
            timer: 0,
            active: true,
            break_sound,
            viewport: source.viewport,
            radians_per_degree: source.radians_per_degree,
            secondary_rotation_scale: source.secondary_rotation_scale,
            draw_depth: source.draw_depth,
            // 5C38 selects mode 0, then 11864 clears its initial alpha.
            fade: TransitionFrame {
                color: fade_color,
                alpha: 0,
                wipe_rows: Vec::new(),
                wipe_progress: 0,
            },
        })
    }

    /// 56A8 sets alpha 255 when loading finishes; 40C8 leaves it unchanged.
    pub fn begin_camera_fade(&mut self) {
        self.fade.alpha = 255;
    }

    /// 3EA4 calls 117E8/117A0 before its world visit. The screen-break timer
    /// has already advanced throughout loading and cannot own this clock.
    pub fn advance_camera_fade(&mut self) {
        self.fade.alpha = self.fade.alpha.saturating_sub(16);
    }

    /// 11940(1) draws this over battle/HUD, before 6184 draws the field pieces.
    pub fn fade(&self) -> Option<&TransitionFrame> {
        (self.fade.alpha != 0).then_some(&self.fade)
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub fn timer(&self) -> u16 {
        self.timer
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn viewport(&self) -> [f32; 2] {
        self.viewport
    }

    pub fn draw_depth(&self) -> f32 {
        self.draw_depth
    }

    /// Y and Z rotations, in radians. BBA0 concatenates Z * Y.
    pub fn rotation(&self, piece: &Piece) -> [f32; 2] {
        [
            self.radians_per_degree * piece.angle,
            self.radians_per_degree * (self.secondary_rotation_scale * piece.angle),
        ]
    }

    pub fn alpha(&self) -> u8 {
        match self.timer {
            0..57 => 255,
            57..120 => (255 - ((self.timer - 57) << 2)) as u8,
            _ => 0,
        }
    }

    /// Called once per admitted source update, including camera entry/combat.
    /// The ordinary loader becoming ready does not retire this transition.
    pub fn advance(&mut self, paused: bool) -> Option<SoundBinding> {
        if paused || !self.active {
            return None;
        }
        let sound = (self.timer == 30).then_some(self.break_sound);
        if self.timer >= 30 {
            for piece in &mut self.pieces {
                piece.center[0] += piece.velocity[0];
                piece.center[1] += piece.velocity[1];
                piece.angle += piece.angular_velocity;
            }
            if self.timer >= 180 {
                self.active = false;
            }
        }
        self.timer += 1;
        sound
    }
}

#[cfg(test)]
mod tests;
