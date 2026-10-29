//! Actor weight and guard recovery parameters.
use serde::Serialize;

pub const PATH: &str = "battle/recoil.json";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Table {
    pub impulses: Vec<[f32; 2]>,
    pub suppression_distance: f32,
    pub guard_speed: f32,
    pub light_vertical_scale: f32,
    pub heavy_vertical_scale: f32,
    /// Recovery bonuses for the default saved party strategy, keyed by party character ID.
    pub default_guard_recovery_bonuses: std::collections::BTreeMap<u8, u8>,
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
