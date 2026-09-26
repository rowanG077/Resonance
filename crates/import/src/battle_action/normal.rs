//! REL normal-attack groups selected by 3DF34; descriptor reach is read by 1F548.
//! Publish original parameters and streams, never generated native control flow.
use super::*;
use crate::rel::Rel;
use std::path::Path;

const GROUPS: usize = 9;
const ACTIONS: usize = 7;

fn group(module: &Rel, index: usize) -> Result<NormalGroup> {
    let table = 0x3a48 + index * 12;
    let bundles = module.pointer(5, table)?;
    let rules = module.pointer(5, table + 4)?;
    let selectors = module
        .at(module.pointer(5, table + 8)?)?
        .get(..ACTIONS * 4)
        .context("truncated normal selectors")?
        .chunks_exact(4)
        .map(|row| NormalSelector {
            action: row[0],
            allowed_directions: row[1],
            fallback: row[2],
            storage: row[3],
        })
        .collect();
    let mut roots = Vec::with_capacity(ACTIONS);
    for action in 0..ACTIONS {
        roots.push([
            module.pointer(bundles.0, bundles.1 + action * 16)?,
            module.pointer(bundles.0, bundles.1 + action * 16 + 4)?,
            module.pointer(bundles.0, bundles.1 + action * 16 + 8)?,
            module.pointer(bundles.0, bundles.1 + action * 16 + 12)?,
        ]);
    }
    let descriptors = roots[0][0].1;
    let mut starts = [usize::MAX; 3];
    for root in &roots {
        for (slot, start) in starts.iter_mut().enumerate() {
            *start = (*start).min(root[slot + 1].1);
        }
    }
    let [hits, animations, commands_start] = starts;
    let boundaries = [
        rules.1,
        descriptors,
        animations,
        hits,
        commands_start,
        bundles.1,
    ];
    ensure!(
        rules.0 == bundles.0
            && roots.iter().flatten().all(|r| r.0 == rules.0)
            && boundaries.windows(2).all(|p| p[0] <= p[1]),
        "invalid normal action pools"
    );
    let section = module.at((rules.0, 0))?;
    let pool = |start: usize, end: usize, stride: usize| -> Result<&[u8]> {
        ensure!(
            (end - start).is_multiple_of(stride),
            "misaligned normal action pool"
        );
        section
            .get(start..end)
            .context("truncated normal action pool")
    };
    ensure!(
        animations - descriptors == ACTIONS * 24,
        "invalid normal descriptors"
    );
    let hit_rules = pool(rules.1, descriptors, 28)?
        .chunks_exact(28)
        .map(hit_rule)
        .collect::<Result<_>>()?;
    let hit_rows = pool(hits, commands_start, 32)?
        .chunks_exact(32)
        .map(hit)
        .collect::<Result<_>>()?;
    let animation_rows = pool(animations, hits, 12)?
        .chunks_exact(12)
        .map(animation)
        .collect::<Result<_>>()?;
    let descriptors_data = pool(descriptors, animations, 24)?
        .chunks_exact(24)
        .map(|row| {
            Ok(NormalDescriptor {
                duration: u16::read(row, 0)?,
                recovery_ticks: u16::read(row, 2)?,
                combo_at: <[u16; 2]>::read(row, 4)?,
                buffer_until: row[8],
                recovery_clip: row[9],
                recovery_rate: FloatOperand::read(row, 12)?,
                startup_effect: i32::read(row, 16)?,
                reach: <[u16; 2]>::read(row, 20)?,
                storage: unreferenced_storage(row, vec![0..10, 12..24]),
            })
        })
        .collect::<Result<_>>()?;
    let mut actions = Vec::with_capacity(ACTIONS);
    for root in &roots {
        let mut indices = [0; 4];
        for (i, (base, end, stride)) in [
            (descriptors, animations, 24),
            (hits, commands_start, 32),
            (animations, hits, 12),
            (commands_start, bundles.1, 2),
        ]
        .into_iter()
        .enumerate()
        {
            let at = root[i].1;
            ensure!(
                (base..end).contains(&at) && (at - base).is_multiple_of(stride),
                "normal stream outside pool"
            );
            indices[i] = ((at - base) / stride) as u32;
        }
        let [descriptor, hit, animation, command] = indices;
        actions.push(NormalAction {
            descriptor,
            hit,
            animation,
            command,
        });
    }
    let command_pool = pool(commands_start, bundles.1, 2)?;
    let commands = commands(command_pool, actions.iter().map(|a| a.command))?;
    let command_storage = unreferenced_storage(command_pool, command_ranges(&commands).collect());
    Ok(NormalGroup {
        selectors,
        actions,
        descriptors: descriptors_data,
        hit_rules,
        hits: hit_rows,
        animations: animation_rows,
        commands,
        command_storage,
    })
}

fn read(module: &Rel) -> Result<NormalTable> {
    Ok(NormalTable {
        source_sha256: crate::digest(&module.bytes),
        weapon_flights: module
            .at((5, 0x1cbc))?
            .get(..3 * 16)
            .context("truncated weapon-flight profiles")?
            .chunks_exact(16)
            .map(|row| {
                Ok(WeaponFlight {
                    outbound_ticks: i16::read(row, 0)?,
                    speed: FloatOperand::read(row, 4)?,
                    return_speed: FloatOperand::read(row, 8)?,
                    direction_y: FloatOperand::read(row, 12)?,
                    storage: unreferenced_storage(row, vec![0..2, 4..16]),
                })
            })
            .collect::<Result<_>>()?,
        groups: (0..GROUPS)
            .map(|index| group(module, index))
            .collect::<Result<_>>()?,
    })
}

pub fn publish(file: &Path, output: &Path, prefix: &str) -> Result<String> {
    let table = read(&Rel::read(file)?)?;
    let path = format!("{prefix}/normal-actions.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&table)?)?;
    Ok(path)
}

#[cfg(test)]
mod tests;
