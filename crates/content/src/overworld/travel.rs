use super::{Position, World};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mount {
    Foot,
    Noishe,
    Rheairds,
    Ship,
}
impl Mount {
    pub fn long_range(self) -> bool {
        self != Self::Foot
    }
    pub fn airborne(self) -> bool {
        self == Self::Rheairds
    }
}

/// World HUD display modes, in the original Start-button cycle order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapDisplay {
    #[default]
    Small,
    Full,
    Hidden,
}
impl MapDisplay {
    pub fn next(self) -> Self {
        match self {
            Self::Small => Self::Full,
            Self::Full => Self::Hidden,
            Self::Hidden => Self::Small,
        }
    }
    pub fn opacity(self) -> [u8; 2] {
        match self {
            Self::Small => [255, 0],
            Self::Full => [0, 255],
            Self::Hidden => [0, 0],
        }
    }
}

/// Stable travel data retained while a field, battle or save owns the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TravelState {
    pub world: World,
    pub position: Position,
    /// Terrain heading and camera yaw in radians.
    pub heading: f32,
    pub camera_yaw: f32,
    pub alternate_perspective: bool,
    pub map_display: MapDisplay,
    pub mount: Mount,
    /// Flight display altitude, separate from the terrain underneath.
    pub altitude: f32,
}
impl TravelState {
    pub fn validate_shape(&self) -> Result<()> {
        ensure!(
            [self.heading, self.camera_yaw]
                .into_iter()
                .all(|v| v.is_finite() && (0.0..TAU).contains(&v))
                && self.altitude.is_finite(),
            "invalid saved world heading or altitude"
        );
        Ok(())
    }
}
