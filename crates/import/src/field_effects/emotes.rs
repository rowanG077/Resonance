use super::*;
use resonance_content::effect::EmoteRotation;

pub(super) fn read(executable: &[u8]) -> Result<BTreeMap<u16, EmoteTrack>> {
    let value = |address| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(executable, address, 4)?.try_into()?,
        ))
    };
    let double = |address| -> Result<f64> {
        Ok(f64::from_be_bytes(
            dol::slice(executable, address, 8)?.try_into()?,
        ))
    };
    let height = value(0x8035B118)?;
    let anchor = dol::text(executable, 0x8017A498)?;
    let missing_anchor_offset = [0., 0., value(0x8035AFC8)? - height];
    let right = value(0x8035B0B4)?;
    let ring_z = height + value(0x8035B058)?;
    let mut emotes = BTreeMap::new();
    for kind in [0, 1, 2, 3, 4, 5, 6, 10, 11, 12, 14] {
        let mut frames = Vec::new();
        let (intro_ticks, cycle_ticks, phase_count): (usize, usize, u8) = match kind {
            0 => (64, 1, 16),
            2 => (1, 16, 8),
            3 => (16, 16, 1),
            4 => (80, 1, 16),
            11 => (1, 20, 1),
            _ => (24, 24, 1),
        };
        for age in 0..intro_ticks + cycle_ticks {
            for phase in 0..usize::from(phase_count) {
                let mut sprites = Vec::new();
                let mut emit = |offset, size, uv: [f32; 4], rotation| {
                    sprites.push(Sprite {
                        offset,
                        size,
                        uv: uv.map(|v| v / 256.),
                        rotation,
                        vertical_anchor: VerticalAnchor::Center,
                        alpha: 255,
                    });
                };
                // The first update initializes the controller before drawing.
                if age > 0 && matches!(kind, 0 | 4) {
                    let ring_size = (2 + age * 10).min(80) as f32;
                    emit([right, 0., ring_z], [ring_size; 2], [0., 0., 96., 96.], 0.);
                    // Marks appear after each seeded 16-tick boundary, then stay full.
                    let count =
                        ((age + phase) / 16 - (phase + 1) / 16).min(if kind == 0 { 3 } else { 4 });
                    for mark in 0..count {
                        let (x, z, u, v) = if kind == 0 {
                            (
                                12. + mark as f32 * 12.,
                                height + value(0x8035B11C)?,
                                223.,
                                144.,
                            )
                        } else {
                            (
                                10. + mark as f32 * 12.,
                                height + value(0x8035B124)?,
                                192.,
                                112.,
                            )
                        };
                        emit([x, 0., z], [52.; 2], [u, v, u + 32., v + 32.], 0.);
                    }
                } else if age > 0 && kind == 2 {
                    // Five independently sized/rotated strokes alternate two authored poses.
                    let pose = ((age - 1 + phase) / 8) % 2;
                    for index in 0..5 {
                        let address = 0x801E3938 + ((pose * 5 + index) * 20) as u32;
                        let row = dol::slice(executable, address, 20)?;
                        let u = f32::from(row[12]);
                        let v = f32::from(row[13]);
                        emit(
                            [
                                value(address)?,
                                value(address + 4)?,
                                value(0x8035B124)? + (height + value(address + 8)?),
                            ],
                            [
                                u16::from_be_bytes([row[16], row[17]]) as f32,
                                i16::from_be_bytes([row[18], row[19]]) as f32,
                            ],
                            [u, v, u + 32., v + 32.],
                            i16::from_be_bytes([row[14], row[15]]) as f32,
                        );
                    }
                } else if age > 0 && kind == 11 {
                    let elapsed = (age - 1) % 20;
                    for (index, (horizontal, vertical)) in [
                        (0x8035B0D8, 0x8035B140),
                        (0x8035B144, 0x8035B148),
                        (0x8035B01C, 0x8035B14C),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let address = 0x801E3A00 + index as u32 * 12;
                        let mut position =
                            [value(address)?, value(address + 4)?, value(address + 8)?];
                        let mut velocity = value(vertical)?;
                        for _ in 0..elapsed {
                            position[0] += value(horizontal)?;
                            position[2] += velocity;
                            // Gravity is a double operation rounded back to a float each tick.
                            velocity = (f64::from(velocity) - double(0x8035B030)?) as f32;
                        }
                        position[2] = position[2] + height + value(0x8035B054)?;
                        // The helper adds six to the controller's requested size of sixteen.
                        emit(
                            position,
                            [22.; 2],
                            [223., 112., 255., 144.],
                            if index == 2 { 105. } else { 135. },
                        );
                    }
                } else if age > 0 && kind == 3 {
                    // Case 3 draws the two-frame sweat glyph without a balloon.
                    let u = ((age / 8) % 2) as f32 * 48.;
                    emit(
                        [value(0x8035B0B8)?, 0., ring_z - 2. * value(0x8035B0E4)?],
                        [80.; 2],
                        [u, 176., u + 48., 224.],
                        0.,
                    );
                } else if age > 0 && kind == 5 {
                    // Exasperation uses a separate atlas cell and a 12-step pop/settle sequence.
                    let ring_size = (2 + age * 10).min(80) as f32;
                    emit(
                        [right, 0., ring_z],
                        [ring_size; 2],
                        [96., 0., 192., 96.],
                        0.,
                    );
                    let row = dol::slice(
                        executable,
                        0x801E37E0 + 0x244 + ((age - 1).min(11) * 2) as u32,
                        2,
                    )?;
                    let size = u16::from_be_bytes(row.try_into()?) as f32;
                    emit(
                        [right, 0., height + value(0x8035B0E4)?],
                        [size; 2],
                        [96., 144., 128., 176.],
                        0.,
                    );
                } else if age > 0 && kind != 14 {
                    let ring_size = (2 + age * 10).min(80) as f32;
                    emit([right, 0., ring_z], [ring_size; 2], [0., 0., 96., 96.], 0.);
                    // Move the balloon from −15 toward 52 at six units per update.
                    let size = (-15 + age as i32 * 6).min(52);
                    if size >= 2 {
                        let frame = if matches!(kind, 6 | 10) {
                            0.
                        } else {
                            ((age / 8) % 3) as f32
                        };
                        let (x, z, u, v) = if kind == 1 {
                            (value(0x8035B120)?, height + value(0x8035B11C)?, 96., 112.)
                        } else if kind == 6 {
                            // The hole interaction uses a static question glyph;
                            // it shares the balloon and glyph growth with surprise.
                            (value(0x8035B120)?, height + value(0x8035B124)?, 64., 144.)
                        } else if kind == 10 {
                            // Once full-sized, the top-anchored glyph drops four units per tick.
                            let drop = (age.saturating_sub(12) as f64 * double(0x8035B138)?)
                                .min(f64::from(value(0x8035B078)?))
                                as f32;
                            (
                                value(0x8035B12C)?,
                                value(0x8035B120)?
                                    + (value(0x8035B11C)? + height)
                                    + value(0x8035B130)?
                                    - drop,
                                0.,
                                144.,
                            )
                        } else {
                            // The sleep glyph is four units ABOVE the balloon
                            // center (paired Dolphin sprite positions 210/214).
                            // The flattened C's reused height temporary loses
                            // the preceding 30-unit ring offset in this case.
                            (right, ring_z + value(0x8035AFD4)?, 128., 144.)
                        };
                        emit(
                            [x, 0., z],
                            [size as f32; 2],
                            [u + frame * 32., v, u + frame * 32. + 32., v + 32.],
                            0.,
                        );
                    }
                } else if age > 0 {
                    // Four rotating strokes form the emphasis emote.
                    let radius = (value(0x8035B078)? + (age - 1) as f32 * value(0x8035B170)?)
                        .min(value(0x8035B058)?);
                    for i in 0..4 {
                        let angle = value(0x8035B168)? + i as f32 * value(0x8035B16C)?;
                        emit(
                            [
                                // The original SDK calls are sin (80129AD4)
                                // for horizontal displacement and cos (8012928C)
                                // for height. Swapping them buries the strokes
                                // behind the character's head.
                                radius * (-angle).to_radians().sin(),
                                0.,
                                radius * angle.to_radians().cos() + height + value(0x8035B054)?
                                    - value(0x8035B074)?,
                            ],
                            [12., 30.],
                            [192., 176., 224., 208.],
                            angle,
                        );
                    }
                }
                match kind {
                    2 => sprites
                        .iter_mut()
                        .for_each(|s| s.vertical_anchor = VerticalAnchor::Bottom),
                    4 | 5 | 6 | 10 => sprites.iter_mut().skip(1).for_each(|s| {
                        s.vertical_anchor = if kind == 10 {
                            VerticalAnchor::Top
                        } else {
                            VerticalAnchor::Bottom
                        };
                    }),
                    11 if age > 0 => {
                        let remaining = 20 - (age - 1) % 20;
                        let alpha = (value(0x8035AFD0)?
                            - value(0x8035B150)? * (value(0x8035B054)? - remaining as f32).max(0.))
                            as u8;
                        sprites.iter_mut().for_each(|s| s.alpha = alpha);
                    }
                    _ => {}
                }
                frames.push(sprites);
            }
        }
        let cycle = frames.split_off(intro_ticks * usize::from(phase_count));
        emotes.insert(
            kind,
            EmoteTrack {
                anchor: anchor.clone(),
                missing_anchor_offset,
                rotation: EmoteRotation::Fixed,
                phase_count,
                intro: frames,
                cycle,
            },
        );
    }
    for kind in [7, 8, 9, 13, 15, 16, 17, 18, 19] {
        let (intro_ticks, cycle_ticks, phase_count) = match kind {
            7 | 8 => (12, 1, 1),
            9 => (33, 1, 1),
            13 => (32, 64, 32),
            _ => (1, 1, 1),
        };
        let mut poses = vec![(0usize, 0f32, 0f32); phase_count];
        let mut frames = Vec::new();
        for age in 0..intro_ticks + cycle_ticks {
            for (phase, (pose, scale, rise)) in poses.iter_mut().enumerate() {
                let mut sprites = Vec::new();
                let mut emit = |offset, size, uv: [f32; 4], alpha, vertical_anchor| {
                    sprites.push(Sprite {
                        offset,
                        size,
                        uv: uv.map(|v| v / 256.),
                        rotation: 0.,
                        vertical_anchor,
                        alpha,
                    });
                };
                if age > 0 {
                    match kind {
                        7 | 8 => {
                            emit(
                                [right, 0., ring_z],
                                [(2 + age * 10).min(80) as f32; 2],
                                [0., 0., 96., 96.],
                                255,
                                VerticalAnchor::Center,
                            );
                            let size = (-15 + age as i32 * 6).min(52);
                            if size >= 2 {
                                let (x, z, v, anchor) = if kind == 7 {
                                    (
                                        value(0x8035B120)?,
                                        value(0x8035B124)?,
                                        144.,
                                        VerticalAnchor::Bottom,
                                    )
                                } else {
                                    (right, value(0x8035B11C)?, 112., VerticalAnchor::Center)
                                };
                                emit(
                                    [x, 0., height + z],
                                    [size as f32; 2],
                                    [32., v, 64., v + 32.],
                                    255,
                                    anchor,
                                );
                            }
                        }
                        9 => emit(
                            [
                                right,
                                0.,
                                height
                                    + ((age - 1) as f32 * value(0x8035B080)?)
                                        .min(value(0x8035B074)?),
                            ],
                            [52.; 2],
                            [0., 112., 32., 144.],
                            ((age - 1) * 8).min(255) as u8,
                            VerticalAnchor::Center,
                        ),
                        13 => {
                            // The random counter advances before testing its 32-tick boundary.
                            if (age + phase) & 31 == 31 {
                                *pose ^= 1;
                                *scale = 0.;
                                *rise = 0.;
                            }
                            *scale = ((f64::from(*scale) + double(0x8035B158)?) as f32).min(1.);
                            *rise = (f64::from(*rise) + double(0x8035B160)?) as f32;
                            for index in 0..4 {
                                let address = 0x801E38B8 + ((*pose * 4 + index) * 16) as u32;
                                let width = u16::from_be_bytes(
                                    dol::slice(executable, address + 12, 2)?.try_into()?,
                                );
                                let size = (*scale * f32::from(width)) as u16;
                                // The sprite renderer skips zero-width entries.
                                if size != 0 {
                                    emit(
                                        [
                                            value(address)?,
                                            value(address + 4)?,
                                            (height + value(address + 8)?) + *rise,
                                        ],
                                        [f32::from(size); 2],
                                        [64., 112., 96., 144.],
                                        (value(0x8035AFD0)? * *scale) as u8,
                                        VerticalAnchor::Center,
                                    );
                                }
                            }
                        }
                        15 => emit(
                            [value(0x8035B054)?, 0., height - value(0x8035B078)?],
                            [30.; 2],
                            [223., 176., 255., 208.],
                            255,
                            VerticalAnchor::Center,
                        ),
                        16..=19 => {} // These admitted kinds only advance their lifetime.
                        _ => unreachable!(),
                    }
                }
                frames.push(sprites);
            }
        }
        let cycle = frames.split_off(intro_ticks * phase_count);
        emotes.insert(
            kind,
            EmoteTrack {
                anchor: anchor.clone(),
                missing_anchor_offset,
                rotation: if kind == 9 {
                    EmoteRotation::GlobalTick {
                        degrees_per_tick: 4,
                    }
                } else {
                    EmoteRotation::Fixed
                },
                phase_count: phase_count as u8,
                intro: frames,
                cycle,
            },
        );
    }
    Ok(emotes)
}

