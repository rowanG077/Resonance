use super::*;

fn check_ring_reserved_word(cooker: &mut Cooker, row: &[u8], id: EffectId, bank: &[u8]) {
    let actor = cooker.actor(row, id, bank, false).unwrap();
    assert!(matches!(actor.geometry, Geometry::BillboardRing { .. }));
    let expected = serde_json::to_value(&actor).unwrap();
    let mut changed = row.to_vec();
    for bits in [0, 1f32.to_bits(), (-1f32).to_bits(), f32::NAN.to_bits()] {
        changed[0x84..0x88].copy_from_slice(&bits.to_be_bytes());
        assert_eq!(
            serde_json::to_value(cooker.actor(&changed, id, bank, false).unwrap()).unwrap(),
            expected
        );
    }
    for offset in [0x7c, 0x80] {
        changed.copy_from_slice(row);
        changed[offset..offset + 4].copy_from_slice(&1f32.to_be_bytes());
        assert!(
            cooker
                .actor(&changed, id, bank, false)
                .unwrap_err()
                .to_string()
                .contains("unsupported effect actor motion state")
        );
    }
    // No active ring program writes this otherwise unsupported modifier field.
    assert!(
        actor
            .validate_modifiers(&[Modifier::Float {
                field: FloatField::GeometryAngleStep,
                operation: Arithmetic::Set,
                value: FloatValue::Constant(1.),
            }])
            .is_err()
    );
}

#[test]
fn billboard_ring_ignores_only_its_reserved_motion_word() {
    let mut row = [0; ACTOR_BYTES];
    row[0] = 7;
    row[2] = 255;
    row[0x12] = 48;
    row[0x32] = 255;
    let id = EffectId {
        bank: EffectBank::Common,
        id: 0,
    };
    let mut cooker = test_cooker();
    check_ring_reserved_word(&mut cooker, &row, id, &[]);
    row[0x84..0x88].copy_from_slice(&1f32.to_be_bytes());
    for kind in [5, 6] {
        row[0] = kind;
        assert!(cooker.actor(&row, id, &[], false).is_err());
    }
}

