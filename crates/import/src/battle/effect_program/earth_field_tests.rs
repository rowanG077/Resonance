//! Every reachable original model and emission is required, including retained actors.
use super::*;

#[test]
#[ignore = "requires original extracted US assets; parses effects, palettes and models"]
fn original_ground_dasher_and_grave_close_all_models_retained_updates_and_audio() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let mut cooker = test_cooker();
    for (package, model_count, end) in [(14, 3, 160), (15, 5, 120), (23, 4, 180), (28, 6, 150)] {
        let source = archive.package(package).unwrap();
        let bytes = magic_member(source, 4).unwrap().unwrap();
        let bank = EffectBank::Magic(package);
        assert_eq!(bytes[4], 2);
        cooker.textures.insert(
            TextureBank::Magic(package),
            magic_member(source, 8).unwrap().unwrap().to_vec(),
        );
        cooker.program(bytes, EffectId { bank, id: 1 }).unwrap();
        let models = cooker
            .result
            .models()
            .filter_map(|model| match model {
                ModelRef::Magic {
                    package: actual,
                    index,
                } if actual == package => Some(index),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        // Grave adds Random%3 to the signed halfword [model=4, selector=0].
        // This changes only its dormant selector, leaving package models 5/6 unreachable.
        assert_eq!(models, (0..model_count).collect());
        for index in models {
            crate::model_preview::preflight(crate::model_preview::Layer {
                model: magic_member(source, 12 + usize::from(index) * 4)
                    .unwrap()
                    .unwrap(),
                outline: magic_member(source, 52 + usize::from(index) * 4).unwrap(),
                animation: None,
                attached_to: None,
                additive: false,
            })
            .unwrap();
        }
        let root = cooker.result.program(EffectId { bank, id: 1 }).unwrap();
        assert_eq!(root.end_tick, end);
        let sounds = root
            .emissions
            .iter()
            .filter_map(|e| match e.command {
                EffectCommand::Sound { sound, .. } => {
                    Some((e.tick, sound, e.repeat.map(|r| (r.count, r.interval))))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if package == 14 {
            assert_eq!(sounds, [(0, 90, Some((8, 20)))]);
        } else if package == 15 {
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                "d99cfd4c6cdb21a8b650bfd43bff62c18a37ff83ab9c4c43dd5e14efb7450b6c"
            );
            let actor = cooker.result.actor(EffectId { bank, id: 4 }).unwrap();
            assert!(matches!(
                actor.geometry,
                Geometry::Model {
                    model: ModelRef::Magic {
                        package: 15,
                        index: 4
                    },
                    animation: None,
                    presentation: ModelPresentation {
                        animation_selector: 0,
                        external_animation: false,
                        ..
                    },
                    ..
                }
            ));
            let selected = root
                .emissions
                .iter()
                .filter_map(|e| match &e.command {
                    EffectCommand::Particle {
                        actor: EffectId { id: 4, .. },
                        modifiers,
                        ..
                    } => Some((e.tick, modifiers)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                selected.iter().map(|(tick, _)| *tick).collect::<Vec<_>>(),
                [32, 74, 84, 94, 104]
            );
            for (_, modifiers) in selected {
                let at = modifiers
                    .iter()
                    .position(|m| {
                        matches!(
                            m,
                            Modifier::Integer {
                                field: IntegerField::ModelSelection,
                                operation: Arithmetic::Add,
                                value: IntegerValue::Temporary(0)
                            }
                        )
                    })
                    .unwrap();
                assert!(
                    at > 0
                        && matches!(
                            modifiers[at - 1],
                            Modifier::RandomInteger {
                                field: IntegerField::Temporary(0),
                                modulus: 3
                            }
                        )
                );
                assert_eq!(model_indices(4, 0, modifiers).unwrap(), BTreeSet::from([4]));
            }
            assert_eq!(
                sounds,
                [
                    (40, 89, None),
                    (82, 89, None),
                    (92, 89, None),
                    (102, 89, None),
                    (112, 89, None)
                ]
            );
            assert_eq!(
                root.emissions
                    .iter()
                    .filter_map(
                        |e| matches!(e.command, EffectCommand::ModifyRetained { .. })
                            .then_some(e.tick)
                    )
                    .collect::<Vec<_>>(),
                [40, 82, 92, 102, 112]
            );
        } else {
            let (sound, count, interval, digest) = if package == 23 {
                (
                    140,
                    6,
                    30,
                    "8594c18600daa538a0f230d5b8aa56b5b698513aa6b688ad950f28cd3c2f3f1a",
                )
            } else {
                (
                    91,
                    7,
                    20,
                    "4247ac2d88d28d565e127eb7b48544b449fb5e83ce9b165334f8e40782e64f39",
                )
            };
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), digest);
            assert_eq!(sounds, [(0, sound, Some((count, interval)))]);
            if package == 28 {
                let modifiers = root
                    .emissions
                    .iter()
                    .find_map(|e| match &e.command {
                        EffectCommand::Particle {
                            actor: EffectId { id: 5, .. },
                            modifiers,
                            ..
                        } => Some(modifiers),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    model_indices(2, 0, modifiers).unwrap(),
                    BTreeSet::from([2, 3, 4, 5])
                );
                assert!(
                    !modifiers
                        .iter()
                        .any(|m| matches!(m, Modifier::RequireFreshIntegers))
                );
                assert!(root.emissions.iter().any(|e| e.tick == 5
                    && matches!(e.command, EffectCommand::ModifyRetained { slot: 0, .. })));
            }
        }
    }
    cooker.result.validate().unwrap();
}

#[test]
fn grave_model_word_decodes_as_halfword_not_byte_or_adjacent_runtime_state() {
    let mut bytes = vec![0, 0];
    for value in [2u16, 0x7ffc, 3, 0, 3, 0xd4, 0x7ffc, 0, 0xffff] {
        bytes.extend(value.to_be_bytes());
    }
    let decoded = modifiers(&bytes, 2).unwrap();
    assert!(matches!(
        decoded.as_slice(),
        [
            Modifier::RandomInteger {
                field: IntegerField::Temporary(0),
                modulus: 3
            },
            Modifier::Integer {
                field: IntegerField::ModelSelection,
                operation: Arithmetic::Add,
                value: IntegerValue::Temporary(0)
            }
        ]
    ));
    for destination in [0xd5u16, 0xd6] {
        bytes[12..14].copy_from_slice(&destination.to_be_bytes());
        assert!(modifiers(&bytes, 2).is_err());
    }
}
