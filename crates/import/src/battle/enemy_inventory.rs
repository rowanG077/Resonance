//! Recover all directory slots; errors remain visible as structured inventory issues.
use super::{action_program::decode_commands, actions};
use crate::{
    compression,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle::{
        effects::{EffectBank, EffectId},
        enemy_inventory::*,
    },
    monster::MONSTER_COUNT,
};
use std::{fs, path::Path};

const ACTION_BYTES: usize = 0x44;

pub(crate) fn recover(extracted: &Path) -> Result<EnemyInventory> {
    let directory = fs::read(extracted.join("files/BTL/BTLusual.dat"))?;
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat"))?;
    let table = word(&directory, 0x2c)? as usize;
    let mut inventory = EnemyInventory {
        packages: Vec::new(),
        issues: Vec::new(),
    };
    for monster in 0..MONSTER_COUNT {
        let start = word(&directory, table + monster * 4)? as usize;
        let end = word(&directory, table + (monster + 1) * 4)? as usize;
        let result = (|| {
            let packed = archive
                .get(start..end)
                .context("enemy directory range exceeds archive")?;
            let bytes = compression::decode(packed)?;
            package(&bytes, monster as u8, &mut inventory.issues)
        })();
        inventory.packages.push(match result {
            Ok(package) => package,
            Err(error) => {
                inventory.issues.push(RecoveryIssue {
                    monster: monster as u8,
                    action: None,
                    section: RecoverySection::Package,
                    source_offset: start as u32,
                    reason: format!("{error:#}"),
                    disposition: RecoveryDisposition::RequiredProgram,
                });
                EnemyPackageInventory {
                    monster: monster as u8,
                    variant_count: 0,
                    actions: Vec::new(),
                }
            }
        });
    }
    inventory.validate()?;
    Ok(inventory)
}

