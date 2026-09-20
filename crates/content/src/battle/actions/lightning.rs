//! Ground-targeted lightning spells share placement while retaining distinct pulse schedules.
use super::{AnimationCommand, HitRule, StoredSpellPresentation};
use crate::battle::effects::{EffectBank, EffectId};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightningKind {
    Lightning,
    SparkWave,
    Indignation,
    ThunderBlade,
}

impl LightningKind {
    pub const fn native(self) -> u16 {
        match self {
            Self::Lightning => 216,
            Self::SparkWave => 217,
            Self::Indignation => 218,
            Self::ThunderBlade => 219,
        }
    }
    pub const fn bank(self) -> EffectBank {
        match self {
            Self::Lightning => EffectBank::Techniques,
            _ => EffectBank::Magic(self.native() - 200),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GroundSpellOrigin {
    pub height: f32,
    pub nudge: f32,
    pub direction_threshold: f32,
}

impl GroundSpellOrigin {
    /// The target's root is captured once and nudged horizontally toward the caster.
    pub fn capture(self, caster: [f32; 3], target: [f32; 3]) -> [f32; 3] {
        let delta = self.toward(target, caster, self.nudge);
        [target[0] + delta[0], self.height, target[2] + delta[2]]
    }
    pub fn toward(self, origin: [f32; 3], target: [f32; 3], speed: f32) -> [f32; 3] {
        let delta = [target[0] - origin[0], target[2] - origin[2]];
        let length = delta[0].hypot(delta[1]);
        if length >= self.direction_threshold {
            [delta[0] / length * speed, 0., delta[1] / length * speed]
        } else {
            [0.; 3]
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LightningPulse {
    pub tick: u16,
    pub projectile: EffectId,
    pub rule: HitRule,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SpellTracking {
    pub first_tick: u16,
    pub end_tick: u16,
    pub speed: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredLightning {
    pub resume: AnimationCommand,
    pub presentation: StoredSpellPresentation,
    pub effect: EffectId,
    pub scale: f32,
    pub tracking: Option<SpellTracking>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightningRecipe {
    pub kind: LightningKind,
    pub lifetime: u16,
    pub origin: GroundSpellOrigin,
    pub pulses: Vec<LightningPulse>,
    pub stored: Option<StoredLightning>,
}

impl LightningRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 0
                && self.origin.height.is_finite()
                && self.origin.nudge.is_finite()
                && self.origin.nudge >= 0.
                && self.origin.direction_threshold.is_finite()
                && self.origin.direction_threshold > 0.
                && !self.pulses.is_empty()
                && self.pulses.windows(2).all(|p| p[0].tick < p[1].tick),
            "invalid lightning origin or schedule"
        );
        for pulse in &self.pulses {
            ensure!(
                pulse.tick < self.lifetime && pulse.projectile.bank == self.kind.bank(),
                "invalid lightning projectile pulse"
            );
            pulse.rule.impact_program_from(
                EffectBank::Techniques,
                match self.kind.bank() {
                    EffectBank::Magic(id) => Some(id),
                    _ => None,
                },
            )?;
        }
        ensure!(
            self.stored.is_some() == (self.kind != LightningKind::Lightning),
            "invalid lightning lifecycle"
        );
        if let Some(stored) = &self.stored {
            ensure!(
                self.lifetime > 45
                    && stored.effect.bank == self.kind.bank()
                    && stored.presentation.color[3] == 255
                    && stored.presentation.camera_distance.is_finite()
                    && stored.presentation.camera_distance >= 0.
                    && (0. ..90.).contains(&stored.presentation.camera_elevation)
                    && stored.scale.is_finite()
                    && stored.scale > 0.
                    && matches!(stored.resume, AnimationCommand::Play {clip:12,rate,..} if rate.is_finite() && rate > 0.)
                    && stored.tracking.is_some() == (self.kind == LightningKind::SparkWave),
                "invalid stored lightning presentation"
            );
            if let Some(tracking) = stored.tracking {
                ensure!(
                    self.pulses.len() == 1
                        && tracking.first_tick > self.pulses[0].tick
                        && tracking.first_tick < tracking.end_tick
                        && tracking.end_tick <= self.lifetime
                        && tracking.speed.is_finite()
                        && tracking.speed > 0.,
                    "invalid lightning tracking window"
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ground_capture_and_tracking_ignore_height_without_overshoot_clamping() {
        let origin = GroundSpellOrigin {
            height: 200.,
            nudge: 1.,
            direction_threshold: 0.5,
        };
        assert_eq!(
            origin.capture([3., 100., 4.], [0., 90., 0.]),
            [0.6, 200., 0.8]
        );
        assert_eq!(origin.capture([0.3, 0., 0.], [0.; 3]), [0., 200., 0.]);
        assert_eq!(
            origin.toward([0., 200., 0.], [0.5, 0., 0.], 1.25),
            [1.25, 0., 0.]
        );
        assert_eq!(origin.toward([0., 200., 0.], [0.49, 0., 0.], 1.25), [0.; 3]);
    }
}
