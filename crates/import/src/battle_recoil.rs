//! Shared hit response: 6124C impulses/scalars and 1A9AC guard preferences.
use crate::{read::Field, rel::Rel};
use anyhow::{Context, Result};
use resonance_content::{battle_recoil::Table, source::FloatOperand};
use std::path::Path;

fn read(module: &Rel) -> Result<Table> {
    let pairs = module
        .at((5, 0x5600))?
        .get(..19 * 8)
        .context("truncated recoil table")?;
    let constants = module.at((4, 0x3a58))?;
    Ok(Table {
        source_sha256: crate::digest(&module.bytes),
        impulses: pairs
            .chunks_exact(8)
            .map(|pair| <[FloatOperand; 2]>::read(pair, 0))
            .collect::<Result<_>>()?,
        suppression_distance: FloatOperand::read(constants, 0)?,
        light_vertical_scale: FloatOperand::read(constants, 8)?,
        heavy_vertical_scale: FloatOperand::read(constants, 12)?,
        guard_speed: FloatOperand::read(constants, 16)?,
        default_guard_preferences: module
            .at((4, 0x10ef))?
            .get(..11)
            .context("truncated guard preferences")?
            .to_vec(),
    })
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
    fn preserves_nonfinite_and_signed_zero_source_operands_and_rejects_truncation() -> Result<()> {
        let mut module = Rel {
            bytes: vec![0; 1 + 0x5600 + 19 * 8],
            sections: vec![(1, 0x5600 + 19 * 8); 6],
            pointers: Default::default(),
            local_targets: Default::default(),
        };
        let preferences = [0, 1, 2, 5, 5, 1, 2, 1, 1, 2, 0];
        module.bytes[1 + 0x10ef..1 + 0x10ef + preferences.len()].copy_from_slice(&preferences);
        for (i, bits) in [0x7fc12345_u32, 0x80000000, 0xff800000]
            .into_iter()
            .enumerate()
        {
            module.bytes[1 + 0x5600 + i * 4..1 + 0x5604 + i * 4]
                .copy_from_slice(&bits.to_be_bytes());
        }
        let table: Table = serde_json::from_slice(&serde_json::to_vec(&read(&module)?)?)?;
        assert_eq!(table.impulses[0][0].bits(), 0x7fc12345);
        assert_eq!(table.impulses[0][1].bits(), 0x80000000);
        assert_eq!(table.impulses[1][0].bits(), 0xff800000);
        assert_eq!(table.default_guard_preferences, preferences);
        module.sections[5].1 -= 1;
        assert!(read(&module).is_err());
        module.sections[5].1 += 1;
        module.sections[4].1 = 0x3a68 + 3;
        assert!(read(&module).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted original discs"]
    fn original_recoil_table_roundtrips_and_keeps_variant_publications_separate() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("battle-recoil"));
        let result = (|| -> Result<()> {
            let mut previous = None;
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/files/US_r_Top2Btl.rel"));
                let module = Rel::read(&file)?;
                let path = publish(&file, &output, "battle")?;
                assert_eq!(path, resonance_content::battle_recoil::PATH);
                let json = std::fs::read(output.join(&path))?;
                let variant = publish(&file, &output, "battle/variants/source")?;
                assert_eq!(std::fs::read(output.join(variant))?, json);
                let table: Table = serde_json::from_slice(&json)?;
                assert_eq!(
                    table.default_guard_preferences,
                    module.at((4, 0x10ef))?[..11]
                );
                let bytes: Vec<_> = table
                    .impulses
                    .iter()
                    .flatten()
                    .flat_map(|v| v.bits().to_be_bytes())
                    .collect();
                assert_eq!(bytes, module.at((5, 0x5600))?[..19 * 8]);
                for (value, offset) in [
                    (table.suppression_distance, 0x3a58),
                    (table.light_vertical_scale, 0x3a60),
                    (table.heavy_vertical_scale, 0x3a64),
                    (table.guard_speed, 0x3a68),
                ] {
                    assert_eq!(value.bits().to_be_bytes(), module.at((4, offset))?[..4]);
                }
                if let Some(previous) = &previous {
                    assert_eq!(&json, previous);
                }
                previous = Some(json);
            }
            Ok(())
        })();
        if output.exists() {
            std::fs::remove_dir_all(output)?;
        }
        result
    }
}
