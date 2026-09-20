//! Complete formation geometry and actor strategy defaults, including unused columns.
use super::embedded::{self, Layout, PARTY_COUNT};
use crate::{read::f32 as float, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::battle::{GeneratedEnemyLayout, PartyLayout};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Serialize)]
pub(super) struct PlacementLayout {
    /// Section 4 XYZ arrays followed immediately by three initial row counts.
    pub party_points: usize,
    pub enemy_points: usize,
    pub strategies: usize,
    pub strategy_columns: usize,
    /// origin_x, row_step, member_x, member_z, center_z, single_even, single_odd, height.
    pub party_scalars: [usize; 8],
    pub enemy_scalars: [usize; 8],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Placement {
    positions: Vec<[f32; 3]>,
    initial_counts: [u8; 3],
    origin_x: f32,
    row_step: f32,
    member_offset: [f32; 2],
    center_z: f32,
    single_z: [f32; 2],
    height: f32,
}

impl Placement {
    fn validate(&self, count: usize) -> Result<()> {
        ensure!(
            self.positions.len() == count,
            "invalid formation point count"
        );
        ensure!(
            self.positions
                .iter()
                .flatten()
                .chain(&self.member_offset)
                .chain(&self.single_z)
                .chain([&self.origin_x, &self.row_step, &self.center_z, &self.height])
                .all(|v| v.is_finite()),
            "nonfinite formation geometry"
        );
        Ok(())
    }

    fn rows(&self) -> Result<[f32; 3]> {
        ensure!(
            self.initial_counts == [0; 3] && self.height == 0.,
            "runtime requires empty formation rows at ground height"
        );
        let rows = [0., 1., 2.].map(|row| self.origin_x + row * self.row_step);
        ensure!(rows.iter().all(|v| v.is_finite()), "formation row overflow");
        Ok(rows)
    }

