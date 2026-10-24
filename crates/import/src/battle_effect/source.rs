//! Cook effect recipes while leaving unsupported members available for diagnostics.
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{
    EffectOperation, ScheduledEvent, SourceBank, UvAnimation, UvChange, UvFrame,
    declaration::Declaration,
};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Copy)]
pub(super) struct UvRecord {
    pub timing: u8,
    pub control: u8,
    pub values: [i16; 4],
}

pub(super) fn uv_animation(rows: &[UvRecord]) -> Result<UvAnimation> {
    let first = rows.first().context("empty effect UV track")?;
    if (128..255).contains(&first.timing) {
        let interval = u32::from(first.timing & 127);
        ensure!(interval > 0, "zero effect UV scroll interval");
        return Ok(UvAnimation::Scroll {
            origin: [first.values[0], first.values[1]],
            step: [first.values[2], first.values[3]],
            interval,
        });
    }
    let mut frames = Vec::new();
    for row in rows {
        if row.timing == 255 {
            let start = usize::from(row.control);
            ensure!(start < frames.len(), "effect UV loop outside animation");
            return Ok(UvAnimation::Frames {
                frames,
                loop_start: Some(start),
            });
        }
        if row.timing == 254 && row.values == [0; 4] {
            frames.push(UvFrame {
                duration: 1,
                change: UvChange::Rectangle { rect: [0; 4] },
            });
            return Ok(UvAnimation::Frames {
                frames,
                loop_start: None,
            });
        }
        ensure!(
            (1..128).contains(&row.timing),
            "unsupported effect UV frame-to-scroll transition"
        );
        frames.push(UvFrame {
            duration: u32::from(row.timing),
            change: if row.values[0] == -32000 {
                UvChange::Palette {
                    index: row.values[1].try_into()?,
                }
            } else {
                UvChange::Rectangle { rect: row.values }
            },
        });
    }
    anyhow::bail!("unterminated effect UV track")
}

pub fn read(bytes: &[u8]) -> Result<SourceBank> {
    read_with_palettes(bytes).map(|(source, _)| source)
}

pub(crate) fn read_with_palettes(bytes: &[u8]) -> Result<(SourceBank, Vec<u8>)> {
    read_with_owner(bytes, None)
}

pub(crate) fn read_enemy_with_palettes(
    bytes: &[u8],
    body: &resonance_content::animation::Skeleton,
) -> Result<(SourceBank, Vec<u8>)> {
    read_with_owner(bytes, Some(body))
}

