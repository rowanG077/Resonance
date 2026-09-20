use anyhow::Result;
use resonance_content::battle::action_inventory::ActionInventory;
use std::path::Path;

pub(crate) fn recover(extracted: &Path) -> Result<ActionInventory> {
    let enemies = super::enemy_inventory::recover(extracted)?;
    let projectile_modifiers = super::projectile_modifiers::recover(extracted, &enemies)?;
    let inventory = ActionInventory {
        version: ActionInventory::VERSION,
        artes: super::arte_inventory::recover(extracted)?,
        enemies,
        effects: super::effect_inventory::recover(extracted)?,
        projectile_modifiers,
    };
    inventory.validate()?;
    Ok(inventory)
}
