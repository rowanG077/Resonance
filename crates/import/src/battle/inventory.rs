//! Recover every source row before selecting which resources to cook.
use super::{
    action_inventory, audio,
    formations::{FormationTable, RECORD_SIZE, Record},
};
use crate::{
    compression, digest, dol,
    read::{u16 as half, u32 as word},
    write_atomic,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{battle::inventory::*, monster::MONSTER_COUNT};
use std::{collections::BTreeMap, fs, path::Path};

pub(super) fn write(extracted: &Path, output: &Path) -> Result<()> {
    let inventory = recover(extracted)?;
    let audit = inventory.formation_audit();
    let audio_audit = inventory.audio.routing_audit(&inventory.actions);
    write_atomic(output, &serde_json::to_vec_pretty(&inventory)?)?;
    write_atomic(
        &output.with_extension("audit.json"),
        &serde_json::to_vec_pretty(&audit)?,
    )?;
    write_atomic(
        &output.with_extension("audio-audit.json"),
        &serde_json::to_vec_pretty(&audio_audit)?,
    )?;
    println!(
        "Inventoried {} formation rows ({} distinct), {} enemy packages, {} formation reference issues",
        audit.rows,
        audit.distinct_rows,
        inventory.enemies.len(),
        audit.issues.len()
    );
    Ok(())
}

pub(super) fn recover(extracted: &Path) -> Result<SourceInventory> {
    let mut sources = BTreeMap::new();
    let mut read = |path: &str| -> Result<Vec<u8>> {
        let bytes =
            fs::read(extracted.join(path)).with_context(|| format!("read source {path}"))?;
        sources.insert(path.into(), source(&bytes));
        Ok(bytes)
    };
    let directory = read("files/BTL/BTLusual.dat")?;
    let archive = read("files/BTL/BTLenemy.dat")?;
    let executable = read("sys/main.dol")?;
    let bytes = super::actions::member(&directory, 1)?;
    let table = FormationTable::read(bytes)?;
    ensure!(
        table.formations.len() == 1000,
        "unexpected source formation table size"
    );
    let mut seen = BTreeMap::<Vec<u8>, u16>::new();
    let formations = table
        .formations
        .iter()
        .zip(bytes.chunks_exact(RECORD_SIZE))
        .enumerate()
        .map(|(id, (row, bytes))| {
            let duplicate_of = seen.get(bytes).copied();
            seen.entry(bytes.to_vec()).or_insert(id as u16);
            formation(row, id as u16, duplicate_of)
        })
        .collect::<Result<_>>()?;
    let enemy_offsets = word(&directory, 0x2c)? as usize;
    let enemies = (0..MONSTER_COUNT)
        .map(|id| {
            let start = word(&directory, enemy_offsets + id * 4)? as usize;
            let end = word(&directory, enemy_offsets + (id + 1) * 4)? as usize;
            let compressed = archive
                .get(start..end)
                .context("enemy package exceeds archive")?;
            let bytes =
                compression::decode(compressed).with_context(|| format!("decode enemy {id}"))?;
            ensure!(
                bytes.starts_with(b"em8\0"),
                "invalid source enemy package {id}"
            );
            let metadata = usize::from(half(&bytes, 4)?);
            let metadata = bytes
                .get(metadata..metadata + 0x1f0)
                .context("truncated enemy metadata")?;
            let variants = metadata[0x1e7];
            ensure!(variants < 16, "invalid source enemy variant count");
            if variants != 0 {
                let offset = word(&bytes, 0x1e0)? as usize;
                ensure!(
                    offset >= 0x1e8 && offset + usize::from(variants) * 36 <= bytes.len(),
                    "source enemy variant table exceeds package"
                );
            }
            let name = word(dol::slice(&executable, 0x802113f4 + id as u32 * 12, 12)?, 0)?;
            Ok(EnemyPackage {
                monster: id as u8,
                name: dol::text(&executable, name)?,
                compressed: source(compressed),
                decoded_bytes: u32::try_from(bytes.len())?,
                variants: variants + 1,
                palette_rows: metadata[0xef],
                auxiliary_models: metadata[0x1e4],
                auxiliary_resources: metadata[0x1ec],
            })
        })
        .collect::<Result<_>>()?;
    let rel = crate::rel::Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
    let layout = &super::embedded::Layout::RETAIL;
    let inventory = SourceInventory {
        version: SourceInventory::VERSION,
        sources,
        formations,
        enemies,
        actions: action_inventory::recover(extracted)?,
        audio: audio::recover(extracted)?,
        victory_groups: super::victory_group::cook(extracted)?,
        items: super::items::cook(
            &crate::item::read(&executable)?,
            &super::actor_tables::read(&rel, layout)?,
        )?,
        unison: super::unison::cook(
            &super::unison::Inputs::read_source(extracted)?,
            &super::unison_tables::read(&rel, layout)?,
            &super::unison_opener::read(&rel, layout)?,
            &crate::arte::read(&executable)?,
        )?,
    };
    inventory.validate()?;
    Ok(inventory)
}

fn source(bytes: &[u8]) -> SourceFile {
    SourceFile {
        bytes: bytes.len() as u64,
        sha256: digest(bytes),
    }
}

fn formation(row: &Record, id: u16, duplicate_of: Option<u16>) -> Result<FormationRecord> {
    ensure!(
        (1..=8).contains(&row.actor_count) && (1..=4).contains(&row.resource_count),
        "invalid formation source counts"
    );
    ensure!(
        row.storage == [0; 8],
        "unknown formation tail needs recovery"
    );
    Ok(FormationRecord {
        id,
        duplicate_of,
        placement: if row.flags & 2 != 0 {
            Placement::Generated
        } else {
            Placement::Explicit
        },
        escape_allowed: row.flags & 1 == 0,
        victory_music: row.flags & 0x10 == 0,
        victory_camera: row.flags & 0x20 == 0,
        victory_celebration: row.flags & 0xa0 == 0,
        opening_event: row.flags & 0x40 == 0,
        unresolved_flags: row.flags & !(1 | 2 | 0x10 | 0x20 | 0x40 | 0x80),
        hidden_names: row.hidden_names,
        // Inventory diagnostics identify invalid resource halfwords by their original bits.
        resources: row.resources[..usize::from(row.resource_count)]
            .iter()
            .map(|&id| id as u16)
            .collect(),
        enemies: row.actors[..usize::from(row.actor_count)]
            .iter()
            .map(|actor| FormationActor {
                resource: actor.resource,
                variant: actor.variant,
                palette: actor.appearance,
                auxiliary_models: actor.attachments,
                position: actor.position,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_keeps_generated_placements_and_auxiliary_variants() {
        let mut row = Record {
            actor_count: 2,
            resource_count: 1,
            flags: 0xd3,
            hidden_names: 3,
            resources: [49, 0, 0, 0],
            ..Default::default()
        };
        row.actors[0].appearance = 2;
        row.actors[0].variant = 1;
        row.actors[0].attachments[0] = 3;
        row.actors[1].attachments[1] = 4;
        row.actors[0].position = [-200, 300];
        let result = formation(&row, 25, Some(3)).unwrap();
        assert_eq!(result.placement, Placement::Generated);
        assert!(!result.escape_allowed && !result.victory_music && !result.opening_event);
        assert!(result.victory_camera && !result.victory_celebration);
        assert_eq!((result.unresolved_flags, result.hidden_names), (0, 3));
        assert_eq!(result.enemies[0].position, [-200, 300]);
        assert_eq!(result.enemies[0].auxiliary_models, [3, 0]);
        assert_eq!(result.enemies[1].auxiliary_models, [0, 4]);
        let mut enemy = EnemyPackage {
            monster: 49,
            name: "Ghost".into(),
            compressed: source(&[1]),
            decoded_bytes: 1,
            variants: 1,
            palette_rows: 2,
            auxiliary_models: 1,
            auxiliary_resources: 1,
        };
        assert_eq!(
            FormationAudit::new(std::slice::from_ref(&result), std::slice::from_ref(&enemy)).issues,
            [
                FormationIssue::MissingVariant {
                    formation: 25,
                    actor: 0,
                    monster: 49,
                    variant: 1
                },
                FormationIssue::MissingPalette {
                    formation: 25,
                    actor: 0,
                    monster: 49,
                    palette: 2
                },
                FormationIssue::MissingAuxiliaryModel {
                    formation: 25,
                    actor: 0,
                    monster: 49,
                    slot: 0,
                    variant: 3
                }
            ]
        );
        enemy.variants = 2;
        enemy.palette_rows = 3;
        enemy.auxiliary_resources = 4;
        assert!(FormationAudit::new(&[result], &[enemy]).issues.is_empty());
        row.resources[0] = -1;
        row.actor_count = 1;
        let result = formation(&row, 25, None).unwrap();
        assert_eq!(result.resources, [u16::MAX]);
        assert_eq!(
            FormationAudit::new(&[result], &[]).issues,
            [FormationIssue::MissingEnemy {
                formation: 25,
                actor: 0,
                monster: u16::MAX,
            }]
        );
        row.storage[0] = 1;
        assert!(formation(&row, 25, None).is_err());
    }
}
