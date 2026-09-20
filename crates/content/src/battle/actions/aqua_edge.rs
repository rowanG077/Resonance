use super::HitRule;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Aqua Edge owns three persistent flights independently of its caster's action.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AquaEdgeRecipe {
    pub lifetime: u16,
    pub rule: HitRule,
}

impl AquaEdgeRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime > 120,
            "Aqua Edge ends before retiring its flights"
        );
        Ok(())
    }
}