pub(super) fn package(
    bytes: &[u8],
    monster: u8,
    issues: &mut Vec<RecoveryIssue>,
) -> Result<EnemyPackageInventory> {
    ensure!(
        bytes.starts_with(b"em8\0"),
        "unsupported enemy package format"
    );
    let metadata = usize::from(half(bytes, 4)?);
    let variants = *bytes
        .get(metadata + 0x1e7)
        .context("truncated enemy variant metadata")?;
    ensure!(variants < 16, "invalid enemy variant count");
    if variants != 0 {
        let start = word(bytes, 0x1e0)? as usize;
        ensure!(
            start >= 0x1e8
                && bytes
                    .get(start..start + usize::from(variants) * 36)
                    .is_some(),
            "truncated enemy variant table"
        );
    }
    let start = usize::from(half(bytes, 10)?);
    let end = usize::from(half(bytes, 12)?);
    ensure!(
        end > start
            && (end - start).is_multiple_of(ACTION_BYTES)
            && (end - start) / ACTION_BYTES <= usize::from(u8::MAX),
        "invalid enemy action table"
    );
    let rules = section(bytes, 8)?;
    let commands = section(bytes, 14)?;
    let hits = section(bytes, 16)?;
    let rows = bytes
        .get(start..end)
        .context("truncated enemy action table")?;
    let ordinary_only = ordinary_selection_only(rows, section(bytes, 12)?)?;
    let mut actions = Vec::new();
    for (id, row) in rows.chunks_exact(ACTION_BYTES).enumerate() {
        let id = id as u8;
        let animation_index = half(row, 0x18)?;
        let command_index = half(row, 0x1a)?;
        let hit_index = half(row, 0x1c)?;
        let recovery = half(row, 0x36)?;
        let command_offset = usize::from(command_index) * 2;
        let parsed_commands = recover_program(
            monster,
            id,
            RecoverySection::Commands,
            usize::from(half(bytes, 14)?) + command_offset,
            commands
                .get(command_offset..)
                .context("invalid enemy command index")
                .and_then(decode_commands),
            issues,
        );
        let recovery_commands = if recovery == 0 {
            None
        } else {
            let offset = usize::from(recovery) * 2;
            recover_program(
                monster,
                id,
                RecoverySection::RecoveryCommands,
                usize::from(half(bytes, 14)?) + offset,
                commands
                    .get(offset..)
                    .context("invalid enemy recovery command index")
                    .and_then(decode_commands),
                issues,
            )
        };
        let parsed_animations = recover_program(
            monster,
            id,
            RecoverySection::Animations,
            usize::from(half(bytes, 18)?) + usize::from(animation_index) * 12,
            actions::animations_at(
                bytes,
                usize::from(half(bytes, 18)?) + usize::from(animation_index) * 12,
            ),
            issues,
        );
        let parsed_hits = recover_program(
            monster,
            id,
            RecoverySection::Hits,
            usize::from(half(bytes, 16)?) + usize::from(hit_index) * 32,
            hits.get(usize::from(hit_index) * 32..)
                .context("invalid enemy hit index")
                .and_then(|stream| actions::hits(stream, rules)),
            issues,
        );
        let native = half(row, 0x40)?;
        let flags = word(row, 8)?;
        let entry_route = entry_route(row)?;
        if ordinary_only && entry_route == EnemyEntryRoute::MoveAwayFromTarget {
            for issue in issues
                .iter_mut()
                .filter(|issue| issue.monster == monster && issue.action == Some(id))
            {
                issue.disposition = RecoveryDisposition::UnusedByEntryRoute(entry_route);
            }
        }
        actions.push(EnemyActionInventory {
            id,
            entry_route,
            duration: half(row, 14)?,
            tp: row[0x34],
            native_technique: (native != 0).then_some(native),
            cast_voices: [half(row, 0x3c)?, half(row, 0x3e)?],
            effect: (row[0x22] != 0).then_some(EffectId {
                bank: EffectBank::Enemy(monster),
                id: row[0x22],
            }),
            recovery_animation: (row[3] != 0).then_some(row[3]),
            hit_recovery_animation: (row[0x35] != 0).then_some(row[0x35]),
            source: EnemyActionSource {
                offset: (start + usize::from(id) * ACTION_BYTES) as u32,
                selection_flags: flags,
                animation_index,
                command_index,
                hit_index,
                recovery_command_index: (recovery != 0).then_some(recovery),
            },
            commands: parsed_commands,
            recovery_commands,
            animations: parsed_animations,
            hits: parsed_hits,
        });
    }
    Ok(EnemyPackageInventory {
        monster,
        variant_count: variants + 1,
        actions,
    })
}

fn recover_program<T>(
    monster: u8,
    action: u8,
    section: RecoverySection,
    offset: usize,
    result: Result<T>,
    issues: &mut Vec<RecoveryIssue>,
) -> Option<T> {
    match result {
        Ok(program) => Some(program),
        Err(error) => {
            issues.push(RecoveryIssue {
                monster,
                action: Some(action),
                section,
                source_offset: offset as u32,
                reason: format!("{error:#}"),
                disposition: RecoveryDisposition::RequiredProgram,
            });
            None
        }
    }
}

fn section(bytes: &[u8], header: usize) -> Result<&[u8]> {
    let start = usize::from(half(bytes, header)?);
    offset_section(bytes, start)
}

pub(super) fn entry_route(row: &[u8]) -> Result<EnemyEntryRoute> {
    Ok(if word(row, 8)? & 0x0200_0000 != 0 {
        EnemyEntryRoute::MoveAwayFromTarget
    } else if half(row, 0x40)? != 0 {
        EnemyEntryRoute::Technique
    } else {
        EnemyEntryRoute::Attack
    })
}

