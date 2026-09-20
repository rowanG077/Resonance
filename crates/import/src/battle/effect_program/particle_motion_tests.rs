use super::*;

#[test]
fn mathematical_modifiers_keep_axes_units_selectors_and_flag_masks() {
    let stream = |opcode: u16, destination: u16, operands: &[u16]| {
        let mut bytes = vec![0; 8];
        bytes.extend(
            [opcode, destination]
                .into_iter()
                .chain(operands.iter().copied())
                .chain([u16::MAX])
                .flat_map(u16::to_be_bytes),
        );
        bytes
    };
    for (index, axis) in [VectorAxis::X, VectorAxis::Y, VectorAxis::Z]
        .into_iter()
        .enumerate()
    {
        let bytes = stream(16, 0x40, &[index as u16, (-900i16) as u16]);
        assert!(
            matches!(modifiers(&bytes, 8).unwrap()[..], [Modifier::RotateVector {
            field: VectorField::Velocity, axis: actual, angle: FloatValue::Constant(-90.),
        }] if actual == axis)
        );
        let bytes = stream(28 + index as u16, 0x4c, &[0x3fc0, 0, 0x1234, 0]);
        assert!(
            matches!(modifiers(&bytes, 8).unwrap()[..], [Modifier::SetEmitterAxisVector {
            field: VectorField::Acceleration, axis: actual, value: FloatValue::Constant(1.5),
        }] if actual == axis)
        );
    }
    let bytes = stream(16, 0x7ff8, &[1, 0x7ffb]);
    assert!(matches!(
        modifiers(&bytes, 8).unwrap()[..],
        [Modifier::RotateVector {
            field: VectorField::Temporary(0),
            axis: VectorAxis::Y,
            angle: FloatValue::Temporary(3),
        }]
    ));
    let bytes = stream(17, 0x34, &[(-25i16) as u16, 900]);
    assert!(matches!(
        modifiers(&bytes, 8).unwrap()[..],
        [Modifier::PolarVector {
            field: VectorField::Position,
            radius: FloatValue::Constant(-2.5),
            angle: FloatValue::Constant(90.),
        }]
    ));
    let bytes = stream(17, 0x7ff9, &[0x7ff8, 0x7ffb]);
    assert!(matches!(
        modifiers(&bytes, 8).unwrap()[..],
        [Modifier::PolarVector {
            field: VectorField::Temporary(1),
            radius: FloatValue::Temporary(0),
            angle: FloatValue::Temporary(3),
        }]
    ));
    for (opcode, enabled) in [(21, true), (19, false)] {
        let bytes = stream(opcode, u16::MAX, &[0x10c8, 0x142a]);
        let parsed = modifiers(&bytes, 8).unwrap();
        for (modifier, expected) in parsed.iter().zip([
            EffectFlag::BottomAnchored,
            EffectFlag::ColorGradient,
            EffectFlag::RibbonDepth,
            EffectFlag::FollowEmitter,
            EffectFlag::GroundRelative,
            EffectFlag::DepthTest,
            EffectFlag::UseElementVariant,
            EffectFlag::DepthWrite,
            EffectFlag::CullBack,
        ]) {
            assert!(matches!(modifier, Modifier::Flag { field, enabled: actual }
                if *field == expected && *actual == (enabled ^ (expected == EffectFlag::DepthTest))));
        }
        assert_eq!(parsed.len(), 9);
        assert!(
            modifiers(&stream(opcode, 0x7fff, &[0, 0]), 8)
                .unwrap()
                .is_empty()
        );
        for flag in [0x100u32, 0x40000, 0x40000000] {
            assert!(
                modifiers(
                    &stream(opcode, 0x14, &[(flag >> 16) as u16, flag as u16]),
                    8
                )
                .is_err()
            );
        }
    }
    for (opcode, destination, operands) in [
        (16, 0x40, [3, 900]),
        (16, 0x40, [u16::MAX, 900]),
        (16, 0x7ffa, [0, 900]),
        (16, 0x40, [0, 0x7ffc]),
        (17, 0x34, [0x7fff, 900]),
        (17, 0x34, [10, 0x7ffc]),
    ] {
        assert!(modifiers(&stream(opcode, destination, &operands), 8).is_err());
    }
    let bytes = stream(16, 0x40, &[0, 900]);
    assert!(modifiers(&bytes[..bytes.len() - 3], 8).is_err());
}

