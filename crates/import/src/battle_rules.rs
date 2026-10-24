//! Prepare shared battle rules directly from the native executable and tables.
use anyhow::Result;
use resonance_content::battle_rules::Captured;
use std::{fs, path::Path};

pub fn read(extracted: &Path) -> Result<Captured> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let sources = crate::source_assets::Sources::read_with(extracted, &executable)?;
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let module = crate::rel::Rel::read(&extracted.join("files").join(&sources.module))?;
    from_sources(&executable, &usual, &module)
}

pub(crate) fn from_sources(
    executable: &[u8],
    usual: &[u8],
    module: &crate::rel::Rel,
) -> Result<Captured> {
    Ok(Captured {
        formations: crate::battle_formation::read(usual)?,
        party: crate::battle_profile::read_party(module, usual)?,
        defense: crate::battle_defense_system::read(module)?,
        unison: crate::battle_unison::read(executable, module)?,
        projectiles: crate::battle_projectile::read_resident(usual, module)?,
        recoil: crate::battle_recoil::read(module)?,
        techniques: crate::arte::read(executable)?.catalogue,
    })
}
