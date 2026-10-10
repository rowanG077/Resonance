//! Shared hit-response parameters and guard recovery bonuses.
use crate::{read::Field, rel::Rel};
use anyhow::{Context, Result};
use resonance_content::battle_recoil::{Table, strategy_guard_recovery};
use std::path::Path;

fn read(module: &Rel) -> Result<Table> {
    let constants = module.at((4, 0x3a58))?;
    Ok(Table {
        source_sha256: crate::digest(&module.bytes),
        light_vertical_scale: f32::read(constants, 8)?,
        heavy_vertical_scale: f32::read(constants, 12)?,
        default_guard_recovery_bonuses: guard_defaults(module)?,
    })
}

pub(crate) fn guard_defaults(module: &Rel) -> Result<Vec<u8>> {
    Ok(module
        .at((4, 0x10ef))?
        .get(..11)
        .context("truncated guard preferences")?
        .iter()
        .map(|&position| strategy_guard_recovery(position))
        .collect())
}

pub fn publish(file: &Path, output: &Path, prefix: &str) -> Result<String> {
    let table = read(&Rel::read(file)?)?;
    let path = format!("{prefix}/recoil.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&table)?)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_weight_and_guard_preferences_and_rejects_invalid_input() -> Result<()> {
        let mut module = Rel {
            bytes: vec![0; 1 + 0x3a68],
            sections: vec![(1, 0x3a68); 5],
            pointers: Default::default(),
            local_targets: Default::default(),
        };
        let preferences = [0, 1, 2, 5, 5, 1, 2, 1, 1, 2, 0];
        module.bytes[1 + 0x10ef..1 + 0x10ef + preferences.len()].copy_from_slice(&preferences);
        let table = read(&module)?;
        assert_eq!(
            table.default_guard_recovery_bonuses,
            [10, 0, 5, 10, 10, 0, 5, 0, 0, 5, 10]
        );
        module.bytes[1 + 0x3a60..1 + 0x3a64].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(read(&module).is_err());
        module.bytes[1 + 0x3a60..1 + 0x3a64].fill(0);
        module.sections[4].1 -= 1;
        assert!(read(&module).is_err());
        Ok(())
    }
}