#[test]
#[ignore = "requires original extracted GameCube records; no texture encoding"]
fn original_summon_strikes_close_clamped_particles_and_motion_facing_models() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    assert_eq!(&rel.at((4, 0x1fbc)).unwrap()[..3], b"XYZ");
    for (offset, expected) in [(0x1fc4, 0.017453292), (0x1fc8, 0.1), (0x1fd0, 0.017453289)] {
        assert_eq!(float(rel.at((4, offset)).unwrap(), 0).unwrap(), expected);
    }
    for (offset, expected) in [
        (0x2108, 0.1),
        (0x210c, 180.),
        (0x2110, 57.29579),
        (0x2114, -1.),
    ] {
        assert_eq!(float(rel.at((4, offset)).unwrap(), 0).unwrap(), expected);
    }
    for (offset, instruction) in [
        (0x407dc, 0x54000109),
        (0x407fc, 0x4080000c),
        (0x40804, 0xd01f0038),
        (0x4080c, 0x54600631),
        (0x40814, 0x5460056b),
        (0x40820, 0x389d0260),
        (0x40870, 0xec0100ba),
        (0x40874, 0xd01f0058),
        (0x40894, 0xd01f005c),
        (0x41fe8, 0x2c000003),
        (0x41fec, 0x418200b4),
        (0x40020, 0x540000c7),
        (0x40028, 0x38600002),
    ] {
        assert_eq!(word(rel.at((1, offset)).unwrap(), 0).unwrap(), instruction);
    }
    for (package, actor_id, flags) in [(90, 18, 0x0c000042), (92, 11, 0x10000480)] {
        let source = archive.package(package).unwrap();
        let bank = magic_member(source, 4).unwrap().unwrap();
        let id = EffectId {
            bank: EffectBank::Magic(package),
            id: actor_id,
        };
        let row = actor_source(bank, id).unwrap();
        assert_eq!(word(row, 0x14).unwrap(), flags);
        let mut cooker = test_cooker();
        cooker.textures.insert(
            TextureBank::Magic(package),
            magic_member(source, 8).unwrap().unwrap().to_vec(),
        );
        cooker.program(bank, EffectId { id: 2, ..id }).unwrap();
        cooker.result.validate().unwrap();
        let actor = cooker.result.actor(id).unwrap();
        if package == 90 {
            assert_eq!(actor.geometry, Geometry::Quad);
            assert!(matches!(actor.ground, Some(GroundResponse::Clamp)));
            assert_eq!(actor.position, [0., 75., 0.]);
            assert_eq!(actor.dimensions, [900., 192., 0.]);
            assert!(actor.bottom_anchored && !actor.follow_emitter);
            assert_eq!(actor.lifetime, Some(20));
            // A secondary impact records the child before the parent is clamped.
            let mut secondary = row.to_vec();
            secondary[0x91] = 4;
            assert!(matches!(
                cooker.actor(&secondary, id, bank, false).unwrap().ground,
                Some(GroundResponse::EmitOnce { clamp: true, .. })
            ));
        } else {
            assert!(matches!(
                actor.geometry,
                Geometry::Model {
                    model: ModelRef::Magic {
                        package: 92,
                        index: 2
                    },
                    presentation: ModelPresentation {
                        orientation: ModelOrientation::FollowMotion,
                        ..
                    },
                    ..
                }
            ));
            assert!(actor.follow_emitter && actor.cull_back);
            assert_eq!(actor.lifetime, None);
            assert_eq!(actor.dimensions, [2.5; 3]);
            // Exercise the recovered release command on this original model recipe.
            let mut secondary = row.to_vec();
            secondary[0x91] |= 1;
            secondary[0x94] = 255;
            assert!(matches!(
                cooker.actor(&secondary, id, bank, false).unwrap().geometry,
                Geometry::Model {
                    presentation: ModelPresentation {
                        reverse_at: Some(255),
                        ..
                    },
                    ..
                }
            ));
            let mut unsupported = row.to_vec();
            unsupported[0] = 5;
            assert!(cooker.actor(&unsupported, id, bank, false).is_err());
            unsupported[0] = 3;
            unsupported[0x16] &= !4;
            assert!(cooker.actor(&unsupported, id, bank, false).is_err());
        }
    }
    for (package, hash, end, backdrop_end, actors, models, clips) in [
        (
            84,
            "75ed2a9e32562a7a71f3ebb4d0a3bb4e17e1dc09d867c46400aa066935e34acc",
            270,
            100,
            19,
            &[0, 1, 2, 4][..],
            &[(0, 0, 0, false), (0, 1, 4, true), (0, 0, 15, false)][..],
        ),
        (
            87,
            "c7a1a2bf05b178102b106191a9a2ea7b16a400c581f53d228bf68fa585f96047",
            210,
            100,
            18,
            &[0, 1][..],
            &[(0, 1, 0, true), (0, 2, 2, true), (0, 0, 4, false)][..],
        ),
        (
            85,
            "6a1e2a480fac0de97b964dd17a4a74b46d82fe3e04c9eab1e9264ca071cbddf0",
            270,
            100,
            15,
            &[0, 1, 2, 3, 4][..],
            &[(0, 1, 0, false), (0, 2, 4, true)][..],
        ),
        (
            93,
            "32d1ccb99f3259efd3f6d4ca809109f49ba06a11a4f08cb35b40a3bdafc81689",
            270,
            120,
            29,
            &[0, 1, 2, 3][..],
            &[(0, 0, 0, false), (0, 1, 0, true)][..],
        ),
        (
            89,
            "ef44efaafb20c6b5a5793fa8d15432a1844622ac00860d9301427da7af14c659",
            250,
            150,
            18,
            &[0, 1, 2, 3][..],
            &[(0, 0, 0, false), (1, 0, 0, false)][..],
        ),
        (
            91,
            "0a2e9d323c8a81f8b4032db973545f0ef05edfa0e86e367a48c460419adefd58",
            270,
            150,
            13,
            &[0, 1][..],
            &[(0, 0, 0, false), (0, 1, 15, true)][..],
        ),
        (
            86,
            "a1bf0a6c696064bf429d9cab154460baeaabdebbc9d80de20a9798e42677ab72",
            295,
            150,
            31,
            &[0, 1, 2, 3, 4, 5, 6][..],
            &[(0, 0, 0, true), (1, 0, 0, true), (2, 0, 0, true)][..],
        ),
        (
            88,
            "3d04b530f2aac574d34935547c4b60103f78000222c078d6bcb02b5ea35ad513",
            250,
            64,
            13,
            &[0, 1, 2, 3, 4, 5, 6, 7][..],
            &[(0, 0, 0, false), (0, 1, 4, true), (0, 2, 4, true)][..],
        ),
    ] {
        let source = archive.package(package).unwrap();
        let bank = magic_member(source, 4).unwrap().unwrap();
        assert_eq!(crate::digest(bank), hash);
        let effect = |id| EffectId {
            bank: EffectBank::Magic(package),
            id,
        };
        let mut cooker = test_cooker();
        cooker.textures.insert(
            TextureBank::Magic(package),
            magic_member(source, 8).unwrap().unwrap().to_vec(),
        );
        for id in [1, 2] {
            cooker.program(bank, effect(id)).unwrap();
        }
        if package == 86 {
            cooker.program(bank, effect(3)).unwrap();
        }
        cooker.result.validate().unwrap();
        assert_eq!(cooker.result.program(effect(1)).unwrap().end_tick, end);
        assert_eq!(
            cooker.result.program(effect(2)).unwrap().end_tick,
            backdrop_end
        );
        assert_eq!(cooker.result.actors.len(), actors);
        assert_eq!(
            cooker
                .result
                .models()
                .collect::<std::collections::BTreeSet<_>>(),
            models
                .iter()
                .map(|&index| ModelRef::Magic { package, index })
                .collect()
        );
        let actor = cooker
            .result
            .actor(effect(if package == 88 { 0 } else { 4 }))
            .unwrap();
        assert_eq!(actor.retained, package != 89);
        assert!(matches!(
            actor.geometry,
            Geometry::Model {
                model: ModelRef::Magic { index: 0, .. },
                presentation: ModelPresentation {
                    external_animation: true,
                    ..
                },
                ..
            }
        ));
        if package == 87 {
            assert!(matches!(actor.ground, Some(GroundResponse::Clamp)));
        }
        if package == 93 {
            for (id, joint) in [(7, 2), (8, 3)] {
                assert!(
                    matches!(cooker.result.actor(effect(id)).unwrap().geometry, Geometry::Model {
                    model: ModelRef::Magic { package: 93, index: 3 },
                    presentation: ModelPresentation { attachment: Some(RetainedJoint {slot: 0, joint: actual}), .. }, ..
                } if actual == joint)
                );
            }
            // Both attached blades read real joints of the retained animated parent.
            let model = magic_member(source, 12).unwrap().unwrap();
            let skeleton = crate::battle::pose::skeleton(model).unwrap();
            assert!(skeleton.bones.len() > 3);
        }
        if package == 86 {
            let adjacent = [0, 140, 6, 0, 0, 0, 0, 140, 253, 1, 0x2a, 0xd4];
            let with_child = [
                0, 215, 7, 0, 0, 0, 0, 215, 8, 0, 0, 0, 0, 215, 253, 2, 0x2b, 0x3c,
            ];
            let at = |fragment: &[u8]| {
                bank.windows(fragment.len())
                    .position(|row| row == fragment)
                    .unwrap()
            };
            let (first, second) = (at(&adjacent), at(&with_child));
            let program = cooker.result.program(effect(1)).unwrap();
            for (id, tick, model) in [(6, 140, 1), (7, 215, 2)] {
                assert!(program.emissions.iter().any(|emission| emission.tick == tick
                    && matches!(&emission.command, EffectCommand::Particle {actor, modifiers, ..}
                        if actor.id == id && matches!(modifiers.as_slice(), [Modifier::PlayModelAnimation {animation}]
                            if animation.model == model && animation.clip == 0 && animation.hold))));
            }
            // Only the two same-tick bindings move. Later motion changes keep their slots.
            assert_eq!(
                program
                    .emissions
                    .iter()
                    .filter_map(|e| match e.command {
                        EffectCommand::ModifyRetained { slot, .. } => Some((e.tick, slot)),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                [(103, 0), (235, 2), (275, 2)]
            );
            for (offset, value) in [(first + 7, 141), (first + 9, 0), (second + 9, 1)] {
                let mut source = bank.to_vec();
                source[offset] = value;
                let mut rejected = test_cooker();
                // Reuse recovered assets: these mutations exercise only command binding.
                rejected.result = cooker.result.clone();
                rejected.result.programs.clear();
                rejected.program(&source, effect(1)).unwrap();
                assert!(
                    rejected.result.validate().is_err(),
                    "unproven delayed/slot/child binding was accepted"
                );
            }
            for (id, slot) in [(5, 0), (8, 2)] {
                assert!(matches!(
                    cooker.result.actor(effect(id)).unwrap().geometry,
                    Geometry::Model { presentation: ModelPresentation {
                        attachment: Some(RetainedJoint { slot: actual, joint: 0 }), ..
                    }, .. } if actual == slot
                ));
            }
            let trail = cooker.result.actor(effect(30)).unwrap();
            assert!(trail.follow_emitter && trail.lifetime.is_none());
            assert!(matches!(
                trail.geometry,
                Geometry::Model {
                    model: ModelRef::Magic {
                        package: 86,
                        index: 5
                    },
                    presentation: ModelPresentation {
                        orientation: ModelOrientation::FollowMotion,
                        ..
                    },
                    ..
                }
            ));
            assert_eq!(cooker.result.program(effect(3)).unwrap().end_tick, 0);
            assert!(matches!(
                &modifiers(bank, 0x2b50).unwrap()[..],
                [
                    Modifier::SetEmitterAxisVector {
                        axis: VectorAxis::Z,
                        field: VectorField::Velocity,
                        value: FloatValue::Constant(11.)
                    },
                    Modifier::SetEmitterAxisVector {
                        axis: VectorAxis::Z,
                        field: VectorField::Acceleration,
                        value: FloatValue::Constant(-0.25)
                    },
                ]
            ));
            for (offset, replacement) in [(0x2b52, 0x34u16), (0x2b58, 0x7ffc)] {
                let mut unsupported = bank.to_vec();
                unsupported[offset..offset + 2].copy_from_slice(&replacement.to_be_bytes());
                assert!(modifiers(&unsupported, 0x2b50).is_err());
            }
            for (offset, bytes, hash) in [
                (
                    0x3f0c0,
                    2756,
                    "b7e718292f08832c3e5c713b1a0229100abba9508a5f0d9633dc43dd40067bcd",
                ),
                (
                    0x413b8,
                    332,
                    "c0c1074ab7ca98a6fa1db517bfdcf13750fb550ee6433cc29ad511b5c2843bb2",
                ),
                (
                    0x418b4,
                    0x810,
                    "fada0d5d1b343b1b6216356950ccca82f86a48c25fd859f7a52ac966bea3d661",
                ),
            ] {
                assert_eq!(crate::digest(&rel.at((1, offset)).unwrap()[..bytes]), hash);
            }
        }
        let animations = cooker
            .result
            .programs
            .iter()
            .flat_map(|program| &program.emissions)
            .filter_map(|emission| match &emission.command {
                EffectCommand::Particle { modifiers, .. }
                | EffectCommand::ModifyRetained { modifiers, .. } => Some(modifiers),
                _ => None,
            })
            .flatten()
            .filter_map(|modifier| match modifier {
                Modifier::PlayModelAnimation { animation } => Some(animation),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(animations.len(), clips.len());
        for (animation, &(model, clip, blend, hold)) in animations.iter().zip(clips) {
            assert_eq!(
                (
                    animation.model,
                    animation.clip,
                    animation.blend_ticks,
                    animation.hold
                ),
                (model, clip, blend, hold)
            );
            let rate = if package == 87 && clip == 2 { 0.4 } else { 0.5 };
            assert!((animation.rate - rate).abs() < 1e-6);
        }
        // Covers every model layer, outline palette, controlled rig and requested clip.
        crate::battle::visual::preflight_effect_models(&extracted, &cooker.result).unwrap();
    }
    // Undine requests the same recipient-owned healing program as First Aid: source bank1.
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let textures = member(&usual, 4).unwrap();
    let mut cooker = test_cooker();
    cooker.textures.insert(
        TextureBank::Fixed(0),
        compression::decode(member(textures, 1).unwrap()).unwrap(),
    );
    cooker
        .textures
        .insert(TextureBank::Fixed(1), member(textures, 4).unwrap().to_vec());
    cooker
        .program(
            member(&usual, 3).unwrap(),
            EffectId {
                bank: EffectBank::Techniques,
                id: 20,
            },
        )
        .unwrap();
    cooker.result.validate().unwrap();
}

#[test]
#[ignore = "requires original extracted GameCube records; no texture encoding"]
fn original_drake_ring_uses_live_owner_bone_space_without_admitting_other_flag_consumers() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    // Ring local translation, live bone-matrix concatenation, heading omission, and root fallback.
    for (offset, instruction) in [
        (0x7a610, 0x54800043),
        (0x7a618, 0x80770034),
        (0x7a6f4, 0x4bfc8c4d),
        (0x7a70c, 0x88b7008c),
        (0x7a714, 0x4bfe1271),
        (0x7a718, 0x388100a0),
        (0x7a71c, 0x38610070),
        (0x7a720, 0x7c852378),
        (0x43488, 0x54000043),
        (0x43494, 0xc01d005c),
        (0x434b0, 0x48000028),
        (0x434bc, 0xc01d0158),
        (0x41038, 0x2c000003),
        (0x4103c, 0x4082000c),
        (0x5b9ac, 0x7c070000),
        (0x5b9b0, 0x41800008),
        (0x5b9b4, 0x38a00000),
        (0x5b9f0, 0x807f0084),
    ] {
        assert_eq!(word(rel.at((1, offset)).unwrap(), 0).unwrap(), instruction);
    }
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let directory = member(&usual, 10).unwrap();
    let start = word(directory, 172 * 4).unwrap() as usize;
    let end = word(directory, 173 * 4).unwrap() as usize;
    let enemy = compression::decode(&archive[start..end]).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&enemy)),
        "9cd7afae93ac759947b87a8e639f77dc98a85ff39be027f5c76fe82d288ba6d8"
    );
    let (start, end) = (
        word(&enemy, 0x1cc).unwrap() as usize,
        word(&enemy, 0x1d0).unwrap() as usize,
    );
    assert_eq!((start, end), (428640, 433920));
    let bank = &enemy[start..end];
    let id = EffectId {
        bank: EffectBank::Enemy(172),
        id: 10,
    };
    let row = actor_source(bank, id).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "9ac8fa05eac9079a41172bdb7ea796c78c35b35b4e1cd931990214c5745f680d"
    );
    assert_eq!(word(row, 0x14).unwrap(), 0x44000010);
    assert_eq!(&bank[4394..4406], &[0, 0, 10, 0, 0, 0, 0, 0, 254, 0, 0, 0]);
    let mut cooker = test_cooker();
    cooker
        .textures
        .insert(TextureBank::Enemy(172), enemy[end..].to_vec());
    cooker.program(bank, EffectId { id: 4, ..id }).unwrap();
    cooker.result.validate().unwrap();
    let actor = cooker.result.actor(id).unwrap();
    assert_eq!(actor.space, EffectSpace::OwnerBone { bone: 13 });
    assert_eq!(actor.position, [0., 0., -50.]);
    assert_eq!(actor.velocity, [0., 0., 1.]);
    assert_eq!(actor.angles, [0., -90., 0.]);
    assert_eq!(actor.dimensions, [15., 40., 60.]);
    assert_eq!(actor.dimension_velocity, [-1., -2., -4.]);
    assert_eq!(actor.lifetime, Some(15));
    assert!(actor.material.is_some() && actor.uv_animation.is_some());
    for (offset, value) in [
        (0, 3),
        (0, 5),
        (0x17, 0x50),
        (0x14, 0x64),
        (0x14, 0xc4),
        (0x8d, 1),
    ] {
        let mut unsupported = row.to_vec();
        unsupported[offset] = value;
        assert!(cooker.actor(&unsupported, id, bank, false).is_err());
    }
}

