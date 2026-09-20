//! Recover the modifier programs referenced by enemy hit emissions.
use super::{
    action_program::{HIT_BYTES, HitRecord},
    enemy_inventory::{offset_section, program_extent},
};
use crate::{
    compression,
    read::{f32 as float, u16 as half, u32 as word},
};
use anyhow::{Context, Result, bail, ensure};
use resonance_content::battle::{
    actions::{EnemyActions, HitEmission, HitWindow},
    effects::{EffectBank, EffectId},
    enemy_inventory::{EnemyInventory, EnemyPackageInventory},
    projectile_modifiers::*,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    ops::Range,
    path::Path,
};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct AuthoredModifiers {
    /// Pool location within the decompressed enemy package, for source diagnostics.
    pub source_offset: usize,
    pub source_size: usize,
    pub instructions: Vec<AuthoredInstruction>,
    /// Nonzero storage outside completed instruction streams; offsets are pool-relative.
    pub unreferenced_storage: Vec<crate::read::Storage>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct AuthoredInstruction {
    pub offset: usize,
    pub operation: AuthoredOperation,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum AuthoredOperation {
    End,
    Vector { destination: u16, value: [f32; 3] },
    Byte { destination: u16, value: u8 },
    Halfword { destination: u16, value: u16 },
    Float { destination: u16, value: f32 },
}

/// Decode complete sequential streams, including unused ones, and account for
/// trailing storage. Halfword terminators also make zeros valid empty entry points.
pub(crate) fn authored(package: &[u8], range: Range<usize>) -> Result<AuthoredModifiers> {
    let source_offset = range.start;
    let pool = package
        .get(range)
        .context("modifier pool exceeds package")?;
    // Inspect the complete physical hit table, including rows unused by actions.
    // Only effect emissions consume a nonzero key; terminal rows consume nothing.
    let hits = offset_section(package, usize::from(half(package, 16)?))?;
    let extent = program_extent(hits, HIT_BYTES, -1)?;
    let keys = hits[..extent]
        .chunks_exact(HIT_BYTES)
        .filter_map(|row| {
            HitRecord::read(row)
                .map(|record| record.projectile_modifier())
                .transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    let mut decoded = authored_pool(pool, &keys)?;
    decoded.source_offset = source_offset;
    Ok(decoded)
}

fn authored_pool(pool: &[u8], keys: &[u16]) -> Result<AuthoredModifiers> {
    let mut out = Vec::new();
    let mut cursor = 0;
    let mut terminated = false;
    while cursor < pool.len() {
        let opcode = half(pool, cursor)?;
        if terminated && opcode > 5 {
            break;
        }
        let scalar = |offset| {
            let value = float(pool, cursor + offset)?;
            ensure!(value.is_finite(), "nonfinite projectile modifier value");
            Ok::<_, anyhow::Error>(value)
        };
        let (operation, size) = if opcode == 0 {
            (AuthoredOperation::End, 2)
        } else {
            let destination = half(pool, cursor + 2)?;
            let width = match opcode {
                1 => 12,
                2 | 5 => 1,
                3 => 2,
                4 => 4,
                _ => bail!("unknown projectile modifier {opcode} at {cursor:#x}"),
            };
            ensure!(
                usize::from(destination) + width <= 400,
                "projectile modifier destination exceeds record"
            );
            match opcode {
                1 => (
                    AuthoredOperation::Vector {
                        destination,
                        value: [scalar(4)?, scalar(8)?, scalar(12)?],
                    },
                    16,
                ),
                2 | 5 => (
                    AuthoredOperation::Byte {
                        destination,
                        value: word(pool, cursor + 4)? as u8,
                    },
                    8,
                ),
                3 => (
                    AuthoredOperation::Halfword {
                        destination,
                        value: word(pool, cursor + 4)? as u16,
                    },
                    8,
                ),
                4 => (
                    AuthoredOperation::Float {
                        destination,
                        value: scalar(4)?,
                    },
                    8,
                ),
                _ => unreachable!(),
            }
        };
        pool.get(cursor..cursor + size)
            .context("truncated projectile modifier instruction")?;
        out.push(AuthoredInstruction {
            offset: cursor,
            operation,
        });
        terminated = opcode == 0;
        cursor += size;
    }
    ensure!(
        terminated || pool.is_empty(),
        "unterminated projectile modifier stream"
    );
    for &key in keys.iter().filter(|&&key| key != 0) {
        ensure!(
            out.binary_search_by_key(&usize::from(key), |instruction| instruction.offset)
                .is_ok(),
            "projectile modifier key {key:#x} is not a decoded instruction boundary"
        );
    }
    Ok(AuthoredModifiers {
        source_offset: 0,
        source_size: pool.len(),
        instructions: out,
        unreferenced_storage: crate::read::unreferenced_storage(pool, vec![0..cursor]),
    })
}

pub(crate) fn recover(extracted: &Path, enemies: &EnemyInventory) -> Result<ProjectileModifiers> {
    let sources = super::all::Sources::read(extracted)?;
    let directory = fs::read(extracted.join("files").join(sources.usual))?;
    let archive = fs::read(extracted.join("files").join(sources.enemy))?;
    recover_actions(inventory_hits(&enemies.packages), |monster| {
        let bytes = enemy(&directory, &archive, monster)?;
        let start = word(&bytes, 0x1dc)? as usize;
        let pool = offset_section(&bytes, start)?;
        authored(&bytes, start..start + pool.len())
    })
}

pub(crate) fn bind(
    output: &Path,
    disc: u8,
    source: &str,
    enemies: &[EnemyActions],
) -> Result<ProjectileModifiers> {
    let source = crate::cooked::Source::open(output, disc, source)?;
    let modifiers = bind_programs(
        &source,
        enemies.iter().flat_map(|enemy| {
            enemy
                .actions
                .iter()
                .map(|action| (enemy.monster, action.id, action.action.hits.as_slice()))
        }),
    )?;
    ensure!(
        modifiers
            .programs
            .iter()
            .all(|program| program.issues.is_empty()),
        "projectile modifier program is not completely recovered"
    );
    Ok(modifiers)
}

fn bind_programs<'a>(
    source: &crate::cooked::Source<'_>,
    actions: impl IntoIterator<Item = (u8, u8, &'a [HitWindow])>,
) -> Result<ProjectileModifiers> {
    recover_actions(actions, |monster| {
        let (_, bytes) = source.resolve(&format!(
            "battle/all/enemy-{monster}/projectile-modifiers.json"
        ))?;
        Ok(serde_json::from_slice(&bytes)?)
    })
}

fn inventory_hits(
    packages: &[EnemyPackageInventory],
) -> impl Iterator<Item = (u8, u8, &[HitWindow])> {
    packages.iter().flat_map(|package| {
        package.actions.iter().map(|action| {
            (
                package.monster,
                action.id,
                action.hits.as_deref().unwrap_or_default(),
            )
        })
    })
}

fn enemy(directory: &[u8], archive: &[u8], monster: u8) -> Result<Vec<u8>> {
    ensure!(
        usize::from(monster) < resonance_content::monster::MONSTER_COUNT,
        "invalid modifier monster"
    );
    let table = word(directory, 0x2c)? as usize;
    let start = word(directory, table + usize::from(monster) * 4)? as usize;
    let end = word(directory, table + (usize::from(monster) + 1) * 4)? as usize;
    compression::decode(
        archive
            .get(start..end)
            .context("invalid enemy modifier package")?,
    )
}

fn recover_actions<'a>(
    actions: impl IntoIterator<Item = (u8, u8, &'a [HitWindow])>,
    mut load: impl FnMut(u8) -> Result<AuthoredModifiers>,
) -> Result<ProjectileModifiers> {
    let mut required = BTreeMap::<_, BTreeSet<_>>::new();
    let mut uses = Vec::new();
    for (monster, action, hits) in actions {
        for (hit, window) in hits.iter().enumerate() {
            if let (HitEmission::Effect { effect, .. }, Some(offset)) =
                (&window.emission, window.projectile_modifier)
            {
                let id = ModifierId { monster, offset };
                required.entry(monster).or_default().insert(id);
                uses.push(ModifierUse {
                    monster,
                    action,
                    hit: hit as u16,
                    projectile: EffectId {
                        bank: EffectBank::Enemy(monster),
                        id: *effect,
                    },
                    modifier: id,
                });
            }
        }
    }
    let mut programs = Vec::new();
    for (monster, ids) in required {
        let pool = load(monster)?;
        for id in ids {
            let mut program = ModifierProgram {
                id,
                operations: Vec::new(),
                issues: Vec::new(),
            };
            if let Err(error) = lower(&pool, &mut program) {
                program.issues.push(ModifierIssue {
                    source_offset: (pool.source_offset + usize::from(id.offset)) as u32,
                    problem: ModifierProblem::Source {
                        reason: format!("{error:#}"),
                    },
                });
            }
            programs.push(program);
        }
    }
    let inventory = ProjectileModifiers { programs, uses };
    inventory.validate()?;
    Ok(inventory)
}

fn lower(pool: &AuthoredModifiers, program: &mut ModifierProgram) -> Result<()> {
    let mut cursor = usize::from(program.id.offset);
    let start = pool
        .instructions
        .binary_search_by_key(&cursor, |instruction| instruction.offset)
        .map_err(|_| anyhow::anyhow!("modifier key is not an instruction boundary"))?;
    for instruction in &pool.instructions[start..] {
        ensure!(
            instruction.offset == cursor,
            "discontinuous modifier stream"
        );
        let (store, destination, size) = match instruction.operation {
            AuthoredOperation::End => {
                ensure!(cursor + 2 <= pool.source_size, "modifier exceeds pool");
                return Ok(());
            }
            AuthoredOperation::Vector { destination, .. } => {
                (ProjectileStore::Vector, destination, 16)
            }
            // The two byte-store opcodes have identical semantics.
            AuthoredOperation::Byte { destination, .. } => (ProjectileStore::Byte, destination, 8),
            AuthoredOperation::Halfword { destination, .. } => {
                (ProjectileStore::Halfword, destination, 8)
            }
            AuthoredOperation::Float { destination, .. } => {
                (ProjectileStore::Float, destination, 8)
            }
        };
        ensure!(cursor + size <= pool.source_size, "modifier exceeds pool");
        let operation = match instruction.operation {
            AuthoredOperation::Vector { destination, value } => vector(destination)
                .filter(|(_, axis)| *axis == Axis::X)
                .map(|(field, _)| ProjectileOverride::Vector { field, value }),
            AuthoredOperation::Float { destination, value } => vector(destination)
                .map(|(field, axis)| ProjectileOverride::Component { field, axis, value }),
            AuthoredOperation::Byte {
                destination: 0x13,
                value,
            } => Some(ProjectileOverride::Reaction { value }),
            AuthoredOperation::Byte {
                destination: 0x5d,
                value,
            } => Some(ProjectileOverride::BirthEffect { id: value }),
            _ => None,
        };
        if let Some(operation) = operation {
            program.operations.push(operation);
        } else {
            program.issues.push(ModifierIssue {
                source_offset: (pool.source_offset + cursor) as u32,
                problem: ModifierProblem::UnsupportedDestination { store, destination },
            });
        }
        cursor += size;
    }
    bail!("unterminated projectile modifier stream")
}

fn vector(offset: u16) -> Option<(ProjectileVector, Axis)> {
    let (field, component) = match offset {
        0x24..=0x2c => (ProjectileVector::Velocity, offset - 0x24),
        0x30..=0x38 => (ProjectileVector::Acceleration, offset - 0x30),
        0x60..=0x68 => (ProjectileVector::SpawnOffset, offset - 0x60),
        0x6c..=0x74 => (ProjectileVector::VelocityJitter, offset - 0x6c),
        _ => return None,
    };
    let axis = match component {
        0 => Axis::X,
        4 => Axis::Y,
        8 => Axis::Z,
        _ => return None,
    };
    Some((field, axis))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::enemy_inventory::package;

    #[test]
    #[ignore = "requires both extracted discs and shared cooked records; no media conversion"]
    fn original_projectile_modifier_streams_and_physical_hit_keys() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let files = extracted.join("files");
            let sources = super::super::all::Sources::read(&extracted)?;
            let directory = fs::read(files.join(&sources.usual))?;
            let root = local.join("all-assets");
            let source = crate::cooked::Source::open(&root, disc, &sources.enemy)?;
            let table = crate::battle::actions::member(&directory, 10)?;
            ensure!(table.len().is_multiple_of(4), "invalid enemy offset table");
            let offsets = (0..table.len())
                .step_by(4)
                .map(|at| word(table, at))
                .collect::<Result<Vec<_>>>()?;
            let (mut pools, mut entries, mut uses) = (0, 0, 0);
            for (monster, pair) in offsets.windows(2).enumerate() {
                let (start, end) = (pair[0], pair[1]);
                if end == 0 {
                    ensure!(
                        offsets[monster + 1..].iter().all(|&offset| offset == 0),
                        "enemy offset table has an interior hole"
                    );
                    break;
                }
                ensure!(end >= start, "descending enemy package offsets");
                if end == start {
                    continue;
                }
                let packed = crate::battle::effect_program::all::read_range(
                    &files.join(&sources.enemy),
                    start,
                    end,
                )?;
                let bytes = compression::decode(&packed)?;
                let start = word(&bytes, 0x1dc)? as usize;
                if start == 0 {
                    continue;
                }
                let pool = offset_section(&bytes, start)?;
                let decoded = authored(&bytes, start..start + pool.len())?;
                let (_, published) = source.resolve(&format!(
                    "battle/all/enemy-{monster}/projectile-modifiers.json"
                ))?;
                let published: AuthoredModifiers = serde_json::from_slice(&published)?;
                assert_eq!(
                    serde_json::to_value(&decoded)?,
                    serde_json::to_value(&published)?
                );
                let package = package(&bytes, monster.try_into()?, &mut Vec::new())?;
                let prepared = bind_programs(&source, inventory_hits(&[package]))?;
                ensure!(
                    prepared
                        .programs
                        .iter()
                        .all(|program| program.issues.is_empty()),
                    "shared modifier binding is incomplete for enemy{monster}"
                );
                uses += prepared.uses.len();
                pools += 1;
                // Every physical entry, including unused streams and suffix entries,
                // must lower without silently discarding an authored store.
                for instruction in &published.instructions {
                    if matches!(instruction.operation, AuthoredOperation::End) {
                        continue;
                    }
                    let mut program = ModifierProgram {
                        id: ModifierId {
                            monster: monster as u8,
                            offset: instruction.offset.try_into()?,
                        },
                        operations: Vec::new(),
                        issues: Vec::new(),
                    };
                    lower(&published, &mut program)?;
                    ensure!(
                        program.issues.is_empty(),
                        "disc{disc}/enemy{monster}: {:?}",
                        program.issues
                    );
                    entries += 1;
                }
            }
            ensure!(
                pools > 0 && entries > 0 && uses > 0,
                "no projectile modifiers checked"
            );
            eprintln!(
                "disc{disc}: {pools} modifier pools, {entries} nonempty entry points, {uses} action bindings"
            );
        }
        Ok(())
    }

    #[test]
    fn modifiers_decode_fields_and_preserve_unsupported_destinations() {
        let mut bytes = vec![0; 4];
        bytes.extend([0, 1, 0, 0x24]);
        for value in [2_f32, 3., 4.] {
            bytes.extend(value.to_be_bytes());
        }
        bytes.extend([0, 5, 0, 0x5d, 0, 0, 0, 12]);
        bytes.extend([0, 3, 0, 0x0c, 0, 0, 0, 60]);
        bytes.extend([0, 5, 0, 0x14, 0, 0, 0, 7]);
        bytes.extend([0, 0]);
        let mut program = ModifierProgram {
            id: ModifierId {
                monster: 7,
                offset: 4,
            },
            operations: Vec::new(),
            issues: Vec::new(),
        };
        let mut source = authored_pool(&bytes, &[4]).unwrap();
        source.source_offset = 100;
        let source = serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap();
        lower(&source, &mut program).unwrap();
        assert!(matches!(
            program.operations[0],
            ProjectileOverride::Vector {
                field: ProjectileVector::Velocity,
                value: [2., 3., 4.],
            }
        ));
        assert!(matches!(
            program.operations[1],
            ProjectileOverride::BirthEffect { id: 12 }
        ));
        assert!(matches!(
            program.issues[0].problem,
            ModifierProblem::UnsupportedDestination {
                store: ProjectileStore::Halfword,
                destination: 12
            }
        ));
        assert_eq!(program.issues[0].source_offset, 128);
        assert!(matches!(
            program.issues[1].problem,
            ModifierProblem::UnsupportedDestination {
                store: ProjectileStore::Byte,
                destination: 0x14
            }
        ));
        let previous = serde_json::to_value(&program).unwrap();
        bytes[37] = 2; // Both byte-store encodings have the same semantic diagnostic.
        let mut alias = authored_pool(&bytes, &[4]).unwrap();
        alias.source_offset = 100;
        program.operations.clear();
        program.issues.clear();
        lower(&alias, &mut program).unwrap();
        assert_eq!(serde_json::to_value(&program).unwrap(), previous);
        assert!(source.instructions.iter().any(|instruction| matches!(
            instruction.operation,
            AuthoredOperation::Halfword {
                destination: 12,
                value: 60
            }
        )));
        assert!(matches!(
            source.instructions.last().unwrap().operation,
            AuthoredOperation::End
        ));
        assert!(authored_pool(&bytes[..23], &[]).is_err());
    }

    #[test]
    fn authored_modifiers_require_complete_streams_and_all_physical_hit_keys() -> Result<()> {
        let mut pool = vec![0, 0];
        pool.extend([0, 2, 0, 0x13, 0, 0, 0, 1]);
        pool.extend([0, 0]);
        // An unused, complete second stream remains authored data after End.
        pool.extend([0, 4, 0, 0x24]);
        pool.extend(2f32.to_be_bytes());
        pool.extend([0, 0]);
        let decoded_end = pool.len();
        pool.extend([0xfe, 0xed, 7, 9, 1]);
        assert_eq!(authored_pool(&pool, &[2])?.instructions.len(), 5);

        let mut package = vec![0; 0x280];
        package[16..18].copy_from_slice(&0x200u16.to_be_bytes());
        package[18..20].copy_from_slice(&0x242u16.to_be_bytes());
        package[0x1dc..0x1e0].copy_from_slice(&0x280u32.to_be_bytes());
        // No action references these two physical emissions. Both keys count.
        package[0x202] = (-2i8) as u8;
        // Physical keys remain inspectable when runtime shape/timing admission fails.
        package[0x200..0x202].copy_from_slice(&(-2i16).to_be_bytes());
        package[0x208..0x20c].copy_from_slice(&f32::NAN.to_bits().to_be_bytes());
        package[0x210] = 255;
        package[0x216..0x218].copy_from_slice(&2u16.to_be_bytes());
        package[0x222] = (-2i8) as u8;
        package[0x236..0x238].copy_from_slice(&12u16.to_be_bytes());
        package[0x240..0x242].copy_from_slice(&(-1i16).to_be_bytes());
        package.extend(&pool);
        let range = 0x280..package.len();
        let decoded = authored(&package, range.clone())?;
        assert_eq!(decoded.instructions.len(), 5);
        assert_eq!(decoded.source_size, pool.len());
        assert_eq!(
            decoded.unreferenced_storage,
            [crate::read::Storage {
                offset: decoded_end,
                bytes: pool[decoded_end..].to_vec(),
            }]
        );
        assert!(matches!(
            decoded.instructions[3].operation,
            AuthoredOperation::Float { value: 2., .. }
        ));
        for key in [3u16, 6, decoded_end as u16, pool.len() as u16] {
            package[0x236..0x238].copy_from_slice(&key.to_be_bytes());
            assert!(
                authored(&package, range.clone()).is_err(),
                "must reject physical hit key {key}"
            );
        }
        // The native interpreter accepts an empty stream at a terminal boundary.
        package[0x236..0x238].copy_from_slice(&10u16.to_be_bytes());
        authored(&package, range.clone())?;
        // A malformed-looking key is inactive for contact/projectile emissions and End.
        package[0x236..0x238].copy_from_slice(&u16::MAX.to_be_bytes());
        for emission in [0, 253, 255] {
            package[0x222] = emission;
            authored(&package, range.clone())?;
        }
        package[0x220..0x222].copy_from_slice(&(-1i16).to_be_bytes());
        package[0x222] = 254;
        authored(&package, range)?;

        assert!(
            authored_pool(&pool[2..10], &[]).is_err(),
            "complete instruction without End"
        );
        assert!(
            authored_pool(&[&pool[2..10], &[0xff, 0xff]].concat(), &[]).is_err(),
            "unknown opcode inside active stream"
        );
        assert!(
            authored_pool(&[0xff, 0xff], &[]).is_err(),
            "unknown opcode without a preceding End"
        );
        assert!(
            authored_pool(&[0, 0, 0, 4, 0, 0x24], &[]).is_err(),
            "known suffix opcode with truncated operands"
        );
        assert!(
            authored_pool(&[0, 0, 0], &[]).is_err(),
            "truncated suffix opcode"
        );
        assert!(
            authored_pool(&[0, 0], &[4]).is_err(),
            "out-of-range key without storage suffix"
        );
        Ok(())
    }
}