    fn single_rows(&self) -> [f32; 3] {
        [self.single_z[0], self.single_z[1], self.single_z[0]]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyDefaults {
    /// Actor kind minus one indexes each row; retain unused trailing columns too.
    action: Vec<u8>,
    skill_magic: Vec<u8>,
    position: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlacementTables {
    party: Placement,
    enemy: Placement,
    strategy_defaults: StrategyDefaults,
}

impl PlacementTables {
    fn validate(&self) -> Result<()> {
        self.party.validate(4)?;
        self.enemy.validate(8)?;
        let defaults = &self.strategy_defaults;
        ensure!(
            defaults.position.len() > usize::from(PARTY_COUNT)
                && defaults.action.len() == defaults.position.len()
                && defaults.skill_magic.len() == defaults.position.len(),
            "incomplete actor strategy defaults"
        );
        Ok(())
    }

    pub(super) fn bind(&self) -> Result<(PartyLayout, GeneratedEnemyLayout)> {
        self.validate()?;
        let defaults = &self.strategy_defaults.position[..usize::from(PARTY_COUNT)];
        ensure!(
            defaults.iter().all(|&v| v > 0),
            "unset default party position"
        );
        Ok((
            PartyLayout {
                default_rows: std::array::from_fn(|i| (defaults[i] - 1).min(2)),
                row_x: self.party.rows()?,
                member_offset: self.party.member_offset,
                center_z: self.party.center_z,
                single_z: self.party.single_rows(),
                leader_z: std::array::from_fn(|i| self.party.positions[i][2]),
            },
            GeneratedEnemyLayout {
                row_x: self.enemy.rows()?,
                member_offset: self.enemy.member_offset,
                back_row_x_correction: self.enemy.origin_x,
                center_z: self.enemy.center_z,
                single_z: self.enemy.single_rows(),
                main_z: std::array::from_fn(|i| self.enemy.positions[i][2]),
            },
        ))
    }
}

pub(super) fn read(rel: &Rel, layout: &Layout) -> Result<PlacementTables> {
    let layout = layout.placement;
    let placement = |offset, count: usize, scalars: [usize; 8]| -> Result<Placement> {
        let bytes = rel
            .at((4, offset))?
            .get(..count * 12 + 3)
            .context("truncated formation points or row counts")?;
        let mut constants = [0.; 8];
        for (value, at) in constants.iter_mut().zip(scalars) {
            *value = float(rel.at((4, at))?, 0)?;
        }
        let [
            origin_x,
            row_step,
            member_x,
            member_z,
            center_z,
            single_even,
            single_odd,
            height,
        ] = constants;
        Ok(Placement {
            positions: bytes[..count * 12]
                .chunks_exact(12)
                .map(|row| Ok([float(row, 0)?, float(row, 4)?, float(row, 8)?]))
                .collect::<Result<_>>()?,
            initial_counts: bytes[count * 12..].try_into()?,
            origin_x,
            row_step,
            member_offset: [member_x, member_z],
            center_z,
            single_z: [single_even, single_odd],
            height,
        })
    };
    let columns = layout.strategy_columns;
    let length = columns.checked_mul(3).context("strategy extent overflow")?;
    let defaults = rel
        .at((4, layout.strategies))?
        .get(..length)
        .context("truncated actor strategy defaults")?;
    let tables = PlacementTables {
        party: placement(layout.party_points, 4, layout.party_scalars)?,
        enemy: placement(layout.enemy_points, 8, layout.enemy_scalars)?,
        strategy_defaults: StrategyDefaults {
            action: defaults[..columns].to_vec(),
            skill_magic: defaults[columns..columns * 2].to_vec(),
            position: defaults[columns * 2..].to_vec(),
        },
    };
    tables.validate()?;
    Ok(tables)
}

pub(crate) fn cook_all(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    embedded::write(
        file,
        output,
        "battle-placement",
        &read(&Rel::read(file)?, &layout)?,
        serde_json::json!({
            "section":4, "layout":layout.placement, "point_stride":12,
            "party_points":4, "enemy_points":8, "initial_row_counts":3, "strategy_rows":3,
        }),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    #[ignore = "requires both original extracted discs; no cooking or output files"]
    fn original_placement_and_all_strategy_columns_in_every_module() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut payloads = BTreeMap::new();
        let mut bound = None;
        for disc in [1, 2] {
            for module in [
                "US_r_Top2Btl.rel",
                "r_Top2Btl.rel",
                "US_Top2Btl.rel",
                "US_m_Top2Btl.rel",
                "Top2Btl.rel",
                "m_Top2Btl.rel",
                "Top2BtlD.rel",
            ] {
                let file = extracted.join(format!("disc{disc}/files/{module}"));
                let (_, layout) = Layout::identify(&file).context("missing placement layout")?;
                let rel = Rel::read(&file)?;
                let tables = read(&rel, &layout)?;
                let source = layout.placement;
                let mut roots = vec![
                    source.party_points,
                    source.party_points + 48,
                    source.enemy_points,
                    source.enemy_points + 96,
                    source.strategies,
                ];
                roots.extend(source.party_scalars);
                roots.extend(source.enemy_scalars);
                assert!(
                    roots
                        .iter()
                        .all(|&at| rel.local_targets().contains(&(4, at))),
                    "{module}"
                );
                for (at, geometry) in [
                    (source.party_points, &tables.party),
                    (source.enemy_points, &tables.enemy),
                ] {
                    for (i, xyz) in geometry.positions.iter().enumerate() {
                        for (axis, value) in xyz.iter().enumerate() {
                            assert_eq!(
                                value.to_be_bytes(),
                                rel.at((4, at + i * 12 + axis * 4))?[..4]
                            );
                        }
                    }
                    assert_eq!(geometry.initial_counts, [0; 3]);
                    assert_eq!(geometry.height, 0.);
                    assert_eq!(geometry.single_z, [150., -150.]);
                }
                let defaults = &tables.strategy_defaults;
                for (row, expected) in [
                    (&defaults.action, [4, 3, 4, 4, 4, 8, 4, 4, 8]),
                    (&defaults.skill_magic, [1, 6, 6, 7, 6, 6, 6, 6, 6]),
                    (&defaults.position, [1, 2, 5, 5, 1, 2, 1, 1, 2]),
                ] {
                    assert_eq!(row.len(), source.strategy_columns);
                    assert_eq!(row[..9], expected);
                    assert_eq!(row.last(), Some(&0));
                    assert!(row[9..row.len() - 1].iter().all(|&v| v == 1));
                }
                let (party, enemy) = tables.bind()?;
                assert_eq!(party.default_rows, [0, 1, 2, 2, 0, 1, 0, 0, 1]);
                assert_eq!(party.row_x, [-300., -500., -700.]);
                assert_eq!(party.member_offset, [-50., -400.]);
                assert_eq!(enemy.row_x, [100., 350., 600.]);
                assert_eq!(enemy.member_offset, [50., -250.]);
                let bytes = serde_json::to_vec(&(party, enemy))?;
                assert_eq!(bytes, *bound.get_or_insert_with(|| bytes.clone()));
                let bytes = serde_json::to_vec(&tables)?;
                assert_eq!(serde_json::from_slice::<PlacementTables>(&bytes)?, tables);
                *payloads.entry(bytes).or_insert(0) += 1;
            }
        }
        let mut copies: Vec<_> = payloads.into_values().collect();
        copies.sort_unstable();
        assert_eq!(copies, [6, 8]);
        Ok(())
    }

    #[test]
    fn preserves_unused_data_and_rejects_truncated_or_unbindable_records() -> Result<()> {
        let mut layout = Layout::RETAIL;
        layout.placement = PlacementLayout {
            party_points: 0,
            enemy_points: 52,
            strategies: 152,
            strategy_columns: 12,
            party_scalars: [188, 192, 196, 200, 204, 208, 212, 216],
            enemy_scalars: [188, 192, 196, 200, 204, 208, 212, 216],
        };
        let mut rel = Rel {
            bytes: vec![0; 221],
            sections: vec![(0, 0); 5],
            pointers: Default::default(),
            local_targets: Default::default(),
        };
        rel.sections[4] = (1, 220);
        rel.bytes[153..189].fill(1);
        for offset in [0, 4, 188] {
            rel.bytes[1 + offset..5 + offset].copy_from_slice(&7f32.to_be_bytes());
        }
        let tables = read(&rel, &layout)?;
        assert_eq!(tables.party.positions[0], [7., 7., 0.]);
        assert_eq!(tables.strategy_defaults.position[9..], [1; 3]);
        let (party, enemy) = tables.bind()?;
        assert_eq!(party.leader_z, [0.; 4]);
        assert_eq!(enemy.back_row_x_correction, 7.);
        let mut changed = tables.clone();
        changed.party.initial_counts[0] = 1;
        changed.validate()?;
        assert!(changed.bind().is_err());
        changed = tables.clone();
        changed.enemy.height = 1.;
        assert!(changed.bind().is_err());
        changed = tables.clone();
        changed.strategy_defaults.position[0] = 0;
        assert!(changed.bind().is_err());
        changed = tables.clone();
        changed.party.row_step = f32::MAX;
        assert!(changed.bind().is_err());
        for size in [50, 150, 187, 219] {
            rel.sections[4].1 = size;
            assert!(read(&rel, &layout).is_err());
        }
        rel.sections[4].1 = 220;
        for offset in [0, 188] {
            rel.bytes[1 + offset..5 + offset].copy_from_slice(&f32::NAN.to_be_bytes());
            assert!(read(&rel, &layout).is_err());
            rel.bytes[1 + offset..5 + offset].copy_from_slice(&7f32.to_be_bytes());
        }
        Ok(())
    }
}
