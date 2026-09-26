//! Shared hit-response and recovery parameters.
use crate::source::FloatOperand;
use serde::{Deserialize, Serialize};

pub const PATH: &str = "battle/recoil.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub source_sha256: String,
    /// Forward and vertical speeds, selected by the contact shape's recoil kind.
    pub impulses: Vec<[FloatOperand; 2]>,
    pub suppression_distance: FloatOperand,
    pub light_vertical_scale: FloatOperand,
    pub heavy_vertical_scale: FloatOperand,
    pub guard_speed: FloatOperand,
    /// Actor-kind defaults used when the guard preference is zero (1A9AC).
    pub default_guard_preferences: Vec<u8>,
}
