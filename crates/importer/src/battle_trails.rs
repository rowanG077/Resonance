//! Shared, editable names for native battle ribbon materials.
//!
//! A ribbon's material lives in its weapon or enemy package, so there is no
//! original palette to extract. This maintained list gives recurring native
//! materials one name; a material not in it can be written inline by its owner.
use anyhow::{Result, ensure};
use resonance_content::battle_model::TrailMaterial;
use std::{collections::BTreeMap, path::Path};

pub fn cook_battle_trails(_root: &Path, library: &Path) -> Result<()> {
    let materials: BTreeMap<String, TrailMaterial> =
        serde_json::from_str(include_str!("battle-trail-materials.json"))?;
    ensure!(materials.len() == 44, "incomplete battle trail palette");
    ensure!(
        materials.values().all(|material| material
            .color
            .iter()
            .all(|value| value.is_finite() && *value >= 0.)),
        "invalid battle trail color"
    );
    let output = library.join("battle/trails.json");
    ensure!(!output.exists(), "battle trail palette already exists");
    crate::output::json(&output, &materials)?;
    Ok(())
}
