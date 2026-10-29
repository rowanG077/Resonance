use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// A bounded continuous range, or evenly spaced choices when `step` is positive.
/// Fixed settings have equal endpoints and consume no randomness.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredRange", into = "StoredRange")]
pub struct ValueRange {
    pub min: f32,
    pub max: f32,
    pub step: f32,
    /// Prepared programs use absolute sample indices. Editable programs use
    /// birth-local indices; negative values refer to earlier samples.
    pub draw: Option<i64>,
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

/// Compact authoring syntax: a fixed number or an explicit varying range.
#[derive(Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum StoredRange {
    Fixed(f32),
    Range {
        min: f32,
        max: f32,
        #[serde(default, skip_serializing_if = "is_zero")]
        step: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draw: Option<i64>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reverse: bool,
    },
}

impl From<ValueRange> for StoredRange {
    fn from(value: ValueRange) -> Self {
        if value.min.to_bits() == value.max.to_bits()
            && value.step == 0.
            && value.draw.is_none()
            && !value.reverse
        {
            Self::Fixed(value.min)
        } else {
            Self::Range {
                min: value.min,
                max: value.max,
                step: value.step,
                draw: value.draw,
                reverse: value.reverse,
            }
        }
    }
}

impl From<StoredRange> for ValueRange {
    fn from(value: StoredRange) -> Self {
        match value {
            StoredRange::Fixed(value) => Self::fixed(value),
            StoredRange::Range {
                min,
                max,
                step,
                draw,
                reverse,
            } => Self {
                min,
                max,
                step,
                draw,
                reverse,
            },
        }
    }
}
