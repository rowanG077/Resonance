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
    let programs = super::timelines(bytes)?;
    let half = |i| u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
    let offsets: Vec<_> = (8..20).step_by(2).map(half).collect();
    let pool = |start| {
        let end = offsets
            .iter()
            .copied()
            .filter(|&i| i > start)
            .min()
            .unwrap_or(bytes.len());
        &bytes[start..end]
    };
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
                    let records = expand_origin_groups(records, body, &retained)?;
                    compile_program(
                        &records,
                        &actors,
                        &retained,
                        offsets[2],
                        data_pool(offsets[2]),
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
) -> Result<Vec<super::Record>> {
    let grouped =
        |record: &super::Record| record.command < 252 && (250..=253).contains(&record.argument);
    if !records.iter().any(grouped) {
        return Ok(records);
    }
    let body = body.context("effect origin group has no prepared owner body")?;
    ensure!(
        !records
            .iter()
            .any(|record| matches!(record.command, 253 | 255)),
        "effect origin groups with retained edits or repeats are unsupported"
    );
    let mut expanded = Vec::new();
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
    Ok(expanded)
}

fn compile_program(
    records: &[super::Record],
    actors: &[Declaration],
    retained: &[bool],
    modifier_start: usize,
    modifier_bytes: &[u8],
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
        if record.command < 254 && record.command != 252 && record.operand != 0 {
            let at = usize::from(record.operand)
                .checked_sub(modifier_start)
                .context("effect modifier outside modifier pool")?;
            let words = super::modifier(modifier_bytes, at)?;
            modifiers.insert(record.operand, words);
        }
    }
    Ok(super::timeline::decode(
        records, actors, retained, &modifiers,
    ))
}

pub fn publish(usual: &[u8], output: &Path, prefix: &str) -> Result<Vec<String>> {
    let (mut banks, strides): (Vec<_>, Vec<_>) = [2, 3]
        .into_iter()
        .map(|member| read_with_palettes(crate::source_assets::section(usual, member)?))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .unzip();
    let art = super::art::publish(usual, &mut banks, &strides, output)?;
    let mut paths = ["common", "techniques"]
        .into_iter()
        .zip(banks)
        .map(|(name, mut bank)| {
            bank.art = Some(art.clone());
            let path = format!("{prefix}/effects/{name}.json");
            crate::write_atomic(&output.join(&path), &serde_json::to_vec(&bank)?)?;
            Ok(path)
        })
        .collect::<Result<Vec<_>>>()?;
    paths.extend(art.files.into_keys());
    Ok(paths)
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
    fn cached_metadata_recompiles_programs_and_preserves_palette_bindings() -> Result<()> {
        use resonance_content::battle_effect::{Art, visual::ParticleTexture};
        let bytes = bank(&[u16::MAX]);
        let expected = read(&bytes)?;
        let mut cached = expected.clone();
        cached.programs.clear();
        cached.actors[0].template_mut()?.lifetime = Some(999);
        let Declaration::Particle { visual, .. } = &mut cached.actors[0] else {
            panic!("expected particle fixture")
        };
        let ParticleTexture::Atlas { palette_set, .. } = &mut visual.texture else {
            panic!("expected texture fixture")
        };
        *palette_set = 3;
        cached.art = Some(Art {
            source_sha256: crate::digest(b"retained artwork"),
            textures: BTreeMap::new(),
            models: BTreeMap::new(),
            files: BTreeMap::new(),
        });
        let artwork = serde_json::to_value(&cached.art)?;
        super::super::refresh_cached(&bytes, &mut cached, None)?;
        assert_eq!(cached.programs, expected.programs);
        assert_eq!(cached.actors[0].template()?, expected.actors[0].template()?);
        assert_eq!(cached.actors[0].particle_visual()?.texture.palette_set(), 3);
        assert_eq!(serde_json::to_value(&cached.art)?, artwork);
        cached.source_sha256 = "0".repeat(64);
        assert!(super::super::refresh_cached(&bytes, &mut cached, None).is_err());
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
    fn binary_bank_rejects_corrupt_shared_bounds() {
        let base = bank(&[u16::MAX]);
        let events = u16::from_be_bytes(base[10..12].try_into().unwrap());
        let table = u16::from_be_bytes(base[16..18].try_into().unwrap()) as usize;
        for (at, value) in [(0, b'x'), (4, 3), (5, 1)] {
            let mut bytes = base.clone();
            bytes[at] = value;
            assert!(read(&bytes).is_err(), "byte {at}");
        }
        for (at, value) in [(8, u16::MAX), (10, events + 1), (table + 2, u16::MAX)] {
            let mut bytes = base.clone();
            bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
            assert!(read(&bytes).is_err(), "offset {at}");
        }
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