#[test]
#[ignore = "requires the original extracted disc; parses effects without encoding textures"]
fn original_eruption_closes_all_pulses_models_scrolls_materials_and_shakes() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    assert_eq!(rel.pointer(5, 0x5bc8 + 7 * 4).unwrap(), (1, 0x792cc));
    // The complete ring draw, common update and initializer never read recipe +0x84.
    for (offset, size, digest) in [
        (
            0x792cc,
            0x508,
            "03d5de039abb4ecc54b0d21fc6d20636df0af1b0f6a72db9b22a72133e029292",
        ),
        (
            0x403f4,
            0xa4c,
            "9369dfac3f467163c41793b3a52da6e3138d2ae564700f15ff11f98ada3cd9e1",
        ),
        (
            0x40e40,
            0x2b8,
            "fca41e56e3561d05a371f5f2ee1529803d7f8b64668f75e774a4c88f78fe6440",
        ),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(&rel.at((1, offset)).unwrap()[..size])
            ),
            digest
        );
    }
    let archive = MagicArchive::read(&extracted).unwrap();
    let package = archive.package(5).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(package)),
        "99b44abc8da8cc3072cbdd1c36ea699f9accb804302624708ddbc983d1f914e1"
    );
    let bytes = magic_member(package, 4).unwrap().unwrap();
    let bank = EffectBank::Magic(5);
    let textures = tpl::parse_tpl(magic_member(package, 8).unwrap().unwrap()).unwrap();
    assert_eq!(textures.len(), 2);
    assert!(
        textures
            .iter()
            .all(|t| (t.width, t.height, t.format, t.palette_entries) == (256, 256, 9, 512))
    );
    let mut cooker = test_cooker();
    cooker.result.materials.push(EffectMaterial {
        texture: UiTexture {
            path: "test.ktx2".into(),
            width: 256,
            height: 256,
        },
        rgb_scale: 2.,
    });
    for id in [2, 3, 6, 7] {
        let row = actor_source(bytes, EffectId { bank, id }).unwrap();
        let stride = if row[6] == 0 {
            256
        } else {
            usize::from(row[6])
        };
        for image in 0..2 {
            assert!(usize::from(row[3 + image]) * stride < textures[image].palette_entries);
        }
        cooker.material_indices.insert(
            MaterialKey {
                texture: TextureBank::Magic(5),
                color: row[3],
                alpha: Some(row[4]),
                stride: row[6],
            },
            0,
        );
        if id >= 6 {
            assert_eq!(float(row, 0x84).unwrap(), if id == 6 { 1. } else { 0. });
            check_ring_reserved_word(&mut cooker, row, EffectId { bank, id }, bytes);
        }
    }
    cooker.program(bytes, EffectId { bank, id: 1 }).unwrap();
    cooker.result.validate().unwrap();
    assert_eq!(cooker.result.programs.len(), 1);
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|a| a.id.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 1, 2, 3, 6, 7])
    );
    assert_eq!(
        cooker.result.models().collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 1].map(|index| ModelRef::Magic { package: 5, index }))
    );
    for field in [12, 16] {
        assert!(!magic_member(package, field).unwrap().unwrap().is_empty());
    }
    let program = cooker.result.program(EffectId { bank, id: 1 }).unwrap();
    for emission in &program.emissions {
        if let EffectCommand::Particle {
            actor, modifiers, ..
        } = &emission.command
            && matches!(actor.id, 6 | 7)
        {
            assert!(!modifiers.iter().any(|modifier| matches!(
                modifier,
                Modifier::Float {
                    field: FloatField::GeometryAngleStep,
                    ..
                } | Modifier::RandomFloat {
                    field: FloatField::GeometryAngleStep,
                    ..
                }
            )));
        }
    }
    assert_eq!(program.end_tick, 165);
    assert_eq!(
        program.emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
        [
            0, 0, 15, 15, 30, 30, 30, 30, 30, 35, 60, 60, 60, 60, 65, 90, 90, 90, 90, 95
        ]
    );
    let shakes = program
        .emissions
        .iter()
        .filter_map(|emission| {
            if let EffectCommand::Controller { actor, controller } = &emission.command {
                assert_eq!(*actor, EffectId { bank, id: 4 });
                assert!(matches!(
                    controller,
                    EffectController::Shake {
                        duration: 12,
                        amplitude: 8
                    }
                ));
                Some(emission.tick)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(shakes, [30, 60, 90]);
    let repeats = program
        .emissions
        .iter()
        .filter_map(|e| e.repeat.map(|r| (e.tick, r.count, r.interval)))
        .collect::<Vec<_>>();
    assert_eq!(
        repeats,
        [
            (15, 5, 30),
            (15, 10, 12),
            (30, 3, 30),
            (30, 4, 1),
            (30, 4, 1),
            (60, 4, 1),
            (60, 4, 1),
            (90, 4, 1),
            (90, 4, 1)
        ]
    );
    let sounds = program
        .emissions
        .iter()
        .filter_map(|e| {
            if let EffectCommand::Sound { sound, priority } = e.command {
                assert_eq!(priority, 0);
                Some((e.tick, sound))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(sounds, [(15, 82), (30, 124)]);
    for (id, origin, step) in [(2, [1, 1], [0, 4]), (3, [65, 1], [0, 6])] {
        let actor = cooker.result.actor(EffectId { bank, id }).unwrap();
        assert!(matches!(actor.geometry, Geometry::Hemisphere { .. }));
        let uv = actor.uv_animation.as_ref().unwrap();
        assert_eq!(uv.frames[0].duration, 1);
        assert!(
            matches!(uv.frames[0].update, UvUpdate::Scroll { origin: o, step: s }
            if o == origin && s == step)
        );
    }
    let model = cooker.result.actor(EffectId { bank, id: 1 }).unwrap();
    assert!(matches!(
        model.uv_animation.as_ref().unwrap().model_scroll,
        Some(ModelUvScroll {
            step: [1, -15],
            period: [2500, 2500]
        })
    ));
    let embers = cooker.result.actor(EffectId { bank, id: 7 }).unwrap();
    assert!(matches!(
        embers.ground,
        Some(GroundResponse::Bounce { effect: None })
    ));
    assert!(embers.depth_write);
    // Native205 creates only projectiles2/3, neither of which references another program.
    let projectiles = magic_member(package, 252).unwrap().unwrap();
    for id in [2, 3] {
        let row = &projectiles[id * 400..(id + 1) * 400];
        assert_eq!([row[0xf], row[0x5d], row[0x5f]], [0; 3]);
    }
    // Program2 and actor5/model2 belong to the unrequested projectile1 recipe.
    assert_eq!(
        &program_timeline(bytes, 2).unwrap()[..6],
        &[0, 0, 5, 0, 0, 0]
    );
    let unused = actor_source(bytes, EffectId { bank, id: 5 }).unwrap();
    assert_eq!((unused[0], unused[0xd4]), (3, 2));
    assert!(cooker.result.actor(EffectId { bank, id: 5 }).is_none());
}
