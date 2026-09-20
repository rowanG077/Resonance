//! Recover source-wide arte identities and programs without loading their media.
use super::{
    action_program::decode_commands,
    actions::{self, Rel, member},
};
use crate::read::{f32 as float, u16 as half, u32 as word};
use anyhow::{Context, Result, ensure};
use resonance_content::battle::arte_inventory::*;
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const DATA: usize = 5;
const NORMAL_TABLE: usize = 0x3a48;
const MARTIAL_DISPATCH: usize = 0xd60;
const MARTIAL_DISPATCH_COUNT: usize = 160;
const NATIVE_DISPATCH: usize = 0x1238;
const COMBINED_DISPATCH: usize = 0xf94;
const MAGIC_OFFSETS: usize = 0xfe0;
const MAGIC_OFFSET_COUNT: usize = 121;
const PHASE_BYTES: usize = 28;

pub(super) fn recover(extracted: &Path) -> Result<ArteInventory> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let catalogue = crate::arte::read(&executable)?;
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat"))?;
    let mut magic = fs::File::open(extracted.join("files/BTL/BTLmagic.dat"))?;
    let magic_size = magic.metadata()?.len();
    let party = (1..=9)
        .map(|character| {
            Ok(PartyArteInventory {
                character,
                artes: catalogue
                    .learned_by(character)?
                    .iter()
                    .map(|&id| u16::from(id))
                    .collect(),
                normals: normals(&rel, character)?,
                casting_voices: super::casting_voices::inventory(&rel, &usual, character)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let artes = catalogue
        .definitions
        .iter()
        .enumerate()
        .map(|(id, row)| {
            let native_id = u16::try_from(row.native_id).context("negative native arte ID")?;
            let flags = row.flags;
            Ok(ArteRecord {
                id: id as u16,
                native_id,
                name: row.name.clone().unwrap_or_default(),
                owners: party
                    .iter()
                    .filter(|p| p.artes.contains(&(id as u16)))
                    .map(|p| p.character)
                    .collect(),
                cost: if native_id == 34 {
                    ArteCost::MaximumTpPercent(row.tp_cost)
                } else {
                    ArteCost::Tp(row.tp_cost)
                },
                properties: actions::technique_properties(row)?,
                targeting: ArteTargetMetadata {
                    spread_class: row.target_preference,
                    condition_mask: row.target_condition_mask,
                },
                category: row.menu_category,
                element: row.element,
                route: match row.learning_route {
                    0 => LearningRoute::Either,
                    1 => LearningRoute::Technical,
                    2 => LearningRoute::Strike,
                    3 => LearningRoute::Event,
                    value => anyhow::bail!("unknown arte learning route {value}"),
                },
                level: row.required_level,
                prerequisite: nonzero(
                    row.learning_parent
                        .try_into()
                        .context("negative learning parent")?,
                ),
                technical_successor: nonzero(
                    row.technical_successor
                        .try_into()
                        .context("negative technical successor")?,
                ),
                strike_successor: nonzero(
                    row.strike_successor
                        .try_into()
                        .context("negative strike successor")?,
                ),
                alternatives: row
                    .mutually_exclusive
                    .iter()
                    .filter(|&&id| id != 0)
                    .map(|&id| u16::try_from(id).context("negative excluded arte"))
                    .collect::<Result<_>>()?,
                cast_time_adjustment: row.cast_time_adjustment,
                loads_resource: flags & 1 != 0,
                source_flags: flags,
                implementation: match native_id {
                    0 => ArteImplementation::Dummy,
                    1..200 => ArteImplementation::Martial(native_id),
                    _ => ArteImplementation::Native(native_id),
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // All callback tables delimit one another, including combined Unison programs.
    let dispatches = NATIVE_IDS
        .filter_map(|id| rel.pointer(DATA, native_dispatch_field(id)).ok())
        .chain(
            (0..MARTIAL_DISPATCH_COUNT)
                .filter_map(|i| rel.pointer(DATA, MARTIAL_DISPATCH + i * 4).ok()),
        )
        .chain(
            rel.local_targets()
                .iter()
                .copied()
                .filter(|&(section, _)| section == DATA),
        )
        .collect::<BTreeSet<_>>();
    let martial = artes
        .iter()
        .filter_map(|a| match a.implementation {
            ArteImplementation::Martial(id) => Some(id),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|native_id| {
            Ok(MartialArteInventory {
                native_id,
                phases: bundle(&usual, native_id, 8, native_id).phases,
                dispatch: callback_dispatch(
                    &rel,
                    MARTIAL_DISPATCH + usize::from(native_id) * 4,
                    &dispatches,
                )?,
            })
        })
        .collect::<Result<_>>()?;
    let native = NATIVE_IDS
        .map(|native_id| {
            let index = usize::from(native_id - NATIVE_IDS.start);
            let combined = COMBINED_NATIVE_IDS.contains(&native_id);
            let dispatch = callback_dispatch(&rel, native_dispatch_field(native_id), &dispatches)?;
            let requested = !matches!(dispatch, NativeDispatch::Absent)
                && (combined
                    || artes
                        .iter()
                        .any(|arte| arte.native_id == native_id && arte.loads_resource));
            let resource = recovered(
                RecoveryStage::ExternalResource,
                magic_resource(&rel, index, magic_size, requested),
            );
            let mut bundle = if combined {
                ArteBundle {
                    native_id,
                    phases: if native_id == COMBINED_NATIVE_IDS.start {
                        Recovery::Absent
                    } else {
                        Recovery::Unresolved {
                            stage: RecoveryStage::Bundle,
                            diagnostic: "combined native has no recovered external action bundle"
                                .into(),
                        }
                    },
                }
            } else {
                bundle(&usual, native_id, 9, index as u16)
            };
            let assets = if let Recovery::Recovered(Some(range)) = &resource {
                recovered(
                    RecoveryStage::ExternalResource,
                    (|| {
                        magic.seek(SeekFrom::Start(u64::from(range.offset)))?;
                        let mut package = vec![0; range.length as usize];
                        magic.read_exact(&mut package)?;
                        let assets = native_assets(&package)?;
                        if let Some(offset) = assets.action_bundle_offset {
                            let end = (1..69)
                                .map(|field| word(&package, field * 4))
                                .collect::<Result<Vec<_>>>()?
                                .into_iter()
                                .filter(|&pointer| pointer > offset)
                                .min()
                                .map_or(package.len(), |v| v as usize);
                            let source = package
                                .get(offset as usize..end)
                                .context("native bundle outside magic resource")?;
                            bundle.phases = recovered(
                                RecoveryStage::Bundle,
                                phases(source, |at| ProgramSource::MagicArchive {
                                    native_id,
                                    offset: offset + at as u32,
                                }),
                            );
                        }
                        Ok(Some(assets))
                    })(),
                )
            } else {
                Recovery::Recovered(None)
            };
            Ok(NativeArteInventory {
                native_id,
                dispatch,
                bundle,
                resource,
                assets,
            })
        })
        .collect::<Result<_>>()?;
    let inventory = ArteInventory {
        artes,
        party,
        martial,
        native,
    };
    inventory.validate()?;
    Ok(inventory)
}

fn normals(rel: &Rel, character: u8) -> Result<Vec<NormalBranchInventory>> {
    let table = NORMAL_TABLE + usize::from(character - 1) * 12;
    let bundles = rel.pointer(DATA, table)?;
    let rules = rel.at(rel.pointer(DATA, table + 4)?)?;
    let selectors = rel.at(rel.pointer(DATA, table + 8)?)?;
    (0..7)
        .map(|selection| {
            let selector = selectors
                .get(selection * 4..selection * 4 + 4)
                .context("truncated normal selection table")?;
            let bundle = bundles.1 + usize::from(selector[0]) * 16;
            let pointer = |field: usize| rel.pointer(bundles.0, bundle + field * 4);
            let descriptor = rel
                .at(pointer(0)?)?
                .get(..24)
                .context("truncated normal descriptor")?;
            let animations = pointer(2)?;
            let commands = pointer(3)?;
            let hits = pointer(1)?;
            Ok(NormalBranchInventory {
                selection: selection as u8,
                bundle: selector[0],
                followup_directions: selector[1],
                fallback: (selector[2] != 255).then_some(selector[2]),
                duration: half(descriptor, 0)?,
                recovery_ticks: half(descriptor, 2)?,
                combo_first: half(descriptor, 4)?,
                combo_second: nonzero(half(descriptor, 6)?),
                buffer_until: descriptor[8],
                recovery_animation: (descriptor[9] != 0).then_some(descriptor[9]),
                recovery_rate: float(descriptor, 12)?,
                startup_effect: effect(word(descriptor, 16)?)?,
                reach: half(descriptor, 20)?,
                airborne_reach: half(descriptor, 22)?,
                programs: ActionPrograms {
                    animations: SourceProgram {
                        source: rel_source(animations)?,
                        recovery: recovered(
                            RecoveryStage::Animations,
                            rel.at((animations.0, 0))
                                .and_then(|bytes| actions::animations_at(bytes, animations.1)),
                        ),
                    },
                    commands: SourceProgram {
                        source: rel_source(commands)?,
                        recovery: recovered(
                            RecoveryStage::Commands,
                            rel.at(commands).and_then(decode_commands),
                        ),
                    },
                    hits: SourceProgram {
                        source: rel_source(hits)?,
                        recovery: recovered(
                            RecoveryStage::Hits,
                            rel.at(hits).and_then(|bytes| actions::hits(bytes, rules)),
                        ),
                    },
                },
            })
        })
        .collect()
}

fn bundle(usual: &[u8], native_id: u16, table: u8, record: u16) -> ArteBundle {
    let value = (|| {
        phases(
            member(member(usual, usize::from(table))?, usize::from(record))?,
            |offset| ProgramSource::BattleUsual {
                member: table,
                record,
                offset: offset as u32,
            },
        )
    })();
    ArteBundle {
        native_id,
        phases: recovered(RecoveryStage::Bundle, value),
    }
}

fn phases(
    source: &[u8],
    location: impl Fn(usize) -> ProgramSource,
) -> Result<Vec<ArtePhaseInventory>> {
    ensure!(
        source.len() >= 16 + PHASE_BYTES * 4,
        "truncated arte phase descriptors"
    );
    // Rule-only native bundles place all three empty program tables at the same
    // end offset. Alignment padding following that offset is not initialized.
    let empty_programs =
        word(source, 4)? == word(source, 8)? && word(source, 8)? == word(source, 12)?;
    (0..4)
        .map(|phase| {
            let descriptor = &source[16 + phase * PHASE_BYTES..16 + (phase + 1) * PHASE_BYTES];
            let offset = |kind: usize, stride| -> Result<usize> {
                let index = word(descriptor, 12 + kind * 4)? as usize;
                (word(source, kind * 4)? as usize)
                    .checked_add(
                        index
                            .checked_mul(stride)
                            .context("arte program offset overflow")?,
                    )
                    .context("arte program offset overflow")
            };
            let rules = offset(0, 28)?;
            let hits = offset(1, 32)?;
            let animations = offset(2, 12)?;
            let commands = offset(3, 2)?;
            let rule_end = (1..4)
                .map(|kind| word(source, kind * 4))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .filter(|&base| base as usize >= rules)
                .min()
                .context("unbounded arte hit-rule table")? as usize;
            let at = |offset| {
                source
                    .get(offset..)
                    .context("arte program outside its record")
            };
            Ok(ArtePhaseInventory {
                phase: phase as u8,
                duration: half(descriptor, 0)?,
                recovery_ticks: half(descriptor, 2)?,
                buffer_until: half(descriptor, 4)?,
                combo_at: half(descriptor, 6)?,
                startup_effect: effect(word(descriptor, 8)?)?,
                hit_rules: SourceProgram {
                    source: location(rules),
                    recovery: recovered(
                        RecoveryStage::Hits,
                        (|| {
                            let bytes = source
                                .get(rules..rule_end)
                                .context("arte hit rules outside record")?;
                            ensure!(bytes.len() % 28 == 0, "misaligned arte hit-rule table");
                            bytes.chunks_exact(28).map(actions::hit_rule).collect()
                        })(),
                    ),
                },
                programs: ActionPrograms {
                    animations: SourceProgram {
                        source: location(animations),
                        recovery: if empty_programs && word(descriptor, 20)? == 0 {
                            Recovery::Absent
                        } else {
                            recovered(
                                RecoveryStage::Animations,
                                actions::animations_at(source, animations),
                            )
                        },
                    },
                    commands: SourceProgram {
                        source: location(commands),
                        recovery: if empty_programs && word(descriptor, 24)? == 0 {
                            Recovery::Absent
                        } else {
                            recovered(
                                RecoveryStage::Commands,
                                at(commands).and_then(decode_commands),
                            )
                        },
                    },
                    hits: SourceProgram {
                        source: location(hits),
                        recovery: if empty_programs && word(descriptor, 16)? == 0 {
                            Recovery::Absent
                        } else {
                            recovered(
                                RecoveryStage::Hits,
                                at(hits).and_then(|bytes| actions::hits(bytes, at(rules)?)),
                            )
                        },
                    },
                },
            })
        })
        .collect::<Result<_>>()
}

fn native_dispatch_field(native_id: u16) -> usize {
    let (table, first) = if COMBINED_NATIVE_IDS.contains(&native_id) {
        (COMBINED_DISPATCH, COMBINED_NATIVE_IDS.start)
    } else {
        (NATIVE_DISPATCH, SPELL_NATIVE_IDS.start)
    };
    table + usize::from(native_id - first) * 4
}

fn callback_dispatch(
    rel: &Rel,
    field: usize,
    starts: &BTreeSet<(usize, usize)>,
) -> Result<NativeDispatch> {
    let start = match rel.pointer(DATA, field) {
        Ok(start) => start,
        Err(error) => {
            return Ok(if word(rel.at((DATA, field))?, 0)? == 0 {
                NativeDispatch::Absent
            } else {
                NativeDispatch::Unresolved {
                    diagnostic: error.to_string(),
                }
            });
        }
    };
    let end = starts
        .iter()
        .find(|&&(section, offset)| section == start.0 && offset > start.1)
        .map_or(start.1 + 128, |&(_, offset)| offset.min(start.1 + 128));
    let mut branches = Vec::new();
    for offset in (start.1..end).step_by(4) {
        match rel.pointer(start.0, offset) {
            Ok((1, handler)) => branches.push(NativeCallback {
                state: branches.len() as u8,
                handler: handler as u32,
                recovery: CallbackRecovery::BodyAndRequirementsUnresolved,
            }),
            _ if !branches.is_empty() => break,
            other => {
                return Ok(NativeDispatch::Unresolved {
                    diagnostic: format!(
                        "dispatch +{field:#x} callback at {}:{offset:#x}: {other:?}",
                        start.0
                    ),
                });
            }
        }
    }
    ensure!(
        !branches.is_empty(),
        "empty relocated native callback table"
    );
    Ok(NativeDispatch::Callbacks {
        source: rel_source(start)?,
        branches,
        control_flow: NativeControlFlow::InternalBranchesUnresolved,
    })
}

fn magic_resource(
    rel: &Rel,
    index: usize,
    archive_size: u64,
    requested: bool,
) -> Result<Option<ArteResource>> {
    let table = rel.at((DATA, MAGIC_OFFSETS))?;
    let offset = word(table, index * 4)?;
    // Spread is the first stored resource and legitimately starts at byte zero.
    if offset == 0 && !requested {
        return Ok(None);
    }
    let end = (index + 1..MAGIC_OFFSET_COUNT)
        .map(|i| word(table, i * 4))
        .find(|value| !matches!(value, Ok(0)))
        .context("unterminated magic resource range")??;
    ensure!(
        end > offset && u64::from(end) <= archive_size,
        "magic resource outside archive"
    );
    Ok(Some(ArteResource {
        archive: ArteArchive::Magic,
        offset,
        length: end - offset,
    }))
}

fn native_assets(package: &[u8]) -> Result<NativeAssetRequirements> {
    ensure!(
        package.len() >= 276,
        "truncated native arte resource header"
    );
    let pointer = |field| -> Result<Option<u32>> {
        let offset = word(package, field)?;
        ensure!(
            offset == 0 || (276..package.len()).contains(&(offset as usize)),
            "native resource pointer +{field:#x} outside package"
        );
        Ok((offset != 0).then_some(offset))
    };
    let mut models = Vec::new();
    for slot in 0..10 {
        if let Some(model_offset) = pointer(12 + slot * 4)? {
            let mut animations = Vec::new();
            for animation in 0..4 {
                if let Some(offset) = pointer(92 + slot * 16 + animation * 4)? {
                    animations.push(CallbackResource {
                        slot: animation as u8,
                        offset,
                    });
                }
            }
            models.push(NativeModelRequirement {
                slot: slot as u8,
                model_offset,
                outline_offset: pointer(52 + slot * 4)?,
                animations,
            });
        }
    }
    let mut callback_resources = Vec::new();
    for slot in 0..4 {
        if let Some(offset) = pointer(260 + slot * 4)? {
            callback_resources.push(CallbackResource {
                slot: slot as u8,
                offset,
            });
        }
    }
    Ok(NativeAssetRequirements {
        effect_program_offset: pointer(4)?.context("native arte has no effect program")?,
        texture_archive_offset: pointer(8)?,
        models,
        projectile_recipes_offset: pointer(252)?,
        action_bundle_offset: pointer(256)?,
        callback_resources,
    })
}

fn recovered<T>(stage: RecoveryStage, value: Result<T>) -> Recovery<T> {
    match value {
        Ok(value) => Recovery::Recovered(value),
        Err(error) => Recovery::Unresolved {
            stage,
            diagnostic: format!("{error:#}"),
        },
    }
}

fn rel_source((section, offset): (usize, usize)) -> Result<ProgramSource> {
    let section = match section {
        1 => RelSection::Text,
        4 => RelSection::Rodata,
        5 => RelSection::Data,
        _ => anyhow::bail!("unsupported action source section {section}"),
    };
    Ok(ProgramSource::Rel {
        section,
        offset: u32::try_from(offset)?,
    })
}

fn nonzero(value: u16) -> Option<u16> {
    (value != 0).then_some(value)
}

fn effect(value: u32) -> Result<Option<u16>> {
    if value == u32::MAX || value == 0 {
        return Ok(None);
    }
    Ok(Some(
        u16::try_from(value).context("invalid arte startup effect")?,
    ))
}
