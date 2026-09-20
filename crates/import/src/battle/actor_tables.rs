//! Complete item throw origins and the shared actor tint palette.
use super::{
    actions::Rel,
    embedded::{self, Layout},
};
use crate::read::f32 as float;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActorTables {
    /// Party kind minus one selects an actor-local xyz offset.
    pub(super) item_throw_origins: Vec<[f32; 3]>,
    /// Shared by recovery, items, status changes and spells; retain every slot.
    pub(super) tint_palette: [[u8; 4]; 12],
    /// Remaining storage before the next relocated table root.
    trailing_storage: Vec<u8>,
}

impl ActorTables {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            !self.item_throw_origins.is_empty() && self.trailing_storage.len() < 12,
            "invalid item throw origin extent"
        );
        ensure!(
            self.item_throw_origins
                .iter()
                .flatten()
                .all(|v| v.is_finite()),
            "nonfinite item throw origin"
        );
        Ok(())
    }
}

fn parse(origins: &[u8], colors: &[u8]) -> Result<ActorTables> {
    ensure!(colors.len() == 48, "expected twelve actor tints");
    let rows = origins.chunks_exact(12);
    let tables = ActorTables {
        trailing_storage: rows.remainder().to_vec(),
        item_throw_origins: rows
            .map(|row| Ok([float(row, 0)?, float(row, 4)?, float(row, 8)?]))
            .collect::<Result<_>>()?,
        tint_palette: colors.as_chunks::<4>().0.try_into()?,
    };
    tables.validate()?;
    Ok(tables)
}

pub(super) fn read(rel: &Rel, layout: &Layout) -> Result<ActorTables> {
    let [start, end] = layout.item_throw_origins;
    let bytes = end
        .checked_sub(start)
        .context("reversed item throw origins")?;
    parse(
        rel.at((5, start))?
            .get(..bytes)
            .context("truncated item throw origins")?,
        rel.at((4, layout.tint_palette))?
            .get(..48)
            .context("truncated actor tint palette")?,
    )
}

pub(crate) fn cook_all(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    embedded::write(file, output, "battle-actor-tables", &read(&Rel::read(file)?, &layout)?,
        serde_json::json!({
            "item_throw_origins": {"section":5,"offset":layout.item_throw_origins[0],"end":layout.item_throw_origins[1],"stride":12},
            "tint_palette": {"section":4,"offset":layout.tint_palette,"stride":4,"count":12},
        }),
    ).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    fn complete_tables_preserve_unused_slots_and_reject_invalid_scalars() -> Result<()> {
        let mut origins: Vec<u8> = (0..33).flat_map(|i| (i as f32).to_be_bytes()).collect();
        let colors: Vec<u8> = (0..48).collect();
        let tables = parse(&origins, &colors)?;
        assert_eq!(tables.item_throw_origins[10], [30., 31., 32.]);
        assert_eq!(tables.tint_palette[11], [44, 45, 46, 47]);
        assert_eq!(
            serde_json::from_slice::<ActorTables>(&serde_json::to_vec(&tables)?)?,
            tables
        );
        let extended = [origins.clone(), vec![1, 2, 3, 4]].concat();
        assert_eq!(parse(&extended, &colors)?.trailing_storage, [1, 2, 3, 4]);
        assert!(parse(&[], &colors).is_err());
        assert!(parse(&origins, &colors[..47]).is_err());
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            origins[..4].copy_from_slice(&invalid.to_be_bytes());
            assert!(parse(&origins, &colors).is_err());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; no media conversion"]
    fn original_actor_tables_cover_and_deduplicate_every_module() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("actor-tables"));
        let mut publications = BTreeSet::new();
        for disc in [1, 2] {
            let mut modules = 0;
            for file in fs::read_dir(local.join(format!("disc{disc}/files")))? {
                let file = file?.path();
                let Some((module, layout)) = Layout::identify(&file) else {
                    continue;
                };
                let rel = Rel::read(&file)?;
                let tables = read(&rel, &layout)?;
                let [start, end] = layout.item_throw_origins;
                assert_eq!(
                    tables.item_throw_origins.len(),
                    if end - start == 132 { 11 } else { 9 }
                );
                assert_eq!(tables.item_throw_origins[0], [-10., 100., 10.]);
                assert_eq!(tables.item_throw_origins[8], [-20., 95., 25.]);
                if tables.item_throw_origins.len() == 11 {
                    assert_eq!(tables.item_throw_origins[9..], [[0., 80., 20.]; 2]);
                }
                for (index, xyz) in tables.item_throw_origins.iter().enumerate() {
                    let source = rel.at((5, start + index * 12))?;
                    for (axis, value) in xyz.iter().enumerate() {
                        assert_eq!(value.to_be_bytes(), source[axis * 4..axis * 4 + 4]);
                    }
                }
                assert_eq!(
                    tables.tint_palette.as_flattened(),
                    &rel.at((4, layout.tint_palette))?[..48]
                );
                assert_eq!(
                    tables.trailing_storage,
                    rel.at((5, start))?[tables.item_throw_origins.len() * 12..end - start]
                );
                assert_eq!(
                    tables.trailing_storage.len(),
                    if module == "Top2BtlD.rel" { 4 } else { 0 }
                );
                for root in [
                    (5, start),
                    (5, end),
                    (4, layout.tint_palette),
                    (4, layout.tint_palette + 48),
                ] {
                    assert!(rel.local_targets().contains(&root), "{module}: {root:x?}");
                }
                let paths = cook_all(&file, &output)?.unwrap();
                let published: ActorTables =
                    crate::embedded::read(&output, "battle-actor-tables", module)?;
                assert_eq!(published, tables);
                publications.insert(paths[0].clone());
                modules += 1;
            }
            assert_eq!(modules, 7);
        }
        assert_eq!(publications.len(), 3);
        fs::remove_dir_all(output)?;
        Ok(())
    }
}