/// Keep all declarations for export, but do not follow offsets that execution
/// cannot use. Zero weight alone is insufficient: counters, priorities, chains
/// and native policies can select actions without their ordinary weights.
pub(super) fn program_actions(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    let actions = section(bytes, 10)?;
    ensure!(
        actions.len().is_multiple_of(ACTION_BYTES),
        "misaligned enemy actions"
    );
    let ordinary_only = ordinary_selection_only(actions, section(bytes, 12)?)?;
    actions
        .chunks_exact(ACTION_BYTES)
        .enumerate()
        .filter_map(|(index, row)| {
            let result = (|| {
                let dormant =
                    ordinary_only && index != 0 && row[0] == 0 && word(row, 8)? & 0x0008_0000 == 0;
                let moves_away =
                    ordinary_only && entry_route(row)? == EnemyEntryRoute::MoveAwayFromTarget;
                Ok((!dormant && !moves_away).then_some(row))
            })();
            result.transpose()
        })
        .collect()
}

fn ordinary_selection_only(actions: &[u8], policy: &[u8]) -> Result<bool> {
    let policy = policy.get(..26).context("truncated enemy policy")?;
    let [native_policy, counter_chance, counter_count, back_row_count] =
        [policy[8], policy[11], policy[22], policy[24]];
    Ok(native_policy == 0
        && counter_chance == 0
        && counter_count == 0
        && back_row_count == 0
        && actions
            .chunks_exact(ACTION_BYTES)
            .all(|row| row[0] as i8 >= 0 && row[32] == 0 && row[50] == 0 && row[64..66] == [0, 0]))
}

/// Include every complete program, even when no action selects it. A terminal
/// record consumes only its first halfword; callers validate entry points too.
pub(super) fn program_extent(bytes: &[u8], stride: usize, sentinel: i16) -> Result<usize> {
    if bytes.is_empty() {
        return Ok(0);
    }
    bytes
        .chunks(stride)
        .rposition(|row| half(row, 0).ok() == Some(sentinel as u16))
        .map(|index| index * stride + 2)
        .context("unterminated fixed action table")
}

pub(super) fn offset_section(bytes: &[u8], start: usize) -> Result<&[u8]> {
    ensure!(start > 0, "missing enemy section");
    let mut end = bytes.len();
    for offset in (4..20)
        .step_by(2)
        .map(|at| half(bytes, at).map(usize::from))
        .chain(
            (24..488)
                .step_by(4)
                .map(|at| word(bytes, at).map(|value| value as usize)),
        )
    {
        let offset = offset?;
        if offset > start {
            end = end.min(offset);
        }
    }
    bytes
        .get(start..end)
        .context("enemy section outside package")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dormant_actions_require_closed_selection_routes() -> Result<()> {
        let start = 0x200;
        let policy = start + ACTION_BYTES * 3;
        let mut bytes = vec![0; policy + 26];
        for (header, offset) in [(10, start), (12, policy), (14, policy + 26)] {
            bytes[header..header + 2].copy_from_slice(&(offset as u16).to_be_bytes());
        }
        bytes[policy + 23] = 3;
        assert_eq!(program_actions(&bytes)?.len(), 1); // Row 0 is always the fallback.
        let second = start + ACTION_BYTES;
        for (offset, value) in [
            (policy + 8, 1),
            (policy + 11, 1),
            (policy + 22, 1),
            (policy + 24, 1),
            (second + 32, 1),
            (second + 50, 1),
            (second + 65, 1),
            (second, 128),
        ] {
            bytes[offset] = value;
            assert_eq!(
                program_actions(&bytes)?.len(),
                3,
                "selection route at {offset:#x}"
            );
            bytes[offset] = 0;
        }
        bytes[second + 8..second + 12].copy_from_slice(&0x0008_0000u32.to_be_bytes());
        assert_eq!(program_actions(&bytes)?.len(), 2); // Priority ignores ordinary weight.
        bytes[second + 8..second + 12].copy_from_slice(&0x0208_0000u32.to_be_bytes());
        assert_eq!(program_actions(&bytes)?.len(), 1); // Moving away never starts these programs.
        bytes[policy + 11] = 1;
        assert_eq!(program_actions(&bytes)?.len(), 3); // Counters bypass the move-away route.
        Ok(())
    }
}
