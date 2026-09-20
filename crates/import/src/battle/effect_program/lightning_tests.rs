//! The entire original native216..219 effect closure, without texture encoding or writes.
use super::*;

#[test]
#[ignore = "requires privately extracted US assets; parses full timelines, models and actual palettes"]
fn original_lightning_family_closes_random_models_palettes_and_ribbon_flags() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    // Destination 3 is a signed halfword spanning color and alpha, unlike byte opcode 12.
    for (address, instruction) in [
        (0x3f14c, 0x7fbf1a14),
        (0x3f420, 0x2c000003),
        (0x3f428, 0xa81d0000),
        (0x3f42c, 0x7c00ca14),
        (0x3f430, 0xb01d0000),
    ] {
        assert_eq!(word(rel.at((1, address)).unwrap(), 0).unwrap(), instruction);
    }
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let mut cooker = test_cooker();
    let textures = member(&usual, 4).unwrap();
    cooker.textures.insert(
        TextureBank::Fixed(0),
        compression::decode(member(textures, 1).unwrap()).unwrap(),
    );
    cooker
        .textures
        .insert(TextureBank::Fixed(1), member(textures, 4).unwrap().to_vec());
    let technique = EffectId {
        bank: EffectBank::Techniques,
        id: 28,
    };
    cooker
        .program(member(&usual, 3).unwrap(), technique)
        .unwrap();
    assert_eq!(cooker.result.program(technique).unwrap().end_tick, 18);
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|a| a.id.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([43, 44, 45])
    );
    let mut model_errors = Vec::new();
    for (package, digest, ends, model_count, actor_count) in [
        (
            105,
            "da7a812973427246bc6dc7195541bb848905eba1daca8228e35f7d18fc9aabff",
            &[180, 0, 0, 0, 0, 0, 0, 0, 0, 0][..],
            2,
            7,
        ),
        (
            110,
            "2a893f9bfc04f0bffbf9a40f3851adf97d048bb387ff9a245e50a6eb59eb2cd2",
            &[0, 0, 0, 12][..],
            5,
            5,
        ),
        (
            111,
            "f5ff4fe90c9bf8bf3d1b39fce2f9980d9ce1b468b685d06d4310ed9b6a61e702",
            &[140, 0][..],
            3,
            7,
        ),
        (
            107,
            "56bb88ee3f98d309017f3cace9f03c071dbdffcfe9f34e78cc49c69550023990",
            &[190][..],
            5,
            4,
        ),
        (
            108,
            "e39d467af98da95906e9b4bece3163f61c70f787a4c26dd3c53b45fbdc8f4404",
            &[180][..],
            7,
            6,
        ),
        (
            17,
            "74775ae60c474bc63e8e6e2794c41ae38604e7823b7e14a176ffd479e2ffc601",
            &[130][..],
            6,
            5,
        ),
        (
            18,
            "2c6567fb1832095736dd18788440bd95f0e1ab5732d74af0f0dec01642f747b6",
            &[240][..],
            7,
            16,
        ),
        (
            19,
            "ba7459dc14be2e4ea1e886fa70ed020f75d0e5e87ee714b523fcf48725a9c9bb",
            &[90][..],
            3,
            12,
        ),
        (
            24,
            "b27d215f9229f75149757c6b35c5789bc8c3058591e9d8a3771fe8f2aabcd4c4",
            &[150][..],
            1,
            8,
        ),
        (
            29,
            "84e3b28b3c4bf5e42475c8519e8fd375fb2fc207b129c56ca88ff4039f4e9443",
            &[110][..],
            3,
            8,
        ),
        (
            30,
            "b5316a0d1af43349fb49f6c606f8f2de52d1cc104537547fede82182bd12b032",
            &[30, 8, 14][..],
            8,
            9,
        ),
        (
            31,
            "34711ffea12d988541d658433bd621660a4377d2f8cb003c1b646f0e255c0f06",
            &[200][..],
            5,
            3,
        ),
        (
            33,
            "5b9c583d272ee3302aecd0ee73f7090e8a937e453b8258a02103060999f4f88d",
            &[0, 0, 36][..],
            1,
            7,
        ),
        (
            26,
            "5772b83483225c1ccce55552831b4f2d4a6bd13bfeeadb2af0b3673344cad4e0",
            &[0, 0, 1][..],
            3,
            8,
        ),
    ] {
        let source = archive.package(package).unwrap();
        let bytes = magic_member(source, 4).unwrap().unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), digest);
        cooker.textures.insert(
            TextureBank::Magic(package),
            magic_member(source, 8).unwrap().unwrap().to_vec(),
        );
        let id = EffectId {
            bank: EffectBank::Magic(package),
            id: 1,
        };
        for (index, &end) in ends.iter().enumerate() {
            let root = EffectId {
                id: index as u8 + 1,
                ..id
            };
            cooker.program(bytes, root).unwrap();
            assert_eq!(cooker.result.program(root).unwrap().end_tick, end);
        }
        assert_eq!(
            cooker
                .result
                .actors
                .iter()
                .filter(|a| a.id.bank == id.bank)
                .count(),
            actor_count
        );
        let models = cooker
            .result
            .models()
            .filter_map(|model| match model {
                ModelRef::Magic { package: p, index } if p == package => Some(index),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        // Absolute's fragments add random %4 to model4; its unused model2 is not requested.
        assert_eq!(
            models,
            (0..model_count)
                .filter(|&i| package != 30 || i != 2)
                .filter(|&i| package != 108 || i != 5)
                .collect()
        );
        for index in models {
            let model = magic_member(source, 12 + usize::from(index) * 4)
                .unwrap()
                .unwrap();
            // These members are texture/GPL/model containers, not map section tables.
            // Normalize shared outline palettes and run the same read-only geometry
            // and material checks as actual model cooking. Inspect every model root.
            if let Err(error) = crate::model_preview::preflight(crate::model_preview::Layer {
                model,
                outline: magic_member(source, 52 + usize::from(index) * 4).unwrap(),
                animation: None,
                attached_to: None,
                additive: false,
            }) {
                model_errors.push(format!("Magic{package}/model{index}: {error:#}"));
            }
        }
    }
    for (package, actor, expected) in [
        (111, 4, (8..16).collect()),
        (111, 5, (8..16).collect()),
        (111, 6, (8..16).collect()),
        (107, 3, BTreeSet::from([1, 2, 3, 4])),
        (108, 4, BTreeSet::from([0x101, 0x102, 0x103, 0x104])),
        (108, 5, BTreeSet::from([0x101, 0x102, 0x103, 0x104])),
        (18, 8, BTreeSet::from([5, 6])),
        (19, 12, BTreeSet::from([1, 2, 3, 4])),
    ] {
        let actor = cooker
            .result
            .actor(EffectId {
                bank: EffectBank::Magic(package),
                id: actor,
            })
            .unwrap();
        assert_eq!(
            actor
                .palette
                .as_ref()
                .unwrap()
                .materials
                .keys()
                .copied()
                .collect::<BTreeSet<_>>(),
            expected
        );
        if package == 108 {
            let palette = actor.palette.as_ref().unwrap();
            assert_eq!((palette.index, palette.alpha), (1, Some(1)));
            for (&key, &material) in &palette.materials {
                let [color, alpha] = key.to_be_bytes();
                let row = actor_source(
                    magic_member(archive.package(package).unwrap(), 4)
                        .unwrap()
                        .unwrap(),
                    actor.id,
                )
                .unwrap();
                assert_eq!(
                    cooker.material_indices.get(&MaterialKey {
                        texture: texture_bank(actor.id.bank, row[2]).unwrap(),
                        color,
                        alpha: (word(row, 0x14).unwrap() & 0x4000000 != 0).then_some(alpha),
                        stride: row[6]
                    }),
                    Some(&material)
                );
            }
        }
    }
    let prism = EffectId {
        bank: EffectBank::Magic(111),
        id: 4,
    };
    let palette = &cooker
        .result
        .actor(prism)
        .unwrap()
        .palette
        .as_ref()
        .unwrap()
        .materials;
    for id in 4..=6 {
        let actor = cooker.result.actor(EffectId { id, ..prism }).unwrap();
        assert_eq!(&actor.palette.as_ref().unwrap().materials, palette);
        assert_eq!(
            actor.orientation,
            if id < 6 {
                Orientation::FollowMotion
            } else {
                Orientation::Billboard
            }
        );
        assert!(actor.follow_emitter);
    }
    for emission in &cooker
        .result
        .program(EffectId { id: 2, ..prism })
        .unwrap()
        .emissions
    {
        if let EffectCommand::Particle {
            actor, modifiers, ..
        } = &emission.command
            && matches!(actor.id, 5 | 6)
        {
            assert!(matches!(
                modifiers.first(),
                Some(Modifier::RequireIntegerRange {
                    index: 3,
                    min: 0,
                    max: 7
                })
            ));
            assert!(!modifiers.iter().any(|m| matches!(
                m,
                Modifier::RandomInteger {
                    field: IntegerField::Temporary(3),
                    ..
                }
            )));
        }
    }
    let thunder = cooker
        .result
        .program(EffectId {
            bank: EffectBank::Magic(19),
            id: 1,
        })
        .unwrap();
    assert!(thunder.emissions.iter().any(|e|matches!(&e.command,EffectCommand::Particle{actor,modifiers,..}if actor.id==7 && modifiers.iter().any(|m|matches!(m,Modifier::Flag { field: EffectFlag::RibbonDepth, enabled: true })))));
    assert!(model_errors.is_empty(), "{}", model_errors.join("\n"));
    cooker.result.validate().unwrap();
    assert!(!cooker.pending_images.is_empty()); // Real palette decoding, no substitute materials.
}

#[test]
fn ribbon_flag_has_a_typed_geometry_guard() {
    let mut bytes = vec![0; 2];
    bytes.extend([0, 21, 0, 20, 0, 0, 0, 32, 255, 255]);
    assert!(matches!(
        modifiers(&bytes, 2).unwrap().as_slice(),
        [Modifier::Flag {
            field: EffectFlag::RibbonDepth,
            enabled: true
        }]
    ));
    let mut row = [0; ACTOR_BYTES];
    row[0] = 18;
    row[2] = 255;
    row[0x32] = 255;
    row[0x12] = 4;
    row[0xc0..0xc4].copy_from_slice(&100f32.to_be_bytes());
    let mut actor = test_cooker()
        .actor(
            &row,
            EffectId {
                bank: EffectBank::Magic(19),
                id: 7,
            },
            &[],
            false,
        )
        .unwrap();
    actor
        .validate_modifiers(&[Modifier::Flag {
            field: EffectFlag::RibbonDepth,
            enabled: true,
        }])
        .unwrap();
    actor.geometry = Geometry::Quad;
    assert!(
        actor
            .validate_modifiers(&[Modifier::Flag {
                field: EffectFlag::RibbonDepth,
                enabled: true
            }])
            .is_err()
    );
}