fn read_with_owner(
    bytes: &[u8],
    body: Option<&resonance_content::animation::Skeleton>,
) -> Result<(SourceBank, Vec<u8>)> {
    ensure!(
        bytes.len() >= 20 && &bytes[..4] == b"ef1\0",
        "invalid battle effect bank"
    );
    let half = |i| u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
    let offsets: Vec<_> = (8..20).step_by(2).map(half).collect();
    ensure!(
        offsets
            .iter()
            .all(|&offset| (20..=bytes.len()).contains(&offset)),
        "battle effect section outside bank"
    );
    let pool = |start| {
        let end = offsets
            .iter()
            .copied()
            .filter(|&i| i > start)
            .min()
            .unwrap_or(bytes.len());
        &bytes[start..end]
    };
    let roots = pool(offsets[4])
        .get(..usize::from(bytes[4]) * 2)
        .context("truncated effect program table")?;
    let programs = super::timelines(pool(offsets[1]), roots)?;
    // The table starts immediately after the data pools, even when an empty pool
    // shares its offset. Do not interpret the table as actor/UV declarations.
    let data_pool = |start| {
        if start == offsets[4] {
            &bytes[start..start]
        } else {
            pool(start)
        }
    };
    // Empty actor pools can share the next pool's start (the stage bank's
    // first bytes are a modifier terminator, not a partial actor declaration).
    let actors = if offsets[1..].contains(&offsets[0]) {
        &bytes[offsets[0]..offsets[0]]
    } else {
        data_pool(offsets[0])
    };
    ensure!(
        actors.len().is_multiple_of(352),
        "misaligned effect actor pool"
    );
    let uv = data_pool(offsets[3]);
    ensure!(uv.len().is_multiple_of(10), "misaligned effect UV pool");
    let uv: Vec<_> = uv
        .chunks_exact(10)
        .map(|row| UvRecord {
            timing: row[0],
            control: row[1],
            values: std::array::from_fn(|i| i16::from_be_bytes([row[2 + 2 * i], row[3 + 2 * i]])),
        })
        .collect();
    let uv_roots = pool(offsets[5])
        .get(..usize::from(bytes[5]) * 2)
        .context("truncated effect UV table")?
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .collect::<Vec<_>>();
    ensure!(
        uv_roots
            .iter()
            .all(|&r| r.is_multiple_of(10) && usize::from(r / 10) < uv.len()),
        "effect UV root outside row pool"
    );
    let palette_strides = actors.chunks_exact(352).map(|record| record[6]).collect();
    let animation_bytes: Vec<_> = actors.chunks_exact(352).map(|record| record[213]).collect();
    let empty: Vec<_> = actors
        .chunks_exact(352)
        .map(super::declaration::empty)
        .collect();
    let retained: Vec<_> = actors
        .chunks_exact(352)
        .map(|record| u32::from_be_bytes(record[20..24].try_into().unwrap()) & 0x40000 != 0)
        .collect();
    let actors = actors
        .chunks_exact(352)
        .map(|record| super::declaration::read(record, &uv))
        .collect::<Result<Vec<_>>>()?;
    let programs = programs
        .into_iter()
        .enumerate()
        .map(|(member, records)| {
            records
                .and_then(|mut records| {
                    let is_empty = |record: &super::Record| {
                        record.command < 252
                            && empty
                                .get(usize::from(record.command))
                                .copied()
                                .unwrap_or(false)
                    };
                    if records.iter().any(is_empty) {
                        ensure!(
                            records.iter().all(|record| {
                                !matches!(record.command, 253 | 255)
                                    && (!is_empty(record)
                                        || (record.argument == 0 && record.operand == 0))
                            }),
                            "modified or retained empty particle"
                        );
                        records.retain(|record| !is_empty(record));
                    }
                    let (records, inherited) = expand_origin_groups(records, body, &retained)?;
                    compile_program(
                        &records,
                        &inherited,
                        &actors,
                        &retained,
                        offsets[2],
                        data_pool(offsets[2]),
                        &animation_bytes,
                    )
                })
                .unwrap_or_else(|error| {
                    vec![ScheduledEvent {
                        at: 0,
                        operation: EffectOperation::Unsupported {
                            reason: format!("effect program {member}: {error:#}"),
                        },
                    }]
                })
        })
        .collect();
    Ok((
        SourceBank {
            document: None,
            art: None,
            source_sha256: crate::digest(bytes),
            programs,
            actors,
        },
        palette_strides,
    ))
}

/// Resolve named groups once, leaving ordinary detached bone births at runtime.
fn expand_origin_groups(
    records: Vec<super::Record>,
    body: Option<&resonance_content::animation::Skeleton>,
    retained: &[bool],
) -> Result<(Vec<super::Record>, std::collections::BTreeSet<usize>)> {
    let grouped =
        |record: &super::Record| record.command < 252 && (250..=253).contains(&record.argument);
    if !records.iter().any(grouped) {
        return Ok((records, Default::default()));
    }
    let body = body.context("effect origin group has no prepared owner body")?;
    ensure!(
        !records
            .iter()
            .any(|record| matches!(record.command, 253 | 255)),
        "effect origin groups with retained edits or repeats are unsupported"
    );
    let mut expanded = Vec::new();
    let mut inherited = std::collections::BTreeSet::new();
    for record in records {
        if !grouped(&record) {
            expanded.push(record);
            continue;
        }
        ensure!(
            !retained
                .get(usize::from(record.command))
                .copied()
                .unwrap_or(false),
            "retained effect origin group is unsupported"
        );
        let prefix = format!("ef{:02}", record.argument - 250);
        let start = expanded.len();
        for (index, bone) in body.bones.iter().enumerate() {
            if bone.name.starts_with(&prefix) {
                ensure!(
                    (1..128).contains(&index),
                    "effect origin group {prefix} has an unsupported bone index {index}"
                );
                if expanded.len() > start {
                    inherited.insert(expanded.len());
                }
                expanded.push(super::Record {
                    argument: index as u8,
                    ..record
                });
            }
        }
        ensure!(
            expanded.len() > start,
            "effect origin group {prefix} is missing"
        );
    }
    Ok((expanded, inherited))
}

