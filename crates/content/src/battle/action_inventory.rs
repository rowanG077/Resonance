//! Source coverage is independent of preparation, execution and presentation.
use super::{
    arte_inventory::ArteInventory, effect_inventory::EffectInventory,
    enemy_inventory::EnemyInventory, projectile_modifiers::ProjectileModifiers,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionInventory {
    pub version: u32,
    pub artes: ArteInventory,
    pub enemies: EnemyInventory,
    pub effects: EffectInventory,
    pub projectile_modifiers: ProjectileModifiers,
}

impl ActionInventory {
    pub const VERSION: u32 = 2;

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == Self::VERSION,
            "unsupported battle action inventory"
        );
        self.artes.validate()?;
        self.enemies.validate()?;
        self.effects.validate()?;
        self.projectile_modifiers.validate()?;
        Ok(())
    }
}
