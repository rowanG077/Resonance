//! Full original Pow Devastation/Pow Spear effect records, without texture encoding.
use super::*;

#[test]
fn pow_devastation_ring_preserves_draw_fields_and_bounds_its_unused_marker() {
    let mut row = [0u8; ACTOR_BYTES];
    for (offset, value) in [
        (0x0, 4),
        (0x2, 6),
        (0x3, 3),
        (0x4, 1),
        (0x6, 32),
        (0x9, 97),
        (0xb, 17),
        (0xd, 62),
        (0xf, 46),
        (0x11, 10),
        (0x14, 4),
        (0x17, 2),
        (0x19, 128),
        (0x1b, 128),
        (0x1d, 128),
        (0x1f, 192),
        (0x2f, 16),
        (0x32, 255),
        (0x3c, 65),
        (0x3d, 128),
        (0x58, 194),
        (0x59, 180),
        (0x68, 61),
        (0x69, 204),
        (0x6a, 204),
        (0x6b, 205),
        (0x90, 1),
        (0xb0, 66),
        (0xb1, 160),
        (0xb4, 66),
        (0xb8, 67),
        (0xbc, 64),
        (0xc0, 64),
        (0xc1, 128),
    ] {
        row[offset] = value;
    }
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "86fad35ec449a704d6560a3b46f226d40dbeea1cc6e174a1c8385cb923dd54cd"
    );
    let id = EffectId {
        bank: EffectBank::Magic(112),
        id: 1,
    };
    let mut cooker = test_cooker();
    cooker.material_indices.insert(
        MaterialKey {
            texture: TextureBank::Magic(112),
            color: 3,
            alpha: Some(1),
            stride: 32,
        },
        0,
    );
    let actor = cooker.actor(&row, id, &[], false).unwrap();
    assert!(matches!(
        actor.geometry,
        Geometry::Ring {
            segments: 16,
            flared: false,
            lines: false,
            repeat_uv: false,
            uv_columns: 0
        }
    ));
    assert_eq!(actor.blend, Blend::Alpha);
    assert_eq!(actor.lifetime, Some(10));
    assert_eq!(actor.uv, [97, 17, 62, 46]);
    assert_eq!(actor.position, [0., 0., 16.]);
    assert_eq!(actor.dimensions, [80., 32., 128.]);
    assert_eq!(actor.dimension_velocity, [2., 4., 0.]);
    assert_eq!(actor.angular_velocity, [0., 0.1, 0.]);
    assert!(actor.bottom_anchored && actor.ground.is_none());
    row[0x90] = 2;
    assert!(cooker.actor(&row, id, &[], false).is_err());
    row[0x90] = 1;
    row[0] = 5;
    assert!(cooker.actor(&row, id, &[], false).is_err());
}