fn compile_program(
    records: &[super::Record],
    inherited: &std::collections::BTreeSet<usize>,
    actors: &[Declaration],
    retained: &[bool],
    modifier_start: usize,
    modifier_bytes: &[u8],
    animation_bytes: &[u8],
) -> Result<Vec<ScheduledEvent>> {
    let mut modifiers = BTreeMap::new();
    for record in records {
        if record.command < 252 {
            let actor = actors
                .get(usize::from(record.command))
                .context("effect actor outside declaration pool")?;
            if matches!(
                actor,
                Declaration::Notice { .. } | Declaration::StageTint { .. }
            ) {
                ensure!(
                    record.argument == 0 && record.operand == 0,
                    "unsupported presentation operands"
                );
                continue;
            }
            // Camera declarations do not use particle attachment or modifier fields.
            // Preserve the ignored command bytes without following an offset
            // that does not identify a modifier.
            if matches!(actor, Declaration::CameraShake { .. }) {
                continue;
            }
            if let Declaration::Unsupported { reason } = actor {
                anyhow::bail!("effect declaration {}: {reason}", record.command);
            }
        }
        if record.command < 254
            && record.command != 252
            && record.operand != 0
            && let std::collections::btree_map::Entry::Vacant(entry) =
                modifiers.entry(record.operand)
        {
            let at = usize::from(record.operand)
                .checked_sub(modifier_start)
                .context("effect modifier outside modifier pool")?;
            let words = super::modifier(modifier_bytes, at)?;
            entry.insert(words);
        }
    }
    Ok(super::timeline::decode_grouped(
        records,
        inherited,
        actors,
        retained,
        &modifiers,
        animation_bytes,
    ))
}

