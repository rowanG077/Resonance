//! Scheduled feedback with direct settings for each particle birth.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledEvent {
    pub at: u32,
    pub operation: EffectOperation,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectOperation {
    Spawn {
        particle: u8,
        blend: Option<u8>,
        palette: Option<u8>,
        #[serde(default)]
        birth: Box<ParticleBirth>,
    },
    Sound {
        id: u16,
        priority: u8,
    },
    Shake {
        duration: u32,
        amplitude: u32,
    },
    Unsupported {
        reason: String,
    },
}

/// A bounded continuous range, or evenly spaced choices when `step` is positive.
/// Fixed settings have equal endpoints and consume no randomness.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueRange {
    pub min: f32,
    pub max: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub step: f32,
}
fn is_zero(value: &f32) -> bool {
    *value == 0.
}
impl ValueRange {
    pub const fn fixed(value: f32) -> Self {
        Self {
            min: value,
            max: value,
            step: 0.,
        }
    }
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.min.is_finite()
                && self.max.is_finite()
                && self.min <= self.max
                && (self.max - self.min).is_finite()
                && self.step.is_finite()
                && self.step >= 0.
                && (self.step == 0. || ((self.max - self.min) / self.step).is_finite()),
            "invalid particle range"
        );
        Ok(())
    }
    /// `unit` is a cosmetic sample in [0, 1).
    pub fn sample(self, unit: f32) -> f32 {
        if self.step > 0. {
            self.min
                + (unit * (((self.max - self.min) / self.step).floor() + 1.)).floor() * self.step
        } else {
            self.min + unit * (self.max - self.min)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolarSpread {
    pub angles: [f32; 3],
    pub radius: f32,
    pub radius_jitter: f32,
    pub angle_jitter: f32,
}
impl PolarSpread {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.angles
                .into_iter()
                .chain([self.radius, self.radius_jitter, self.angle_jitter])
                .all(f32::is_finite)
                && self.radius_jitter >= 0.
                && self.angle_jitter >= 0.,
            "invalid particle spread"
        );
        Ok(())
    }
}

fn empty<const N: usize>(values: &[Option<ValueRange>; N]) -> bool {
    values.iter().all(Option::is_none)
}

/// Final birth settings. Absent fields retain the prepared particle template.
/// Position and angle axes vary independently. Size axes share one sample to
/// keep proportional dimensions together; emissions share no mutable samples.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParticleBirth {
    #[serde(skip_serializing_if = "empty")]
    pub offset: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub velocity: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub angles: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub angular_velocity: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub orbit: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub orbit_velocity: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub size: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub size_velocity: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub size_acceleration: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub color: [Option<ValueRange>; 4],
    #[serde(skip_serializing_if = "empty")]
    pub end_color: [Option<ValueRange>; 4],
    #[serde(skip_serializing_if = "empty")]
    pub uv: [Option<ValueRange>; 4],
    #[serde(skip_serializing_if = "empty")]
    pub brighten: [Option<ValueRange>; 4],
    #[serde(skip_serializing_if = "empty")]
    pub fade: [Option<ValueRange>; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment_angle_step: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brighten_until: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry_count: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_spread: Option<PolarSpread>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity_spread: Option<PolarSpread>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub relative_yaw: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cull_back: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub element_tint: bool,
}
impl ParticleBirth {
    pub fn validate(&self) -> Result<()> {
        for value in self
            .offset
            .iter()
            .chain(&self.velocity)
            .chain(&self.angles)
            .chain(&self.angular_velocity)
            .chain(&self.orbit)
            .chain(&self.orbit_velocity)
            .chain(&self.size)
            .chain(&self.size_velocity)
            .chain(&self.size_acceleration)
            .chain(&self.color)
            .chain(&self.end_color)
            .chain(&self.uv)
            .chain(&self.brighten)
            .chain(&self.fade)
            .chain([&self.segment_angle_step])
            .chain([&self.brighten_until])
            .chain([&self.geometry_count])
            .chain([&self.phase])
            .chain([&self.palette])
            .flatten()
        {
            value.validate()?;
        }
        for spread in [self.offset_spread, self.velocity_spread]
            .into_iter()
            .flatten()
        {
            spread.validate()?;
        }
        Ok(())
    }
}
