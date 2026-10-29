//! Native audio selections and shared battle spatial controls.
pub mod document;
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub sounds: BTreeSet<u16>,
    pub streams: BTreeSet<u16>,
    #[serde(default)]
    pub voices: BTreeSet<u32>,
    pub music: BTreeSet<u16>,
}

pub fn spatial_controls(controls: [f32; 5]) -> Result<[f32; 5]> {
    let [origin, divisor, center, low, high] = controls;
    ensure!(
        controls.iter().all(|v| v.is_finite())
            && origin > 0.
            && divisor > 0.
            && low >= 0.
            && low <= center
            && center <= high
            && high <= 127.,
        "invalid battle spatial controls"
    );
    Ok(controls)
}