pub fn prepare(usual: &[u8], output: &Path) -> Result<[SourceBank; 2]> {
    let (mut banks, strides): (Vec<_>, Vec<_>) = [2, 3]
        .into_iter()
        .map(|member| read_with_palettes(crate::source_assets::section(usual, member)?))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .unzip();
    let art = super::art::publish(usual, &mut banks, &strides, output)?;
    for bank in &mut banks {
        bank.art = Some(art.clone());
    }
    Ok(banks
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected two resident effect banks"))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank(modifier: &[u16]) -> Vec<u8> {
        let mut bytes = vec![0; 20 + 2 * 352];
        bytes[..4].copy_from_slice(b"ef1\0");
        bytes[4] = 2;
        bytes[20] = 4;
        bytes[20 + 50] = 255;
        bytes[20 + 352] = 22;
        bytes[20 + 352 + 16..20 + 352 + 18].copy_from_slice(&(-1_i16).to_be_bytes());
        let events = bytes.len();
        let modifiers = events + 5 * 6;
        // A healthy particle program, then a sound followed by a modified particle.
        for record in [
            [0, 0, 0],
            [1, 254 << 8, 0],
            [0, 252 << 8 | 4, 0],
            [0, 0, modifiers as u16],
            [1, 254 << 8, 0],
        ] {
            bytes.extend(record.into_iter().flat_map(u16::to_be_bytes));
        }
        bytes.extend(modifier.iter().flat_map(|word| word.to_be_bytes()));
        let table = bytes.len();
        bytes.extend([0_u16, 12].into_iter().flat_map(u16::to_be_bytes));
        for (offset, field) in [20, events, modifiers, table, table, bytes.len()]
            .into_iter()
            .zip(bytes[8..20].chunks_exact_mut(2))
        {
            field.copy_from_slice(&(offset as u16).to_be_bytes());
        }
        bytes
    }

    fn isolated(bytes: &[u8], reason: &str) -> Result<()> {
        let source = read(bytes)?;
        assert!(
            source
                .program(0)?
                .iter()
                .any(|event| matches!(event.operation, EffectOperation::Spawn { particle: 0, .. }))
        );
        assert!(matches!(source.actors[1], Declaration::Unsupported { .. }));
        assert!(source.program(1).is_err());
        assert!(matches!(
            source.programs[1].as_slice(),
            [ScheduledEvent { operation: EffectOperation::Unsupported { reason: actual }, .. }]
                if actual.contains(reason)
        ));
        Ok(())
    }

    #[test]
    fn grouped_births_accumulate_shared_samples_and_restart_each_group() -> Result<()> {
        use resonance_content::battle_effect::{BirthField, ParticleBirth, ValueRange};
        let mut bytes = bank(&[
            11,
            0x7ff8,
            80,
            0,
            13,
            0xb0,
            0,
            0,
            0x7ff8,
            0,
            13,
            0xb4,
            0,
            0,
            0x7ff8,
            0,
            2,
            0x7ffc,
            4,
            0,
            5,
            0x7ffc,
            32,
            0,
            3,
            8,
            0x7ffc,
            0,
            u16::MAX,
        ]);
        bytes[20] = 5;
        bytes[28..30].copy_from_slice(&1_i16.to_be_bytes());
        for offset in [176, 180] {
            bytes[20 + offset..24 + offset].copy_from_slice(&16_f32.to_be_bytes());
        }
        let events = u16::from_be_bytes(bytes[10..12].try_into()?) as usize;
        let modifier = u16::from_be_bytes(bytes[12..14].try_into()?);
        bytes[events + 3] = 250;
        bytes[events + 4..events + 6].copy_from_slice(&modifier.to_be_bytes());
        bytes[events + 3 * 6 + 3] = 250;
        let body = resonance_content::animation::Skeleton {
            bones: ["root", "ef00_a", "ef00_b", "ef00_c", "ef00_d"]
                .into_iter()
                .map(|name| resonance_content::animation::Bone {
                    name: name.into(),
                    parent: None,
                    bind_channels: Default::default(),
                    bind: Default::default(),
                })
                .collect(),
        };
        let (bank, _) = read_enemy_with_palettes(&bytes, &body)?;
        let samples = [0.5, 0.5, 0.25, 0.5, 0.75, 0.5, 0.5, 0.];
        let evaluate = |birth: &ParticleBirth, field, direct: Option<ValueRange>| {
            let sample =
                |range: ValueRange| range.sample(range.draw.map_or(0., |i| samples[i as usize]));
            if let Some(sum) = birth.sums.iter().find(|sum| sum.field == field) {
                let mut terms = sum.terms.iter();
                let first = sample(*terms.next().unwrap());
                terms.fold(first, |value, range| value + sample(*range))
            } else {
                sample(direct.unwrap())
            }
        };
        for program in [0, 1] {
            let births: Vec<_> = bank
                .program(program)?
                .iter()
                .filter_map(|event| {
                    if let EffectOperation::Spawn { birth, .. } = &event.operation {
                        Some(birth)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(births.len(), 4);
            for (index, birth) in births.iter().enumerate() {
                assert_eq!(birth.random_draws, 2);
                assert_eq!(birth.origin_bone, Some(index as u8 + 1));
                birth.validate()?;
                assert_eq!(
                    evaluate(birth, BirthField::Uv(0), birth.uv[0]),
                    [65., 129., 193., 193.][index]
                );
                assert!(
                    (evaluate(birth, BirthField::Size(0), birth.size[0])
                        - [16., 20., 16., 16.][index])
                        .abs()
                        < 0.001
                );
                assert_eq!(
                    evaluate(birth, BirthField::Size(0), birth.size[0]),
                    evaluate(birth, BirthField::Size(1), birth.size[1])
                );
                assert!(
                    birth
                        .draw_references()
                        .all(|draw| draw < 2 * (index as u32 + 1))
                );
            }
            assert!(births[0].sums.is_empty());
        }
        Ok(())
    }

    #[test]
    fn stage_tint_keeps_authored_color_duration_and_emission_time() -> Result<()> {
        let mut bytes = bank(&[u16::MAX]);
        let actor = 20 + 352;
        bytes[actor..actor + 352].fill(0);
        bytes[actor] = 24;
        bytes[actor + 16..actor + 18].copy_from_slice(&150_i16.to_be_bytes());
        for (channel, value) in [24_i16, 24, 24, 64].into_iter().enumerate() {
            let at = actor + 24 + channel * 2;
            bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
        }
        bytes[actor + 50] = 255;
        let events = u16::from_be_bytes(bytes[10..12].try_into()?) as usize;
        let emission = events + 3 * 6;
        bytes[emission..emission + 6].copy_from_slice(&[0, 30, 1, 0, 0, 0]);
        bytes[emission + 6..emission + 8].copy_from_slice(&30_i16.to_be_bytes());
        let source = read(&bytes)?;
        assert_eq!(
            source.actors[1],
            Declaration::StageTint {
                color: [24; 3],
                duration: 150,
            }
        );
        assert_eq!(
            source.program(1)?.last(),
            Some(&ScheduledEvent {
                at: 30,
                operation: EffectOperation::StageTint {
                    color: [24; 3],
                    duration: 150,
                },
            })
        );
        // Malformed controls stay isolated to the program that requests them.
        for (at, value) in [
            (actor + 16, 0_u16),
            (actor + 16, u16::MAX),
            (actor + 24, 256),
            (actor + 30, 255),
            (emission + 4, 1),
        ] {
            let mut invalid = bytes.clone();
            invalid[at..at + 2].copy_from_slice(&value.to_be_bytes());
            let source = read(&invalid)?;
            assert!(source.program(0).is_ok());
            assert!(source.program(1).is_err());
        }
        Ok(())
    }

    #[test]
    fn binary_bank_isolates_bad_modifiers_and_unused_declarations() -> Result<()> {
        let healthy = read(&bank(&[u16::MAX]))?;
        assert!(healthy.program(0).is_ok() && healthy.program(1).is_ok());
        let mut empty = bank(&[u16::MAX]);
        let events = u16::from_be_bytes(empty[10..12].try_into()?) as usize;
        let particle = empty[20..20 + 352].to_vec();
        empty[20 + 352..20 + 2 * 352].copy_from_slice(&particle);
        empty[events + 3 * 6 + 2] = 1;
        let expected = read(&empty)?;
        empty[20] = 0;
        let source = read(&empty)?;
        assert_eq!(source.actors.len(), healthy.actors.len());
        assert_eq!(source.programs.len(), healthy.programs.len());
        assert!(source.program(0)?.is_empty());
        assert_eq!(source.program(1)?, expected.program(1)?);
        assert!(matches!(
            source.actors[0].particle_visual()?.geometry,
            resonance_content::battle_effect::visual::ParticleShape::NoDraw
        ));
        for (at, value) in [(20 + 16, 1), (events + 4, 1)] {
            let mut active = empty.clone();
            active[at] = value;
            let source = read(&active)?;
            assert!(source.program(0).is_err());
            assert_eq!(source.program(1)?, expected.program(1)?);
        }
        let body = resonance_content::animation::Skeleton {
            bones: ["root", "ef00_a", "ef01_a", "ef02a", "ef03_a", "ef00_b"]
                .into_iter()
                .map(|name| resonance_content::animation::Bone {
                    name: name.into(),
                    parent: None,
                    bind_channels: Default::default(),
                    bind: Default::default(),
                })
                .collect(),
        };
        let mut grouped = bank(&[u16::MAX]);
        let events = u16::from_be_bytes(grouped[10..12].try_into()?) as usize;
        grouped[events..events + 2].copy_from_slice(&3_i16.to_be_bytes());
        grouped[events + 6..events + 8].copy_from_slice(&4_i16.to_be_bytes());
        for (selector, expected) in [
            (250, vec![1, 5]),
            (251, vec![2]),
            (252, vec![3]),
            (253, vec![4]),
        ] {
            grouped[events + 3] = selector;
            let (source, _) = read_enemy_with_palettes(&grouped, &body)?;
            let births: Vec<_> = source
                .program(0)?
                .iter()
                .map(|event| {
                    let EffectOperation::Spawn { birth, .. } = &event.operation else {
                        panic!("expected grouped particle birth");
                    };
                    assert_eq!(event.at, 3);
                    birth.origin_bone.unwrap()
                })
                .collect();
            assert_eq!(births, expected);
            assert_eq!(source.program(1)?, healthy.program(1)?);
            assert!(read(&grouped)?.program(0).is_err());
        }
        grouped[events + 3] = 250;
        let mut missing = body.clone();
        missing.bones.retain(|bone| !bone.name.starts_with("ef00"));
        assert!(
            read_enemy_with_palettes(&grouped, &missing)?
                .0
                .program(0)
                .is_err()
        );
        grouped[20 + 20..20 + 24].copy_from_slice(&0x40000_u32.to_be_bytes());
        assert!(
            read_enemy_with_palettes(&grouped, &body)?
                .0
                .program(0)
                .is_err()
        );
        for (words, reason) in [
            (&[31][..], "unknown effect modifier"),
            (&[7][..], "truncated effect modifier operands"),
            (&[0, 8, 2, 0][..], "truncated effect modifier"),
        ] {
            isolated(&bank(words), reason)?;
        }
        // Invalid edits remain unsupported when a repeat begins at the program end.
        let mut bytes = bank(&[20, 0, 0, 0, 0, 0, 0, 0, u16::MAX]);
        let events = u16::from_be_bytes(bytes[10..12].try_into()?) as usize;
        bytes[events + 2 * 6..events + 3 * 6].copy_from_slice(&[0, 1, 255, 1, 0, 1]);
        isolated(&bytes, "unsupported particle operation")?;
        Ok(())
    }

    #[test]
    fn binary_bank_isolates_bad_programs_and_referenced_declarations() -> Result<()> {
        let base = bank(&[u16::MAX]);
        let events = u16::from_be_bytes(base[10..12].try_into()?) as usize;
        let emission = events + 3 * 6;
        let end = events + 4 * 6;
        for (at, value, reason) in [
            (
                emission + 2,
                253,
                "delayed particle edit has no retained birth",
            ),
            (emission + 2, 1, "negative camera shake duration"),
            (emission + 2, 250, "effect actor outside declaration pool"),
            (emission + 2, 255, "effect repeat requires"),
            (end + 2, 252, "unterminated effect program"),
        ] {
            let mut bytes = base.clone();
            bytes[at] = value;
            isolated(&bytes, reason)?;
        }
        let mut bytes = base;
        bytes[emission + 4..emission + 6].copy_from_slice(&1_u16.to_be_bytes());
        isolated(&bytes, "effect modifier outside modifier pool")
    }

    #[test]
    fn model_animation_birth_requires_matching_default_loop() -> Result<()> {
        for model in 0..4 {
            let words = [
                22,
                0x5c,
                model,
                0,
                0,
                0,
                0,
                0,
                3,
                0x7ffc,
                model,
                0,
                10,
                0xd4,
                0x7ffc,
                0,
                u16::MAX,
            ];
            let make = |words: &[u16]| {
                let mut bytes = bank(words);
                bytes[20] = 3;
                bytes[20 + 2] = 6;
                bytes[20 + 20..20 + 24].copy_from_slice(&0x20_u32.to_be_bytes());
                bytes
            };
            let source = read(&make(&words))?;
            let EffectOperation::Spawn { birth, .. } = &source.program(1)?[1].operation else {
                panic!("model birth")
            };
            assert_eq!(birth.animation.unwrap().clip, 0);
            assert!(birth.animation.unwrap().repeat);
            assert_eq!(birth.model.unwrap().min, f32::from(model));
            for (index, value) in [(4, 1), (5, 1000), (6, 4), (10, (model + 1) % 4)] {
                let mut unsupported = words;
                unsupported[index] = value;
                assert!(read(&make(&unsupported))?.program(1).is_err());
            }
        }
        for (animation, flags, accepted) in [
            (0, 3_u32, true),
            (253, 3, true),
            (254, 3, false),
            (0, 7, false),
        ] {
            let mut bytes = bank(&[2, 0x7ffc, 3, 0, 3, 0xd4, 0x7ffc, 0, u16::MAX]);
            bytes[20] = 3;
            bytes[22] = 6;
            bytes[40..44].copy_from_slice(&flags.to_be_bytes());
            bytes[232] = 4;
            bytes[233] = animation;
            let source = read(&bytes)?;
            if !accepted {
                assert!(source.program(1).is_err());
                continue;
            }
            let EffectOperation::Spawn { birth, .. } = &source.program(1)?[1].operation else {
                panic!("model birth")
            };
            let Declaration::ModelParticle { binding, .. } = &source.actors[0] else {
                panic!("model declaration")
            };
            assert_eq!(binding.slot, 4);
            assert!(binding.animation.is_none() && birth.model.is_none());
            assert_eq!(birth.random_draws, 1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod uv_tests {
    use super::*;

    fn row(timing: u8, values: [i16; 4]) -> UvRecord {
        UvRecord {
            timing,
            control: 0,
            values,
        }
    }

    #[test]
    fn decoding_resolves_palette_changes_loops_and_blank_end_frames() {
        let frames = vec![
            UvFrame {
                duration: 2,
                change: UvChange::Palette { index: 27 },
            },
            UvFrame {
                duration: 4,
                change: UvChange::Rectangle {
                    rect: [1, 2, 30, 30],
                },
            },
        ];
        let mut rows = vec![
            row(2, [-32000, 27, 0, 0]),
            row(4, [1, 2, 30, 30]),
            row(255, [0; 4]),
        ];
        assert_eq!(
            uv_animation(&rows).unwrap(),
            UvAnimation::Frames {
                frames: frames.clone(),
                loop_start: Some(0),
            }
        );
        rows[2] = row(254, [0; 4]);
        let mut ending = frames;
        ending.push(UvFrame {
            duration: 1,
            change: UvChange::Rectangle { rect: [0; 4] },
        });
        assert_eq!(
            uv_animation(&rows).unwrap(),
            UvAnimation::Frames {
                frames: ending,
                loop_start: None,
            }
        );
        assert_eq!(
            uv_animation(&[row(130, [10, 20, 2, -1])]).unwrap(),
            UvAnimation::Scroll {
                origin: [10, 20],
                step: [2, -1],
                interval: 2,
            }
        );
    }

    #[test]
    fn decoding_rejects_invalid_or_unsupported_tracks() {
        for rows in [
            vec![],
            vec![row(1, [0; 4])],
            vec![row(255, [0; 4])],
            vec![row(0, [0; 4]), row(255, [0; 4])],
            vec![row(1, [-32000, -1, 0, 0]), row(255, [0; 4])],
            vec![row(1, [0; 4]), row(129, [0, 0, 1, 1])],
            vec![row(128, [0; 4])],
        ] {
            assert!(uv_animation(&rows).is_err());
        }
    }
}
