//! Actor weight and guard recovery parameters.
use serde::{Deserialize, Serialize};

pub const PATH: &str = "battle/recoil.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub source_sha256: String,
    pub light_vertical_scale: f32,
    pub heavy_vertical_scale: f32,
    /// Recovery bonuses for the default saved party strategy, indexed by actor kind.
    pub default_guard_recovery_bonuses: Vec<u8>,
}

/// Guard recovery contributed by a resolved position strategy.
pub fn strategy_guard_recovery(position: u8) -> u8 {
    match position {
        1 => 0,
        2 => 5,
        3 => 25,
        _ => 10,
    }
}
