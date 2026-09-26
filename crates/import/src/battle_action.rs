//! Common action pools copied by 34C; command widths follow original 2C8B0.
//! These are source records. Native control flow remains maintained `.sym` source.
use crate::read::{Field, FloatOperand, unreferenced_storage};
use anyhow::{Context, Result, bail, ensure};
use resonance_content::battle_action::*;
use std::collections::BTreeMap;

fn phase(row: &[u8], indices: [u32; 4]) -> Result<Phase> {
    Ok(Phase {
        duration: u16::read(row, 0)?,
        recovery_ticks: u16::read(row, 2)?,
        buffer_until: u16::read(row, 4)?,
        combo_at: u16::read(row, 6)?,
        startup_effect: i32::read(row, 8)?,
        indices,
    })
}

fn hit_rule(row: &[u8]) -> Result<HitRule> {
    Ok(HitRule {
        flags: u16::read(row, 0)?,
        element: row[2],
        hitstun: row[3],
        contact_cooldown: row[4],
        stun_chance: row[5],
        stagger: row[6],
        guard_pressure: row[7],
        conditions: u32::read(row, 8)?,
        condition_chance: row[12],
        power_mode: row[13],
        power: u16::read(row, 14)?,
        sound: u16::read(row, 16)?,
        armor_damage: row[20],
        knockback_delay: row[21],
        impact_effect: row[22],
        condition_parameter: row[23] as i8,
        impact_bank: row[24],
        storage: unreferenced_storage(row, vec![0..18, 20..25]),
    })
}

fn hit(row: &[u8]) -> Result<Hit> {
    Ok(Hit {
        start: i16::read(row, 0)?,
        emission: row[2] as i8,
        attachment_count: row[3] as i8,
        emission_operands: <[u8; 4]>::read(row, 4)?,
        radius: FloatOperand::read(row, 8)?,
        height: FloatOperand::read(row, 12)?,
        shape: row[16],
        damage_kind: row[17],
        rule: row[18],
        hit_class: row[19],
        reaction: row[20],
        projectile_modifier: u16::read(row, 22)?,
        inner_radius: FloatOperand::read(row, 28)?,
        storage: unreferenced_storage(row, vec![0..21, 22..24, 28..32]),
    })
}

pub(crate) fn animation(row: &[u8]) -> Result<Animation> {
    Ok(Animation {
        time: i16::read(row, 0)?,
        clip: row[2],
        blend: row[3],
        start: row[4],
        end: row[5],
        layer_flags: row[6],
        resource: row[7] as i8,
        rate: FloatOperand::read(row, 8)?,
    })
}

fn command_size(opcode: i16) -> Result<usize> {
    Ok(match opcode {
        -5 | -3 | 4 | 22 | 30 | 31 => 4,
        0..=3 | 7..=9 | 20 | 21 | 40 | 41 | 48 => 6,
        5 | 6 | 12 | 13 | 15..=19 | 23 | 24 | 26..=28 | 34..=36 | 38 | 39 | 43 | 44 => 8,
        10 | 37 => 10,
        29 | 32 | 42 | 45..=47 => 12,
        14 | 25 => 16,
        33 => 20,
        _ => bail!("unknown action command {opcode}"),
    })
}

fn commands(bytes: &[u8], roots: impl IntoIterator<Item = u32>) -> Result<Vec<Command>> {
    let mut commands = BTreeMap::new();
    for root in roots {
        let mut at = root as usize * 2;
        let rest = bytes
            .get(at..)
            .context("action command root outside pool")?;
        if rest.iter().all(|&v| v == 0) {
            continue;
        }
        loop {
            let word_index = (at / 2) as u32;
            if commands.contains_key(&word_index) {
                break;
            }
            let time = i16::read(bytes, at)?;
            let (size, record) = match time {
                -1 => (2, CommandRecord::End),
                -2 => (2, CommandRecord::Loop),
                _ => {
                    let opcode = i16::read(bytes, at + 2)?;
                    let size = command_size(opcode)?;
                    let operands = bytes
                        .get(at + 4..at + size)
                        .context("truncated action command")?
                        .chunks_exact(2)
                        .map(|word| u16::read(word, 0))
                        .collect::<Result<_>>()?;
                    (
                        size,
                        CommandRecord::Command {
                            time,
                            opcode,
                            operands,
                        },
                    )
                }
            };
            let terminal = matches!(record, CommandRecord::End | CommandRecord::Loop);
            commands.insert(word_index, Command { word_index, record });
            at += size;
            if terminal {
                break;
            }
        }
    }
    Ok(commands.into_values().collect())
}

