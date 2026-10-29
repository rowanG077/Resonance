//! Ordinary casting gestures with only actor-specific differences in JSON.
use crate::battle_action::ActionPose;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Casting {
    /// Chant duration before technique adjustments and explicit enemy overrides.
    #[serde(skip_serializing_if = "base_ticks")]
    pub base_ticks: i16,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub chant_once: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub chant_loop_start: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(with = "crate::battle_motion::optional_byte")]
    pub recovery_motion: Option<u8>,
    #[serde(skip_serializing_if = "opening_clip")]
    #[serde(with = "crate::battle_motion")]
    pub opening_clip: u16,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub opening_start: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opening_end: Option<f32>,
    #[serde(skip_serializing_if = "ordinary_rate")]
    pub opening_rate: f32,
    #[serde(skip_serializing_if = "ordinary_blend")]
    pub opening_blend: u8,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub opening_repeat: bool,
    #[serde(skip_serializing_if = "release_clip")]
    #[serde(with = "crate::battle_motion")]
    pub release_clip: u16,
    #[serde(skip_serializing_if = "crate::defaults::is_unit")]
    pub release_start: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_end: Option<f32>,
    #[serde(skip_serializing_if = "ordinary_rate")]
    pub release_rate: f32,
    #[serde(skip_serializing_if = "ordinary_blend")]
    pub release_blend: u8,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub release_repeat: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub release_loop_start: f32,
}

fn base_ticks(value: &i16) -> bool {
    *value == 120
}
fn opening_clip(value: &u16) -> bool {
    *value == 13
}
fn release_clip(value: &u16) -> bool {
    *value == 12
}
fn ordinary_rate(value: &f32) -> bool {
    *value == 0.5
}
fn ordinary_blend(value: &u8) -> bool {
    *value == 4
}

impl Default for Casting {
    fn default() -> Self {
        Self {
            base_ticks: 120,
            chant_once: false,
            chant_loop_start: 0.,
            recovery_motion: None,
            opening_clip: 13,
            opening_start: 0.,
            opening_end: None,
            opening_rate: 0.5,
            opening_blend: 4,
            opening_repeat: false,
            release_clip: 12,
            release_start: 1.,
            release_end: None,
            release_rate: 0.5,
            release_blend: 4,
            release_repeat: false,
            release_loop_start: 0.,
        }
    }
}

impl Casting {
    pub fn opening(self) -> ActionPose {
        ActionPose {
            clip: self.opening_clip,
            start: self.opening_start,
            end: self.opening_end,
            rate: self.opening_rate,
            blend: self.opening_blend,
            repeat: self.opening_repeat,
        }
    }
    pub fn release(self) -> ActionPose {
        ActionPose {
            clip: self.release_clip,
            start: self.release_start,
            end: self.release_end,
            rate: self.release_rate,
            blend: self.release_blend,
            repeat: self.release_repeat,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_casting_keeps_release_zero_distinct_from_the_default() -> anyhow::Result<()> {
        let defaults: Casting = serde_json::from_str("{}")?;
        assert_eq!(serde_json::to_string(&defaults)?, "{}");
        let edited: Casting =
            serde_json::from_str(r#"{"release_start":0.0,"opening_repeat":true}"#)?;
        assert_eq!(edited.release().start, 0.);
        assert_eq!(defaults.release().start, 1.);
        assert_eq!(edited.release().clip, defaults.release().clip);
        assert!(edited.opening().repeat);
        let document = serde_json::to_value(edited)?;
        assert_eq!(document.as_object().unwrap().len(), 2);
        assert_eq!(document["release_start"], 0.0);
        assert!(serde_json::from_str::<Casting>(r#"{"opening":{"clip":"cast-opening"}}"#).is_err());
        Ok(())
    }

    #[test]
    fn custom_casting_keeps_motion_ranges_rates_and_recovery_editable() -> anyhow::Result<()> {
        let text = r#"{"base_ticks":0,"recovery_motion":"slot-39","chant_once":true,"chant_loop_start":6.5,
            "opening_clip":"slot-55","opening_start":3.0,"opening_end":19.0,"opening_rate":1.0,"opening_blend":0,
            "release_clip":"slot-68","release_start":2.0,"release_end":18.0,"release_rate":2.0,"release_blend":0,
            "release_repeat":true,"release_loop_start":7.0}"#;
        let authored: Casting = serde_json::from_str(text)?;
        let restored: Casting = serde_json::from_slice(&serde_json::to_vec(&authored)?)?;
        assert_eq!(authored, restored);
        let opening = restored.opening();
        let release = restored.release();
        assert_eq!(
            (
                opening.clip,
                opening.start,
                opening.end,
                opening.rate,
                opening.blend
            ),
            (55, 3., Some(19.), 1., 0)
        );
        assert_eq!(
            (
                release.clip,
                release.start,
                release.end,
                release.rate,
                release.blend,
                release.repeat
            ),
            (68, 2., Some(18.), 2., 0, true)
        );
        assert_eq!(restored.base_ticks, 0);
        assert_eq!(restored.recovery_motion, Some(39));
        Ok(())
    }
}