#[test]
#[ignore = "requires original extracted GameCube records; no texture encoding"]
fn original_wyvern_billboard_ignores_ring_binding_flag() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    assert_eq!(rel.pointer(5, 0x5bc8 + 5 * 4).unwrap(), (1, 0x7a100));
    for (offset, instruction) in [
        (0x7a1b4, 0x387f0148),
        (0x7a1b8, 0x389f0034),
        (0x7a228, 0x540005ad),
        (0x7a22c, 0x41820048),
        (0x7a294, 0x54600673),
        (0x7a298, 0x41820030),
        (0x7a2c0, 0x4bfc8ff9),
        (0x7a2c4, 0x48000060),
        (0x432e0, 0xc0030060),
        (0x43308, 0x386355dc),
        (0x41038, 0x2c000003),
        (0x4103c, 0x4082000c),
    ] {
        assert_eq!(word(rel.at((1, offset)).unwrap(), 0).unwrap(), instruction);
    }
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let directory = member(&usual, 10).unwrap();
    let start = word(directory, 171 * 4).unwrap() as usize;
    let end = word(directory, 172 * 4).unwrap() as usize;
    assert_eq!(
        format!("{:x}", Sha256::digest(&archive[start..end])),
        "c6e6cd0e33a38d69b0aaf14b36e9de38d0bb794f982929a05d749958e4f82a26"
    );
    let enemy = compression::decode(&archive[start..end]).unwrap();
    let (start, end) = (
        word(&enemy, 0x1cc).unwrap() as usize,
        word(&enemy, 0x1d0).unwrap() as usize,
    );
    assert_eq!(start, 428480);
    let bank = &enemy[start..end];
    let id = EffectId {
        bank: EffectBank::Enemy(171),
        id: 11,
    };
    let row = actor_source(bank, id).unwrap();
    assert_eq!(row[0], 5);
    assert_eq!(word(row, 0x14).unwrap(), 0x44000040);
    assert_eq!(&bank[4382..4394], &[0, 0, 11, 0, 0, 0, 0, 0, 254, 0, 0, 0]);
    let mut cooker = test_cooker();
    cooker
        .textures
        .insert(TextureBank::Enemy(171), enemy[end..].to_vec());
    cooker.program(bank, EffectId { id: 3, ..id }).unwrap();
    cooker.result.validate().unwrap();
    let actor = cooker.result.actor(id).unwrap().clone();
    assert_eq!(actor.geometry, Geometry::Quad);
    assert_eq!(actor.orientation, Orientation::Billboard);
    assert_eq!(actor.space, EffectSpace::World);
    assert!(actor.material.is_some() && actor.uv_animation.is_some());
    // Neither the flag nor the dormant bone-index bytes affect the billboard draw path.
    let mut plain = row.to_vec();
    plain[0x14] &= !0x40;
    plain[0x8c..0x8e].fill(0);
    let plain = cooker.actor(&plain, id, bank, false).unwrap();
    assert_eq!(
        serde_json::to_value(actor).unwrap(),
        serde_json::to_value(plain).unwrap()
    );
    let mut unsupported = row.to_vec();
    unsupported[0x17] &= !0x40; // World quads reach a different, unimplemented flag consumer.
    assert!(cooker.actor(&unsupported, id, bank, false).is_err());
    unsupported[0] = 6;
    unsupported[0x17] |= 0x40;
    assert!(cooker.actor(&unsupported, id, bank, false).is_err());
}
