//! Authored placement and presentation for the shared team-attack sequence.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    pub placement: [[f32; 3]; 4],
    pub minimum_distance: f32,
    pub short_weapon_penalty: f32,
    pub restore_height: f32,
    pub windup_rate: f32,
    pub windup_color: [u8; 3],
    pub prelude: Prelude,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prelude {
    pub first_x: f32,
    pub spacing: f32,
    pub camera_eye: [f32; 3],
    pub camera_target: [f32; 3],
    pub color: [u8; 3],
}

impl Host {
    pub fn validate(&self) -> Result<()> {
        let prelude = &self.prelude;
        ensure!(
            self.placement
                .iter()
                .flatten()
                .chain(&prelude.camera_eye)
                .chain(&prelude.camera_target)
                .chain([
                    &self.minimum_distance,
                    &self.short_weapon_penalty,
                    &self.restore_height,
                    &self.windup_rate,
                    &prelude.first_x,
                    &prelude.spacing,
                ])
                .all(|value| value.is_finite())
                && self.minimum_distance > 0.
                && self.short_weapon_penalty >= 0.
                && self.restore_height >= 0.
                && self.windup_rate > 0.
                && prelude.spacing > 0.
                && prelude.camera_eye != prelude.camera_target,
            "invalid Unison host metadata"
        );
        Ok(())
    }
}