#[test]
#[ignore = "requires privately extracted US assets; parses all records and palette references without encoding"]
fn original_pow_followups_close_all_programs_actors_modifiers_and_palettes() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let magic = MagicArchive::read(&extracted).unwrap();
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let fixed = member(member(&usual, 4).unwrap(), 4).unwrap();
    for (package, actor_count, digest, ends, impact, ground, release, startup) in [
        (
            112,
            9,
            "7af6b0f698253e49951bba49d59c7f7ee566ac8035579cfdea9d8e9e44d1e25e",
            [0, 0, 74, 5, 0, 4],
            3,
            4,
            5,
            2,
        ),
        (
            114,
            10,
            "1d76cf8429d089359c0677ecfefe4bd9d51fe383110d5cd327d96eb1bc019e13",
            [0, 0, 5, 0, 4, 46],
            2,
            3,
            4,
            5,
        ),
    ] {
        let source = magic.package(package).unwrap();
        let bytes = magic_member(source, 4).unwrap().unwrap();
        let bank = EffectBank::Magic(package);
        let effect = |id| EffectId { bank, id };
        assert_eq!((bytes[4], bytes[5]), (6, 0));
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(&bytes[..usize::from(half(bytes, 18).unwrap())])
            ),
            digest
        );
        let mut cooker = test_cooker();
        for id in 0..actor_count {
            let row = actor_source(bytes, effect(id)).unwrap();
            if id == 0 {
                assert_eq!(
                    controller(row).unwrap(),
                    Some(EffectController::Shake {
                        duration: 12,
                        amplitude: 16
                    })
                );
                continue;
            }
            if row[2] != 255 {
                let key = MaterialKey {
                    texture: texture_bank(bank, row[2]).unwrap(),
                    color: row[3],
                    alpha: (word(row, 20).unwrap() & 0x4000000 != 0).then_some(row[4]),
                    stride: row[6],
                };
                if !cooker.material_indices.contains_key(&key) {
                    let atlas = match key.texture {
                        TextureBank::Magic(p) if p == package => {
                            magic_member(source, 8).unwrap().unwrap()
                        }
                        TextureBank::Fixed(1) if package == 114 && id >= 7 => fixed,
                        _ => panic!("unexpected Pow followup texture binding"),
                    };
                    let textures = tpl::parse_tpl(atlas).unwrap();
                    for (image, palette) in [(0, Some(key.color)), (1, key.alpha)] {
                        if let Some(palette) = palette {
                            let texture = &textures[image];
                            let stride = if key.stride != 0 {
                                usize::from(key.stride)
                            } else if texture.format == 9 {
                                256
                            } else {
                                16
                            };
                            assert!(usize::from(palette) * stride < texture.palette_entries);
                            assert!(texture.palette_offset.is_some());
                        }
                    }
                    let index = cooker.result.materials.len() as u16;
                    cooker.result.materials.push(EffectMaterial {
                        texture: UiTexture {
                            path: format!("fixture-{index}.ktx2"),
                            width: u32::from(textures[0].width),
                            height: u32::from(textures[0].height),
                        },
                        rgb_scale: 2.,
                    });
                    cooker.material_indices.insert(key, index);
                }
            }
            // Also parse declared actors not reached by a program, without inventing a request.
            cooker.recover_actor(bytes, effect(id), false).unwrap();
        }
        // Native requests must pull in ground children without explicit child selection.
        for id in [startup, impact, release] {
            cooker.program(bytes, effect(id)).unwrap();
        }
        assert!(cooker.result.program(effect(ground)).is_some());
        assert_eq!(cooker.result.programs.len(), 4);
        for id in 0..6 {
            cooker.program(bytes, effect(id)).unwrap();
        }
        cooker.result.validate().unwrap();
        assert_eq!(cooker.result.actors.len(), usize::from(actor_count) - 1);
        assert_eq!(cooker.result.programs.len(), 6);
        for (id, end) in ends.into_iter().enumerate() {
            let program = cooker.result.program(effect(id as u8)).unwrap();
            assert_eq!(program.end_tick, end);
            if id < 2 {
                assert!(program.emissions.is_empty());
            }
        }
        let splash = cooker
            .result
            .actor(effect(if package == 112 { 3 } else { 1 }))
            .unwrap();
        assert!(
            matches!(splash.ground, Some(GroundResponse::Bounce { effect:Some(child) }) if child == effect(ground))
        );
        assert_eq!(splash.lifetime, Some(48));
        let startup = cooker.result.program(effect(startup)).unwrap();
        if package == 112 {
            assert!(matches!(
                startup.emissions[0],
                EffectEmission {
                    tick: 42,
                    command: EffectCommand::Controller {
                        controller: EffectController::Shake {
                            duration: 12,
                            amplitude: 16
                        },
                        ..
                    },
                    ..
                }
            ));
        } else {
            assert!(
                startup
                    .emissions
                    .iter()
                    .all(|e| matches!(e.command, EffectCommand::Particle { .. }))
            );
            for id in [7, 8] {
                assert!(matches!(
                    cooker.result.actor(effect(id)).unwrap().geometry,
                    Geometry::Spiral { .. }
                ));
            }
        }
    }
}
