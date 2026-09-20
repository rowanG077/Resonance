//! Authored movement constants, reaction impulses and projectile motion records.
use super::embedded::Layout;
use crate::{
    embedded,
    read::{f32 as float, u16 as half},
    rel::Rel,
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle::{MotionRules, actions::ProjectileMotion};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "battle-motion";
const REACTION_COUNT: usize = 19;
const REACTION_BYTES: usize = REACTION_COUNT * 8;
const PROJECTILE_BYTES: usize = 16;

#[derive(Clone, Copy, Serialize)]
pub(super) struct MotionLayout {
    pub reactions: usize,
    pub projectile_motions: [usize; 2],
    pub acceleration: usize,
    pub action_drag: usize,
    pub drag_thresholds: [usize; 2],
    pub stop_epsilon: usize,
    pub projectile_velocity_reset_scale: usize,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MotionTables {
    acceleration: f32,
    action_drag: f32,
    /// Positive and negative thresholds are separate authored constants.
    drag_thresholds: [f32; 2],
    stop_epsilon: f64,
    projectile_velocity_reset_scale: f32,
    reactions: [[f32; 2]; REACTION_COUNT],
    projectile_motions: Vec<ProjectileMotionRecord>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectileMotionRecord {
    lifetime: i16,
    /// The native table consumer does not read these two bytes.
    unreferenced_02: [u8; 2],
    forward_speed: f32,
    return_speed: f32,
    vertical_speed: f32,
}

impl MotionTables {
    pub(super) fn projectile_velocity_reset_scale(&self) -> Result<f32> {
        ensure!(
            self.projectile_velocity_reset_scale.is_finite(),
            "non-finite projectile velocity reset scale"
        );
        Ok(self.projectile_velocity_reset_scale)
    }

    pub(super) fn bind_projectiles(&self) -> Result<Vec<ProjectileMotion>> {
        self.projectile_motions
            .iter()
            .map(|record| {
                ensure!(
                    [
                        record.forward_speed,
                        record.return_speed,
                        record.vertical_speed
                    ]
                    .into_iter()
                    .all(f32::is_finite),
                    "non-finite projectile motion"
                );
                Ok(ProjectileMotion {
                    lifetime: record.lifetime,
                    forward_speed: record.forward_speed,
                    return_speed: record.return_speed,
                    vertical_speed: record.vertical_speed,
                })
            })
            .collect()
    }

    pub(super) fn bind(&self) -> Result<MotionRules> {
        ensure!(
            [self.acceleration, self.action_drag, self.drag_thresholds[0]]
                .into_iter()
                .all(|value| value.is_finite() && value > 0.)
                && self.drag_thresholds[1] == -self.drag_thresholds[0]
                && self.stop_epsilon.is_finite()
                && self.stop_epsilon > 0.
                && self
                    .reactions
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite()),
            "invalid or asymmetric battle motion rules"
        );
        Ok(MotionRules {
            acceleration: self.acceleration,
            action_drag: self.action_drag,
            drag_deadzone: self.drag_thresholds[0],
            stop_epsilon: self.stop_epsilon,
            reactions: self.reactions,
        })
    }
}

pub(super) fn read(rel: &Rel, layout: &Layout) -> Result<MotionTables> {
    let layout = layout.motion;
    let impulses = rel
        .at((5, layout.reactions))?
        .get(..REACTION_BYTES)
        .context("truncated battle reaction table")?;
    let mut reactions = [[0.; 2]; REACTION_COUNT];
    for (index, pair) in reactions.iter_mut().enumerate() {
        *pair = [float(impulses, index * 8)?, float(impulses, index * 8 + 4)?];
    }
    let scalar = |offset| float(rel.at((4, offset))?, 0);
    let stop_epsilon = f64::from_be_bytes(
        rel.at((4, layout.stop_epsilon))?
            .get(..8)
            .context("truncated battle stop epsilon")?
            .try_into()?,
    );
    ensure!(stop_epsilon.is_finite(), "non-finite battle stop epsilon");
    let [start, end] = layout.projectile_motions;
    let size = end
        .checked_sub(start)
        .context("reversed projectile motion extent")?;
    ensure!(
        size.is_multiple_of(PROJECTILE_BYTES),
        "partial projectile motion record"
    );
    let projectile_motions = rel
        .at((5, start))?
        .get(..size)
        .context("truncated projectile motion table")?
        .chunks_exact(PROJECTILE_BYTES)
        .map(|row| {
            Ok(ProjectileMotionRecord {
                lifetime: half(row, 0)? as i16,
                unreferenced_02: [row[2], row[3]],
                forward_speed: float(row, 4)?,
                return_speed: float(row, 8)?,
                vertical_speed: float(row, 12)?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(MotionTables {
        acceleration: scalar(layout.acceleration)?,
        action_drag: scalar(layout.action_drag)?,
        drag_thresholds: [
            scalar(layout.drag_thresholds[0])?,
            scalar(layout.drag_thresholds[1])?,
        ],
        stop_epsilon,
        projectile_velocity_reset_scale: scalar(layout.projectile_velocity_reset_scale)?,
        reactions,
        projectile_motions,
    })
}

pub(crate) fn cook_all(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    embedded::write(
        file,
        output,
        FAMILY,
        &read(&Rel::read(file)?, &layout)?,
        serde_json::json!({
            "reaction_section": 5, "scalar_section": 4, "layout": layout.motion,
            "reaction_count": REACTION_COUNT, "reaction_stride": 8,
            "reaction_bytes": REACTION_BYTES, "stop_epsilon_bytes": 8,
            "projectile_section": 5, "projectile_stride": PROJECTILE_BYTES,
            "projectile_bytes": layout.motion.projectile_motions[1] - layout.motion.projectile_motions[0],
        }),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    fn projectile_bytes(records: &[ProjectileMotionRecord]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for record in records {
            bytes.extend(record.lifetime.to_be_bytes());
            bytes.extend(record.unreferenced_02);
            for scalar in [
                record.forward_speed,
                record.return_speed,
                record.vertical_speed,
            ] {
                bytes.extend(scalar.to_be_bytes());
            }
        }
        bytes
    }

    #[test]
    fn motion_reads_are_bounded_and_binding_keeps_double_precision() -> Result<()> {
        let mut layout = Layout::RETAIL;
        layout.motion = MotionLayout {
            reactions: 0,
            projectile_motions: [REACTION_BYTES, REACTION_BYTES + 2 * PROJECTILE_BYTES],
            acceleration: 0,
            action_drag: 4,
            drag_thresholds: [8, 12],
            stop_epsilon: 16,
            projectile_velocity_reset_scale: 24,
        };
        let epsilon = 0.550000011920929_f64.next_up();
        let mut bytes = vec![0];
        for value in [0.5_f32, 0.4125, 0.55, -0.55] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&epsilon.to_be_bytes());
        bytes.extend_from_slice(&0.001_f32.to_be_bytes());
        for value in 0..REACTION_COUNT * 2 {
            bytes.extend_from_slice(&(value as f32).to_be_bytes());
        }
        for (lifetime, unused, speeds) in [
            (-1_i16, [0xa5, 0x5a], [-4_f32, 0., 2.]),
            (7, [0, 1], [5., -6., 7.]),
        ] {
            bytes.extend(lifetime.to_be_bytes());
            bytes.extend(unused);
            for speed in speeds {
                bytes.extend(speed.to_be_bytes());
            }
        }
        let mut rel = Rel {
            bytes,
            sections: vec![(0, 0); 6],
            pointers: Default::default(),
            local_targets: Default::default(),
        };
        rel.sections[4] = (1, 28);
        rel.sections[5] = (29, REACTION_BYTES + 2 * PROJECTILE_BYTES);
        let mut tables = read(&rel, &layout)?;
        let binding = tables.bind()?;
        assert_eq!(binding.stop_epsilon.to_bits(), epsilon.to_bits());
        assert_ne!(binding.stop_epsilon, f64::from(binding.stop_epsilon as f32));
        assert_eq!(binding.reactions[18], [36., 37.]);
        assert_eq!(tables.projectile_velocity_reset_scale()?, 0.001);
        let projectiles = tables.bind_projectiles()?;
        assert_eq!(projectiles.len(), 2);
        assert_eq!(projectiles[0].lifetime, -1);
        assert_eq!(projectiles[0].forward_speed, -4.);
        assert_eq!(projectiles[1].return_speed, -6.);
        assert_eq!(
            projectile_bytes(&tables.projectile_motions),
            rel.bytes[29 + REACTION_BYTES..]
        );
        assert_eq!(
            serde_json::from_slice::<MotionTables>(&serde_json::to_vec(&tables)?)?,
            tables
        );
        for section in [4, 5] {
            rel.sections[section].1 -= 1;
            assert!(read(&rel, &layout).is_err());
            rel.sections[section].1 += 1;
        }
        for offset in [1, 5, 9, 13, 25, 29, 29 + REACTION_BYTES + 4] {
            let saved = rel.bytes[offset..offset + 4].to_vec();
            rel.bytes[offset..offset + 4].copy_from_slice(&f32::NAN.to_be_bytes());
            assert!(read(&rel, &layout).is_err());
            rel.bytes[offset..offset + 4].copy_from_slice(&saved);
        }
        rel.bytes[17..25].copy_from_slice(&f64::INFINITY.to_be_bytes());
        assert!(read(&rel, &layout).is_err());
        rel.bytes[17..25].copy_from_slice(&epsilon.to_be_bytes());
        layout.motion.projectile_motions[1] -= 1;
        assert!(read(&rel, &layout).is_err());
        layout.motion.projectile_motions.reverse();
        assert!(read(&rel, &layout).is_err());
        tables.projectile_motions[0].vertical_speed = f32::INFINITY;
        assert!(tables.bind_projectiles().is_err());
        assert!(tables.bind().is_ok());
        tables.projectile_velocity_reset_scale = f32::INFINITY;
        assert!(tables.projectile_velocity_reset_scale().is_err());
        tables.drag_thresholds[1] = -0.6;
        assert!(tables.bind().is_err());
        tables.drag_thresholds[1] = -tables.drag_thresholds[0];
        tables.stop_epsilon = -1.;
        assert!(tables.bind().is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; no media conversion"]
    fn original_motion_tables_cover_every_module_and_match_binding() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let mut publications = BTreeSet::new();
        let mut expected_binding = None;
        let result = (|| -> Result<()> {
            for disc in [1, 2] {
                let mut modules = 0;
                for file in fs::read_dir(local.join(format!("disc{disc}/files")))? {
                    let file = file?.path();
                    let Some((module, layout)) = Layout::identify(&file) else {
                        continue;
                    };
                    let rel = Rel::read(&file)?;
                    let tables = read(&rel, &layout)?;
                    let layout = layout.motion;
                    let impulses = &rel.at((5, layout.reactions))?[..REACTION_BYTES];
                    assert_eq!(
                        crate::digest(impulses),
                        "a5a1196c514fa570d17316d42f2a67cfd4621a7ff22c18273d7ffd63cc4532e4"
                    );
                    for (value, source) in tables
                        .reactions
                        .iter()
                        .flatten()
                        .zip(impulses.chunks_exact(4))
                    {
                        assert_eq!(value.to_bits(), u32::from_be_bytes(source.try_into()?));
                    }
                    for (offset, value) in [
                        (layout.acceleration, tables.acceleration),
                        (layout.action_drag, tables.action_drag),
                        (layout.drag_thresholds[0], tables.drag_thresholds[0]),
                        (layout.drag_thresholds[1], tables.drag_thresholds[1]),
                        (
                            layout.projectile_velocity_reset_scale,
                            tables.projectile_velocity_reset_scale,
                        ),
                    ] {
                        assert!(rel.local_targets().contains(&(4, offset)));
                        assert_eq!(value.to_bits(), crate::read::u32(rel.at((4, offset))?, 0)?);
                    }
                    assert!(rel.local_targets().contains(&(4, layout.stop_epsilon)));
                    assert_eq!(
                        tables.stop_epsilon.to_be_bytes(),
                        rel.at((4, layout.stop_epsilon))?[..8]
                    );
                    for root in [layout.reactions, layout.reactions + REACTION_BYTES] {
                        assert!(rel.local_targets().contains(&(5, root)));
                    }
                    let [start, end] = layout.projectile_motions;
                    assert_eq!(end - start, 3 * PROJECTILE_BYTES);
                    assert!(rel.local_targets().contains(&(5, start)));
                    assert_eq!(
                        rel.local_targets().range((5, start + 1)..).next(),
                        Some(&(5, end)),
                    );
                    assert_eq!(
                        projectile_bytes(&tables.projectile_motions),
                        rel.at((5, start))?[..end - start]
                    );
                    assert!(
                        tables
                            .projectile_motions
                            .iter()
                            .all(|row| row.unreferenced_02 == [0; 2])
                    );
                    let projectiles = tables.bind_projectiles()?;
                    assert_eq!(
                        projectiles
                            .iter()
                            .map(|row| row.lifetime)
                            .collect::<Vec<_>>(),
                        [14, 10, 8]
                    );
                    assert_eq!(
                        projectiles
                            .iter()
                            .map(|row| [row.forward_speed, row.return_speed, row.vertical_speed])
                            .collect::<Vec<_>>(),
                        [[25., 25., 0.], [25., 25., -0.7], [25., 25., 0.4]]
                    );
                    let binding = tables.bind()?;
                    assert_eq!(tables.projectile_velocity_reset_scale()?, 0.001);
                    assert_eq!(binding.acceleration, 0.5);
                    assert_eq!(binding.action_drag.to_bits(), 0x3ed3_3334);
                    assert_eq!(binding.drag_deadzone, 0.55);
                    assert_eq!(binding.stop_epsilon, 0.550000011920929);
                    let binding = serde_json::to_vec(&binding)?;
                    assert_eq!(
                        expected_binding.get_or_insert_with(|| binding.clone()),
                        &binding
                    );
                    let paths = cook_all(&file, &output)?.unwrap();
                    assert_eq!(
                        fs::read(output.join(&paths[0]))?,
                        serde_json::to_vec(&tables)?
                    );
                    let source: serde_json::Value =
                        serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                    assert_eq!(source["module"], module);
                    assert_eq!(source["source_sha256"], crate::digest(&rel.bytes));
                    assert_eq!(source["data"], paths[0]);
                    assert_eq!(source["layout"], serde_json::to_value(layout)?);
                    assert_eq!(source["reaction_section"], 5);
                    assert_eq!(source["scalar_section"], 4);
                    assert_eq!(source["reaction_bytes"], REACTION_BYTES);
                    assert_eq!(source["projectile_section"], 5);
                    assert_eq!(source["projectile_stride"], PROJECTILE_BYTES);
                    assert_eq!(source["projectile_bytes"], end - start);
                    let cooked: MotionTables = embedded::read(&output, FAMILY, module)?;
                    assert_eq!(cooked, tables);
                    assert_eq!(serde_json::to_vec(&cooked.bind()?)?, binding);
                    assert_eq!(
                        serde_json::to_vec(&cooked.bind_projectiles()?)?,
                        serde_json::to_vec(&projectiles)?
                    );
                    publications.insert(paths[0].clone());
                    modules += 1;
                }
                assert_eq!(modules, 7);
            }
            assert_eq!(publications.len(), 1);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
