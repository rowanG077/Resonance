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
    /// Change one earlier birth in this emitter; sound filtering cannot change
    /// the ordinal among spawn operations.
    SetMotion {
        spawn: usize,
        #[serde(flatten)]
        motion: Box<MotionChange>,
    },
    Sound {
        id: u16,
        priority: u8,
    },
    Shake {
        duration: u32,
        amplitude: u32,
    },
    StageTint {
        color: [u8; 3],
        duration: u16,
    },
    Notice {
        text: String,
    },
    Unsupported {
        reason: String,
    },
}

/// Motion changes applied to one retained particle.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionChange {
    #[serde(default)]
    pub offset: [Option<f32>; 3],
    #[serde(default)]
    pub size: [Option<f32>; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<ModelAnimation>,
    pub velocity: [Option<f32>; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity_add: Option<[f32; 3]>,
    #[serde(default)]
    pub acceleration: [Option<f32>; 3],
    #[serde(default)]
    pub angular_velocity: [Option<f32>; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_motion: Option<ForwardMotion>,
    /// Multiply the current velocity after any explicit assignment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity_scale: Option<[f32; 3]>,
    pub size_velocity: [Option<f32>; 3],
    pub size_acceleration: [Option<f32>; 3],
}

/// Assign world motion along the particle's retained emitter heading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForwardMotion {
    pub velocity: Option<f32>,
    pub acceleration: Option<f32>,
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
    /// Shared unit draw within this emitter's schedule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw: Option<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reverse: bool,
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
            draw: None,
            reverse: false,
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
        let delta = if self.step > 0. {
            (unit * self.choice_count()).floor() * self.step
        } else {
            unit * (self.max - self.min)
        };
        if self.reverse {
            self.max - delta
        } else {
            self.min + delta
        }
    }

    /// Inclusive choices, allowing the rounding of a representable endpoint.
    pub fn choice_count(self) -> f32 {
        debug_assert!(self.step > 0.);
        let intervals = (self.max - self.min) / self.step;
        let nearest = intervals.round();
        let endpoint = self.min + nearest * self.step;
        let rounding = self.min.abs().max(self.max.abs()) * f32::EPSILON;
        let intervals = if (endpoint - self.max).abs() <= rounding {
            nearest
        } else {
            intervals.floor()
        };
        intervals + 1.
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolarSpread {
    pub angles: [f32; 3],
    pub radius: f32,
    pub radius_jitter: f32,
    pub angle_jitter: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius_draw: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_draw: Option<u32>,
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
fn vertices_empty(values: &[[Option<ValueRange>; 3]; 3]) -> bool {
    values.iter().all(empty)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelParent {
    pub spawn: usize,
    pub joint: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAnimation {
    pub model: u8,
    pub clip: u16,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frame: f32,
    pub blend_ticks: u8,
    pub rate: f32,
    pub repeat: bool,
}
impl ModelAnimation {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.rate.is_finite() && (self.rate == 0. || (0.01..=4.).contains(&self.rate)),
            "invalid effect model playback rate"
        );
        ensure!(
            self.frame.is_finite() && self.frame >= 0.,
            "invalid effect model initial frame"
        );
        Ok(())
    }
}

/// A particle setting whose birth value contains independent shared samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "axis", rename_all = "snake_case")]
pub enum BirthField {
    Offset(u8),
    Velocity(u8),
    Size(u8),
    Uv(u8),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BirthSum {
    pub field: BirthField,
    /// Ordered additions; negative ranges represent subtraction.
    pub terms: Vec<ValueRange>,
}

/// Final birth settings. Absent fields retain the prepared particle template.
/// Ranges reference explicit shared draws; unrelated settings vary independently.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParticleBirth {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sums: Vec<BirthSum>,
    /// Sample this owner body bone's world position once when the particle is born.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_bone: Option<u8>,
    /// Replace the bone of an existing attachment without changing its mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_bone: Option<u8>,
    /// New unit draws consumed in authored order before applying this birth.
    #[serde(skip_serializing_if = "is_zero_count")]
    pub random_draws: u32,
    #[serde(skip_serializing_if = "empty")]
    pub offset: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub velocity: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub acceleration: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub angles: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub angular_velocity: [Option<ValueRange>; 3],
    #[serde(skip_serializing_if = "empty")]
    pub segment_offset: [Option<ValueRange>; 3],
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
    /// Direct edits to the first three authored quad vertices.
    #[serde(skip_serializing_if = "vertices_empty")]
    pub vertices: [[Option<ValueRange>; 3]; 3],
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
    pub lifetime: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fade_from: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry_count: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect_group: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_palette: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ValueRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_parent: Option<ModelParent>,
    /// Start a prepared clip at birth, optionally holding a selected frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation: Option<ModelAnimation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_spread: Option<PolarSpread>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity_spread: Option<PolarSpread>,
    /// Emitter heading contribution to the prepared yaw.
    #[serde(skip_serializing_if = "is_zero")]
    pub yaw_heading: f32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cull_back: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub element_tint: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ribbon_depth: bool,
}
fn is_zero_count(value: &u32) -> bool {
    *value == 0
}
impl ParticleBirth {
    pub fn ranges(&self) -> impl Iterator<Item = &ValueRange> {
        self.offset
            .iter()
            .chain(&self.velocity)
            .chain(&self.acceleration)
            .chain(&self.angles)
            .chain(&self.angular_velocity)
            .chain(&self.segment_offset)
            .chain(&self.orbit)
            .chain(&self.orbit_velocity)
            .chain(&self.size)
            .chain(&self.size_velocity)
            .chain(&self.size_acceleration)
            .chain(self.vertices.iter().flatten())
            .chain(&self.color)
            .chain(&self.end_color)
            .chain(&self.uv)
            .chain(&self.brighten)
            .chain(&self.fade)
            .chain([&self.segment_angle_step])
            .chain([&self.brighten_until])
            .chain([&self.lifetime])
            .chain([&self.fade_from])
            .chain([&self.geometry_count])
            .chain([&self.phase])
            .chain([&self.palette])
            .chain([&self.alpha_palette])
            .chain([&self.model])
            .flatten()
            .chain(self.sums.iter().flat_map(|sum| &sum.terms))
    }
    pub fn draw_references(&self) -> impl Iterator<Item = u32> + '_ {
        self.ranges().filter_map(|range| range.draw).chain(
            [self.offset_spread, self.velocity_spread]
                .into_iter()
                .flatten()
                .flat_map(|spread| [spread.radius_draw, spread.angle_draw])
                .flatten(),
        )
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.yaw_heading.is_finite(), "nonfinite particle heading");
        let mut fields = std::collections::BTreeSet::new();
        for sum in &self.sums {
            let (axis, values) = match sum.field {
                BirthField::Offset(axis) => (axis, &self.offset[..]),
                BirthField::Velocity(axis) => (axis, &self.velocity[..]),
                BirthField::Size(axis) => (axis, &self.size[..]),
                BirthField::Uv(axis) => (axis, &self.uv[..]),
            };
            ensure!(
                fields.insert(sum.field)
                    && sum.terms.len() >= 2
                    && values.get(usize::from(axis)).is_some_and(Option::is_none)
                    && sum
                        .terms
                        .iter()
                        .all(|term| term.draw.is_some() || term.min == term.max),
                "invalid or overlapping particle sum"
            );
            let (low, high) = sum.terms.iter().fold((0., 0.), |(low, high), term| {
                (low + term.min, high + term.max)
            });
            ensure!(
                low.is_finite() && high.is_finite(),
                "unbounded particle sum"
            );
            if matches!(sum.field, BirthField::Uv(_)) {
                ensure!(
                    low >= f32::from(i16::MIN)
                        && high <= f32::from(i16::MAX)
                        && sum.terms.iter().all(|term| term.min.fract() == 0.
                            && term.max.fract() == 0.
                            && term.step.fract() == 0.),
                    "particle UV sum requires bounded integer choices"
                );
            }
        }
        ensure!(
            self.effect_group.is_none_or(|group| group < 16)
                && (self.effect_group.is_none() || self.attachment_bone.is_none()),
            "invalid particle effect group"
        );
        for value in self.ranges() {
            value.validate()?;
        }
        for (range, maximum) in [
            (self.lifetime, i16::MAX as f32 + 1.),
            (self.fade_from, 255.),
        ] {
            if let Some(range) = range {
                ensure!(
                    range.min >= 0.
                        && range.max <= maximum
                        && range.min.fract() == 0.
                        && range.max.fract() == 0.
                        && (range.min == range.max
                            || (range.step >= 1.
                                && range.step.fract() == 0.
                                && (range.max - range.min) % range.step == 0.)),
                    "particle duration requires bounded integer choices"
                );
            }
        }
        ensure!(
            self.animation.is_none() || self.model.is_some(),
            "model animation start has no model selection"
        );
        if let Some(animation) = self.animation {
            animation.validate()?;
            ensure!(
                animation.blend_ticks == 0,
                "model birth cannot blend a previous pose"
            );
        }
        if let Some(model) = self.model {
            ensure!(
                model.min >= 0.
                    && model.max <= 255.
                    && model.min.fract() == 0.
                    && model.max.fract() == 0.
                    && (model.min == model.max
                        || (model.step >= 1.
                            && model.step.fract() == 0.
                            && (model.max - model.min) % model.step == 0.)),
                "particle model requires discrete byte-sized choices"
            );
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