fn command_ranges(commands: &[Command]) -> impl Iterator<Item = std::ops::Range<usize>> {
    commands.iter().map(|command| {
        let size = match &command.record {
            CommandRecord::End | CommandRecord::Loop => 2,
            CommandRecord::Command { operands, .. } => 4 + operands.len() * 2,
        };
        let start = command.word_index as usize * 2;
        start..start + size
    })
}

pub(crate) mod enemy;
pub mod normal;

pub(crate) fn bundle(bytes: &[u8]) -> Result<Bundle> {
    let compact = u32::read(bytes, 12)? == 28;
    let pool_offsets = <[u32; 4]>::read(bytes, if compact { 12 } else { 0 })?;
    let bases = pool_offsets.map(|v| v as usize);
    let header_size = if compact { 28 } else { 128 };
    ensure!(
        bases[0] == header_size
            && bases.windows(2).all(|p| p[0] <= p[1])
            && bases[3] <= bytes.len(),
        "invalid action pool boundaries"
    );
    let phases = if compact {
        vec![phase(bytes, [0; 4])?]
    } else {
        bytes[16..128]
            .chunks_exact(28)
            .map(|row| phase(row, <[u32; 4]>::read(row, 12)?))
            .collect::<Result<_>>()?
    };
    let rules = &bytes[bases[0]..bases[1]];
    ensure!(
        rules.len().is_multiple_of(28),
        "misaligned action hit rules"
    );
    let hit_rules = rules
        .chunks_exact(28)
        .map(hit_rule)
        .collect::<Result<_>>()?;
    let hits = bytes[bases[1]..bases[2]]
        .chunks_exact(32)
        .map(hit)
        .collect::<Result<Vec<_>>>()?;
    let animations = bytes[bases[2]..bases[3]]
        .chunks_exact(12)
        .map(animation)
        .collect::<Result<Vec<_>>>()?;
    let mut covered = vec![
        0..bases[1],
        bases[1]..bases[1] + hits.len() * 32,
        bases[2]..bases[2] + animations.len() * 12,
    ];
    let ends = [bases[1], bases[2], bases[3], bytes.len()];
    let strides = [28, 32, 12, 2];
    for phase in &phases {
        for i in 0..4 {
            let offset = u64::from(pool_offsets[i]) + u64::from(phase.indices[i]) * strides[i];
            ensure!(offset <= ends[i] as u64, "action phase index outside pool");
        }
    }
    let commands = if bases[1] == bases[3] {
        Vec::new()
    } else {
        commands(&bytes[bases[3]..], phases.iter().map(|p| p.indices[3]))?
    };
    covered.extend(
        command_ranges(&commands).map(|range| bases[3] + range.start..bases[3] + range.end),
    );
    Ok(Bundle {
        pool_offsets,
        phases,
        hit_rules,
        hits,
        animations,
        commands,
        storage: unreferenced_storage(bytes, covered),
    })
}

pub(crate) fn read(bytes: &[u8]) -> Result<Table> {
    let records = crate::field::sections(bytes)?
        .into_iter()
        .enumerate()
        .map(|(index, range)| {
            range
                .map(|range| {
                    bundle(&bytes[range]).with_context(|| format!("action member {index}"))
                })
                .transpose()
        })
        .collect::<Result<_>>()?;
    Ok(Table {
        source_sha256: crate::digest(bytes),
        records,
    })
}

pub fn publish(usual: &[u8], output: &std::path::Path, prefix: &str) -> Result<Vec<String>> {
    [(8, "martial-actions.json"), (9, "spell-actions.json")]
        .into_iter()
        .map(|(member, name)| {
            let table = read(crate::source_assets::section(usual, member)?)?;
            let path = format!("{prefix}/{name}");
            crate::write_atomic(&output.join(&path), &serde_json::to_vec(&table)?)?;
            Ok(path)
        })
        .collect()
}

#[cfg(test)]
mod tests;