#[test]
#[ignore = "requires both extracted discs; compares new emotes with native table and clock rules"]
fn original_missing_emotes_preserve_pop_fade_and_seeded_cycles() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    for disc in ["disc1", "disc2"] {
        let executable = fs::read(root.join(disc).join("sys/main.dol"))?;
        let tracks = read(&executable)?;
        for track in tracks.values() {
            assert_eq!(track.anchor, "Bone_atama");
            assert_eq!(track.missing_anchor_offset, [0., 0., 128.]);
        }
        assert_eq!(
            tracks.keys().copied().collect::<Vec<_>>(),
            (0..20).collect::<Vec<_>>()
        );
        let float = |address| -> Result<f32> {
            Ok(f32::from_be_bytes(
                dol::slice(&executable, address, 4)?.try_into()?,
            ))
        };
        let height = float(0x8035B118)?;
        for kind in [7, 8] {
            let track = &tracks[&kind];
            assert!(track.frame_with_phase(0, 0).is_empty());
            assert_eq!(track.frame_with_phase(1, 0)[0].size, [12.; 2]);
            assert_eq!(track.frame_with_phase(2, 0).len(), 1);
            let sprite = &track.frame_with_phase(3, 0)[1];
            assert_eq!(sprite.size, [3.; 2]);
            assert_eq!(
                sprite.uv,
                if kind == 7 {
                    [32., 144., 64., 176.]
                } else {
                    [32., 112., 64., 144.]
                }
                .map(|v| v / 256.)
            );
            assert!(matches!(
                (kind, sprite.vertical_anchor),
                (7, VerticalAnchor::Bottom) | (8, VerticalAnchor::Center)
            ));
            assert_eq!(track.frame_with_phase(50, 31)[1].size, [52.; 2]);
        }
        assert!(matches!(
            tracks[&9].rotation,
            EmoteRotation::GlobalTick {
                degrees_per_tick: 4
            }
        ));
        for age in [1, 2, 20, 21, 32, 33, 200] {
            let sprite = &tracks[&9].frame_with_phase(age, 0)[0];
            assert_eq!(sprite.offset[2], height + (2. * (age - 1) as f32).min(40.));
            assert_eq!(sprite.alpha, ((age - 1) * 8).min(255) as u8);
            assert_eq!(sprite.size, [52.; 2]);
        }
        let track = &tracks[&13];
        assert_eq!(track.phase_count, 32);
        // Derive the pose and its elapsed time directly from the seeded boundary,
        // including several cycles beyond the baked track's stored interval.
        for phase in 0..32u8 {
            assert!(track.frame_with_phase(0, phase).is_empty());
            let first_boundary = match phase {
                31 => 32,
                _ => 31 - usize::from(phase),
            };
            for age in 1..225 {
                let (pose, elapsed) = if age < first_boundary {
                    (0, age)
                } else {
                    (
                        (1 + (age - first_boundary) / 32) % 2,
                        (age - first_boundary) % 32 + 1,
                    )
                };
                let mut scale = 0f32;
                let mut rise = 0f32;
                for _ in 0..elapsed {
                    scale = ((f64::from(scale) + 0.08) as f32).min(1.);
                    rise = (f64::from(rise) + 0.22) as f32;
                }
                let mut expected = Vec::new();
                for index in 0..4 {
                    let address = 0x801E38B8 + (pose * 64 + index * 16) as u32;
                    let width =
                        u16::from_be_bytes(dol::slice(&executable, address + 12, 2)?.try_into()?);
                    let size = (f32::from(width) * scale) as u16;
                    if size != 0 {
                        expected.push((
                            [
                                float(address)?,
                                float(address + 4)?,
                                (height + float(address + 8)?) + rise,
                            ],
                            [f32::from(size); 2],
                            (255. * scale) as u8,
                        ));
                    }
                }
                let actual = track.frame_with_phase(age, phase);
                assert_eq!(actual.len(), expected.len(), "{disc} phase{phase} age{age}");
                for (sprite, (position, size, alpha)) in actual.iter().zip(expected) {
                    assert_eq!(
                        (sprite.offset, sprite.size, sprite.alpha),
                        (position, size, alpha),
                        "{disc} phase{phase} age{age}"
                    );
                    assert_eq!(sprite.uv, [64., 112., 96., 144.].map(|v| v / 256.));
                }
            }
        }
        let mark = &tracks[&15].frame_with_phase(1, 0)[0];
        assert_eq!(mark.size, [30.; 2]);
        assert_eq!(
            mark.offset,
            [float(0x8035B054)?, 0., height - float(0x8035B078)?]
        );
        assert_eq!(mark.uv, [223., 176., 255., 208.].map(|v| v / 256.));
        for kind in 16..=19 {
            for age in [0, 1, 100] {
                assert!(tracks[&kind].frame_with_phase(age, 0).is_empty());
            }
        }
    }
    Ok(())
}
