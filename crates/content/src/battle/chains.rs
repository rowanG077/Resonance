//! Source rules for cancelling one martial arte into another.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MartialChains {
    pub artes: BTreeMap<u16, ChainArte>,
    pub colors: [[u8; 3]; 3],
    pub ground_height: f32,
    pub aerial_height: f32,
    pub regal_aerial_height: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ChainArte {
    pub upgrades: [u16; 2],
    pub airborne: bool,
    pub element: ChainElement,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "element", rename_all = "snake_case")]
pub enum ChainElement {
    /// Third-tier feedback uses the equipped element; second-tier feedback stays neutral.
    Inherit,
    Neutral,
    Element(crate::menu_data::Element),
}

impl MartialChains {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.ground_height.is_finite()
                && self.ground_height >= 0.
                && self.aerial_height.is_finite()
                && self.aerial_height > self.ground_height
                && self.regal_aerial_height.is_finite()
                && self.regal_aerial_height > self.aerial_height,
            "invalid martial chain heights"
        );
        ensure!(
            self.artes.iter().all(
                |(&id, row)| usize::from(id) < crate::menu_data::TECHNIQUE_COUNT
                    && row
                        .upgrades
                        .iter()
                        .all(|&id| usize::from(id) < crate::menu_data::TECHNIQUE_COUNT)
            ),
            "invalid martial chain arte"
        );
        Ok(())
    }
}
