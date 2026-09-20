use super::*;
#[cfg(test)]
use crate::battle::effect_program::{MagicArchive, magic_member};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    directions: [[f32; 3]; 2],
    distances: [f32; 2],
    color: [u8; 4],
    effect_scale: f32,
    feedback_tick: u16,
    feedback_ticks: u16,
    feedback_limit: u8,
    projectiles: [Option<Projectile>; 2],
    initial_effect: PairEffect,
    voice: PairVoice,
    voice_role: u8,
    voice_before_effect: bool,
    hide_attachments: [bool; 2],
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Projectile {
    tick: u16,
    id: u8,
    rule: usize,
    end_tick: Option<u16>,
    origin: PairOrigin,
    effect: Option<u8>,
    pattern: PairProjectilePattern,
}

pub(super) fn cook(inputs: &Inputs) -> Result<BTreeMap<CombinedPair, CombinedPairProgram>> {
    CombinedPair::ALL
        .into_iter()
        .map(|kind| program(inputs, kind).map(|p| (kind, p)))
        .collect()
}

pub(super) fn read_parameters(rel: &Rel, kind: CombinedPair) -> Result<Parameters> {
    let final_pair = matches!(
        kind,
        CombinedPair::Stardust | CombinedPair::Mjollnir | CombinedPair::Prism
    );
    let (initializer, constants, hook, feedback, release, voice_address, voice) = match kind {
        CombinedPair::Stardust => (0x83c3c, 0x74c0, 0x839c8, 40, 30, 0x83d84, 0x87bb),
        CombinedPair::Mjollnir => (0x855c0, 0x7a68, 0x8542c, 90, 86, 0x85728, 0x87ba),
        CombinedPair::Prism => (0x86294, 0x7c10, 0x8608c, 30, 30, 0x863bc, 0x87c3),
        CombinedPair::Punishment => (0x847d8, 0x7830, 0x8470c, 30, 30, 0x849b8, 0x87b1),
        CombinedPair::ArchWind => (0x84b40, 0x78a8, 0x84a74, 30, 30, 0x84d28, 0x87c1),
        CombinedPair::Tempest => (0x84ed8, 0x7920, 0x84e08, 55, 60, 0x850e8, 0x879a),
        CombinedPair::Blast => (0x923e8, 0x99d8, 0x92318, 70, 88, 0x925f8, 0x87b3),
        CombinedPair::Plasma => (0x9299c, 0x9ac8, 0x92888, 70, 70, 0x92bbc, 0x87b3),
    };
    let dispatch = rel.pointer(5, 0xf94 + usize::from(kind.native() - 300) * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, initializer)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected paired combination dispatch"
    );
    let pins = if final_pair {
        let mut pins = match kind {
            CombinedPair::Stardust => vec![
                (0x839e8, 0x2c000028),
                (0x839f8, 0x38a0008c),
                (0x83a00, 0x38800008),
                (0x83a48, 0x28000002),
                (0x83a54, 0x2c00001e),
                (0x83a5c, 0x2c0000b4),
                (0x83c18, 0x38070001),
            ],
            CombinedPair::Mjollnir => vec![
                (0x8544c, 0x2c00005a),
                (0x8545c, 0x38a0001e),
                (0x85464, 0x38800008),
                (0x854ac, 0x28000002),
                (0x854b8, 0x2c00002a),
                (0x854c0, 0x80be18c0),
                (0x854dc, 0x38e00001),
                (0x85500, 0x2c000056),
                (0x85534, 0x38a00004),
                (0x85580, 0x812518c0),
                (0x8558c, 0x38e00002),
            ],
            CombinedPair::Prism => vec![
                (0x860b8, 0x2c00001e),
                (0x860c8, 0x38a0008c),
                (0x860d0, 0x3880000c),
                (0x86118, 0x28000003),
                (0x86124, 0x2c03001e),
                (0x8612c, 0x2c030096),
                (0x8613c, 0x806418c0),
                (0x86188, 0x38030005),
                (0x86220, 0x38e00001),
                (0x86268, 0x38000001),
            ],
            _ => unreachable!(),
        };
        pins.push((voice_address, 0x38840000 | u32::from(voice)));
        pins
    } else if kind.presea() {
        vec![
            (hook + 0x18, 0x2c00001e),
            (hook + 0x28, 0x38c00082),
            (hook + 0x78, 0x28000007),
            (hook + 0x84, 0x2c00001e),
            (hook + 0x98, 0x38e00001),
            (hook + 0x9c, 0x8125195c),
            (voice_address, 0x38840000 | u32::from(voice)),
        ]
    } else {
        let branch = 0x78;
        vec![
            (hook + 0x18, 0x2c000000 | u32::from(feedback)),
            (hook + 0x28, 0x38c00028),
            (hook + branch, 0x28000004),
            (hook + branch + 12, 0x2c000000 | u32::from(release)),
            (hook + branch + 24, 0x3909001c),
            (hook + branch + 40, 0x38e00002),
            (voice_address, 0x38840000 | u32::from(voice)),
        ]
    };
    for (address, instruction) in pins {
        ensure!(
            word(rel.at((1, address))?, 0)? == instruction,
            "unexpected paired combination callback at {address:#x}"
        );
    }
    if kind == CombinedPair::Plasma {
        for (address, instruction) in [
            (0x92950, 0x2c00004e),
            (0x9295c, 0x39090038),
            (0x9296c, 0x38e00003),
        ] {
            ensure!(
                word(rel.at((1, address))?, 0)? == instruction,
                "unexpected Plasma sword callback"
            );
        }
    }
    if kind == CombinedPair::ArchWind {
        ensure!(
            word(rel.at((1, 0x84d18))?, 0)? == 0x28000006
                && word(rel.at((1, 0x84d48))?, 0)? == 0x388487c2,
            "unexpected Arch Wind voice selection"
        );
    }
    let constants = rel.at((4, constants))?;
    let (first_angle, second_angle, zero, first_distance, second_distance, scale) = match kind {
        CombinedPair::Stardust => (40, 64, 48, 52, 52, 56),
        CombinedPair::Mjollnir => (8, 24, 16, 20, 20, 4),
        CombinedPair::Prism => (40, 56, 20, 48, 48, 52),
        _ if kind.presea() => (8, 24, 16, 20, 32, 36),
        CombinedPair::Tempest => (8, 32, 16, 20, 20, 24),
        _ => (8, 32, 16, 20, 40, 24),
    };
    let direction = |offset| -> Result<[f32; 3]> {
        let angle = f64::from_be_bytes(
            constants
                .get(offset..offset + 8)
                .context("truncated paired combination angle")?
                .try_into()?,
        );
        Ok([
            angle.cos() as f32,
            float(constants, zero)?,
            angle.sin() as f32,
        ])
    };
    let projectile = |tick, id, rule: usize| -> Result<_> {
        Ok(Projectile {
            tick,
            id,
            end_tick: None,
            origin: PairOrigin::TargetAim,
            effect: None,
            pattern: PairProjectilePattern::Fixed,
            rule,
        })
    };
    let data = Parameters {
        directions: [direction(first_angle)?, direction(second_angle)?],
        distances: [
            float(constants, first_distance)?,
            float(constants, second_distance)?,
        ],
        color: constants[..4].try_into()?,
        effect_scale: float(constants, scale)?,
        feedback_tick: feedback,
        feedback_ticks: match kind {
            CombinedPair::Stardust | CombinedPair::Prism => 140,
            CombinedPair::Mjollnir => 30,
            _ if kind.presea() => 130,
            _ => 40,
        },
        feedback_limit: if kind == CombinedPair::Prism { 12 } else { 8 },
        voice_role: if final_pair { 0 } else { 1 },
        voice_before_effect: kind != CombinedPair::Mjollnir,
        hide_attachments: [kind == CombinedPair::Stardust, false],
        projectiles: if kind == CombinedPair::Stardust {
            [
                Some(Projectile {
                    end_tick: Some(180),
                    origin: PairOrigin::ActorRoot,
                    pattern: PairProjectilePattern::Stardust {
                        offset: [0., float(constants, 16)?, 0.],
                        spread: [
                            float(constants, 20)?,
                            float(constants, 28)?,
                            float(constants, 20)?,
                        ],
                        modulus: 100,
                        scale: float(constants, 24)?,
                        variants: 3,
                    },
                    ..projectile(30, 1, 0)?
                }),
                None,
            ]
        } else if kind == CombinedPair::Prism {
            [
                Some(Projectile {
                    end_tick: Some(150),
                    origin: PairOrigin::TargetRoot,
                    pattern: PairProjectilePattern::Prism {
                        angle_choices: 30,
                        angle_offset: 5,
                        angle_step: 12,
                        radians_per_degree: float(constants, 16)?,
                        distance: float(constants, 24)?,
                        contact_interval: 4,
                        short_contact_duration: 1,
                    },
                    ..projectile(30, 1, 0)?
                }),
                None,
            ]
        } else if kind == CombinedPair::Mjollnir {
            [
                Some(Projectile {
                    origin: PairOrigin::ActorRoot,
                    ..projectile(42, 1, 0)?
                }),
                Some(Projectile {
                    origin: PairOrigin::TargetRoot,
                    effect: Some(4),
                    ..projectile(86, 2, 1)?
                }),
            ]
        } else {
            [
                if kind == CombinedPair::Plasma {
                    Some(projectile(78, 3, 2)?)
                } else {
                    None
                },
                Some(if kind.presea() {
                    projectile(release, 1, 0)?
                } else {
                    projectile(release, 2, 1)?
                }),
            ]
        },
        initial_effect: if matches!(kind, CombinedPair::Stardust | CombinedPair::Prism) {
            PairEffect::World {
                role: 0,
                position: [
                    float(constants, 4)?,
                    float(constants, 8)?,
                    float(constants, 12)?,
                ],
            }
        } else if kind.presea() {
            PairEffect::FixedRoot { role: 1 }
        } else {
            PairEffect::default()
        },
        voice: if kind == CombinedPair::ArchWind {
            PairVoice::ByPrimary(BTreeMap::from([(6, voice), (9, 0x87c2)]))
        } else {
            PairVoice::Fixed(voice)
        },
    };
    Ok(data)
}

fn program(inputs: &Inputs, kind: CombinedPair) -> Result<CombinedPairProgram> {
    let parameters = inputs
        .parameters
        .pairs
        .get(&kind)
        .context("missing pair parameters")?;
    let package = inputs.package(kind.native())?;
    let final_pair = matches!(
        kind,
        CombinedPair::Stardust | CombinedPair::Mjollnir | CombinedPair::Prism
    );
    ensure!(
        (kind.presea() || final_pair || package.resources.models == [0; 10])
            && package.resources.callback_resources == [0; 4],
        "unexpected paired combination model resource"
    );
    let source = &package.actions;
    let projectile = |value: Option<Projectile>| -> Result<_> {
        value
            .map(|value| {
                Ok(CombinedPairProjectile {
                    tick: value.tick,
                    id: value.id,
                    rule: source.rule(value.rule)?,
                    end_tick: value.end_tick,
                    origin: value.origin,
                    effect: value.effect,
                    pattern: value.pattern,
                })
            })
            .transpose()
    };
    let data = CombinedPairProgram {
        phases: (0..kind.phase_count())
            .map(|i| phase(source, usize::from(i)))
            .collect::<Result<_>>()?,
        directions: parameters.directions,
        distances: parameters.distances,
        color: parameters.color,
        effect_scale: parameters.effect_scale,
        feedback_tick: parameters.feedback_tick,
        feedback_ticks: parameters.feedback_ticks,
        feedback_limit: parameters.feedback_limit,
        projectiles: [
            projectile(parameters.projectiles[0])?,
            projectile(parameters.projectiles[1])?,
        ],
        initial_effect: parameters.initial_effect,
        voice: parameters.voice.clone(),
        voice_role: parameters.voice_role,
        voice_before_effect: parameters.voice_before_effect,
        hide_attachments: parameters.hide_attachments,
    };
    ensure!(
        source
            .phases
            .get(data.phases.len())
            .is_none_or(|phase| phase.duration == 0),
        "unexpected paired combination phase boundary"
    );
    data.validate(kind)?;
    Ok(data)
}

#[test]
#[ignore = "requires original extracted US disc; no encoding"]
fn original_pairs_keep_roles_tracks_and_native_projectiles() {
    use crate::battle::effects::projectile;
    use sha2::{Digest, Sha256};
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let dol = fs::read(extracted.join("sys/main.dol")).unwrap();
    let programs = cook(&Inputs::read_source(&extracted).unwrap()).unwrap();
    // An authored null keeps the current pose; an absent table slot is an error.
    let colette =
        crate::battle::visual::party::archive(&dol, &extracted.join("files"), 2, 0).unwrap();
    assert!(colette.sections.get(61 + 2).is_some_and(Option::is_none));
    let archive = MagicArchive::read(&extracted).unwrap();
    for (kind, distances, primary, body_off, contacts, digest) in [
        (
            CombinedPair::Tempest,
            [450., 450.],
            1,
            18,
            4,
            "c4874903a00189dd6567ebce344fa5906b3c0efa82e1da3a5b91f887fbb5a0d1",
        ),
        (
            CombinedPair::Blast,
            [100., 300.],
            2,
            15,
            6,
            "e9fd62191b36f05708af21aae7880dd50b305a5bcd9435ae856fee5e93bfa943",
        ),
        (
            CombinedPair::Plasma,
            [200., 300.],
            6,
            40,
            1,
            "b9e830814102e44289b823fea530ac1ba0931a940edf8bfd9eb9a2fa3a6f9918",
        ),
    ] {
        let data = &programs[&kind];
        assert_eq!(data.distances, distances);
        assert_eq!(data.directions[0], [1., 0., 0.]);
        assert!(data.directions[1][0] > data.directions[1][2]);
        assert_eq!(data.color, [16, 16, 16, 255]);
        assert_eq!(kind.phase(0, primary), Some(0));
        assert!(kind.phase(1, 4).is_some());
        assert!(kind.phase(0, 4).is_none());
        assert_eq!(data.phases[0].action.hits.len(), contacts);
        assert!(
            data.phases[0]
                .action
                .commands
                .iter()
                .any(|step| step.tick == body_off
                    && matches!(step.command, ActionCommand::BodyPush { enabled: false, .. }))
        );
        let package = archive.package(kind.native() - 200).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(package)), digest);
        let rows = magic_member(package, 252).unwrap().unwrap();
        for callback in data.projectiles.iter().flatten() {
            let id = callback.id;
            let row = &rows[usize::from(id) * 400..usize::from(id + 1) * 400];
            let decoded = projectile(row, kind.effect(id), 0.1).unwrap();
            assert_eq!(word(row, 8).unwrap(), if id == 3 { 0x44a } else { 0x40a });
            assert_eq!(decoded.lifetime, if id == 3 { 20 } else { 10 });
            assert_eq!(decoded.spawn_effect, (id == 3).then(|| kind.effect(2)));
            assert!(decoded.trail_effect.is_none());
            assert_eq!(decoded.birth_bank, Some(kind.effect(0).bank));
            assert_eq!(decoded.behavior.clamp_ground, id == 3);
            assert_eq!(
                (
                    callback.rule.power,
                    callback.rule.hitstun,
                    callback.rule.contact_cooldown
                ),
                (if id == 3 { 200 } else { 400 }, 50, 30)
            );
        }
        let combined =
            dol::slice(&dol, 0x80208688 + u32::from(kind.combination()) * 64, 64).unwrap();
        assert_eq!(
            (half(combined, 4).unwrap(), half(combined, 6).unwrap()),
            (kind.native(), 2)
        );
        let mut invalid = data.clone();
        invalid.projectiles[1] = None;
        assert!(invalid.validate(kind).is_err());
    }
    for (kind, count, element, digest) in [
        (
            CombinedPair::Punishment,
            4,
            5,
            "65420b6e41a0e1dfabde85d358cd8f854fe7a799523fba7a784228a7acf13079",
        ),
        (
            CombinedPair::ArchWind,
            3,
            3,
            "13ed538be737c436d7ab7af863794d917b0676408143d00c7a8458a0713a3c14",
        ),
    ] {
        let data = &programs[&kind];
        assert_eq!(data.phases.len(), count);
        assert_eq!(kind.phase(1, 7), Some(count as u8 - 1));
        assert!(kind.phase(1, 8).is_none());
        assert_eq!(data.initial_effect, PairEffect::FixedRoot { role: 1 });
        assert_eq!(data.distances, [250., 50.]);
        assert_eq!(data.directions[1], [1., 0., 0.]);
        assert_eq!((data.feedback_tick, data.feedback_ticks), (30, 130));
        assert!(data.phases.iter().all(|p| p.action.hits.is_empty()));
        let presea = &data.phases[count - 1].action;
        assert_eq!(
            presea
                .animations
                .instructions
                .values()
                .filter_map(|instruction| match instruction {
                    resonance_content::battle::actions::AnimationInstruction::Step(step) =>
                        match step.trigger {
                            resonance_content::battle::actions::AnimationTrigger::Tick(tick) =>
                                Some(tick),
                            _ => panic!("unexpected Presea animation trigger"),
                        },
                    resonance_content::battle::actions::AnimationInstruction::End => None,
                    _ => panic!("unexpected stalled program"),
                })
                .collect::<Vec<_>>(),
            [38, 160]
        );
        assert_eq!(
            presea.commands.iter().map(|s| s.tick).collect::<Vec<_>>(),
            [0, 20, 34, 48, 62, 76, 90, 104, 118, 132, 146]
        );
        assert!(matches!(
            presea.commands[0].command,
            ActionCommand::AttachmentTrail {
                slot: 0,
                ticks: 600
            }
        ));
        let (sounds, voices) = presea.audio_ids();
        assert_eq!(sounds.into_iter().collect::<Vec<_>>(), [63]);
        assert!(voices.is_empty());
        let package = archive.package(kind.native() - 200).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(package)), digest);
        let action = magic_member(package, 256).unwrap().unwrap();
        let rule = word(action, 0).unwrap() as usize;
        assert_eq!(
            (half(action, rule).unwrap(), action[rule + 2]),
            (0x20, element)
        );
        let callback = data.projectiles[1].unwrap();
        assert_eq!(
            (
                callback.tick,
                callback.id,
                callback.rule.power,
                callback.rule.hitstun,
                callback.rule.contact_cooldown
            ),
            (30, 1, 70, 35, 10)
        );
        let row = &magic_member(package, 252).unwrap().unwrap()[400..800];
        let decoded = projectile(row, kind.effect(1), 0.1).unwrap();
        assert_eq!((word(row, 8).unwrap(), decoded.lifetime), (0x409, 120));
        assert!(decoded.spawn_effect.is_none() && decoded.trail_effect.is_none());
        assert_eq!(decoded.birth_bank, Some(kind.effect(0).bank));
        if kind == CombinedPair::ArchWind {
            assert_eq!(data.voice.for_primary(6), Some(0x87c1));
            assert_eq!(data.voice.for_primary(9), Some(0x87c2));
            assert_eq!(data.voice.for_primary(3), None);
        } else {
            assert_eq!(data.voice.for_primary(3), Some(0x87b1));
        }
        let mut invalid = data.clone();
        invalid.initial_effect = PairEffect::default();
        assert!(invalid.validate(kind).is_err());
    }
    for (kind, primary, secondary, duration, distance, digest) in [
        (
            CombinedPair::Stardust,
            2,
            1,
            180,
            250.,
            "4b9e8b458dc26487c6b8871b577d54524a53d15b21813fe6b1ab0bbf7098b1d0",
        ),
        (
            CombinedPair::Mjollnir,
            2,
            3,
            160,
            200.,
            "f0f00d4b789aba2c2531426b8c0c848562aa93f2d63570fd46e1291011c29ca1",
        ),
        (
            CombinedPair::Prism,
            3,
            4,
            180,
            250.,
            "16da6d142917a7eab0a0d7df255071ce2f7e48d39e7a67329817e5fb67dffcef",
        ),
    ] {
        let data = &programs[&kind];
        assert_eq!(
            (kind.phase(0, primary), kind.phase(1, secondary)),
            (Some(0), Some(1))
        );
        assert_eq!(data.distances, [distance; 2]);
        assert_eq!(data.directions[0], [1., 0., 0.]);
        assert!(data.directions[1][0] > data.directions[1][2]);
        assert!(
            data.phases
                .iter()
                .all(|p| p.action.duration == duration && p.action.hits.is_empty())
        );
        assert_eq!(data.phases.len(), 2);
        assert_eq!(data.voice_role, 0);
        let package = archive.package(kind.native() - 200).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(package)), digest);
        let rows = magic_member(package, 252).unwrap().unwrap();
        for callback in data.projectiles.iter().flatten() {
            for id in callback.ids() {
                let row = &rows[usize::from(id) * 400..usize::from(id + 1) * 400];
                let shot = projectile(row, kind.effect(id), 0.1).unwrap();
                let (flags, life, power, birth, trail) = match kind {
                    CombinedPair::Stardust => (0x1429, 30, 50, Some(id + 1), Some(id + 4)),
                    CombinedPair::Mjollnir if id == 1 => (0x1029, 80, 500, Some(2), Some(3)),
                    CombinedPair::Mjollnir => (0x409, 8, 500, None, None),
                    CombinedPair::Prism => (0x1002a, 30, 60, Some(2), None),
                    _ => unreachable!(),
                };
                assert_eq!(
                    (word(row, 8).unwrap(), shot.lifetime, callback.rule.power),
                    (flags, life, power)
                );
                assert_eq!(shot.spawn_effect, birth.map(|id| kind.effect(id)));
                assert_eq!(shot.trail_effect, trail.map(|id| kind.effect(id)));
                assert_eq!(shot.birth_bank, Some(kind.effect(0).bank));
                assert_eq!(
                    shot.active,
                    (kind == CombinedPair::Mjollnir && id == 1).then_some([0, 70])
                );
                assert_eq!(
                    (callback.rule.hitstun, callback.rule.contact_cooldown),
                    (35, 30)
                );
                if kind == CombinedPair::Stardust {
                    assert_eq!(shot.ground_effect, Some(kind.effect(id + 7)));
                }
            }
        }
        if kind == CombinedPair::Mjollnir {
            for character in [3, 6, 9] {
                assert_eq!(kind.phase(1, character), Some(1));
            }
            assert!(matches!(
                data.phases[0].action.commands.as_slice(),
                [resonance_content::battle::actions::TimedCommand {
                    tick: 28,
                    command: ActionCommand::ForwardSpeed(6.)
                }]
            ));
        }
        let mut invalid = data.clone();
        invalid.projectiles[0].as_mut().unwrap().origin = PairOrigin::TargetAim;
        assert!(invalid.validate(kind).is_err());
    }
    let plasma = &programs[&CombinedPair::Plasma];
    assert_eq!(
        plasma
            .phases
            .iter()
            .map(|p| (
                p.action.duration,
                p.recovery_ticks,
                p.buffer_until,
                p.combo_at
            ))
            .collect::<Vec<_>>(),
        [(95, 10, 65, 60), (95, 10, 65, 60), (160, 20, 0, 0)]
    );
    assert_eq!(CombinedPair::Plasma.phase(0, 9), Some(1));
    // An old Plasma-only recipe has arrays and ordinary projectile objects, not tagged options.
    let mut legacy = serde_json::to_value(plasma).unwrap();
    for key in [
        "initial_effect",
        "feedback_limit",
        "voice_role",
        "voice_before_effect",
        "hide_attachments",
    ] {
        legacy.as_object_mut().unwrap().remove(key);
    }
    for projectile in legacy["projectiles"].as_array_mut().unwrap() {
        for key in ["end_tick", "origin", "effect", "pattern"] {
            projectile.as_object_mut().unwrap().remove(key);
        }
    }
    assert!(legacy["voice"].is_number());
    assert!(legacy["projectiles"][0].is_object());
    serde_json::from_value::<CombinedPairProgram>(legacy)
        .unwrap()
        .validate(CombinedPair::Plasma)
        .unwrap();
    for (character, menu, native, level, route, prerequisite) in [
        (1, 14, 14, 21, 1, 13),
        (1, 16, 16, 21, 2, 13),
        (2, 40, 40, 8, 0, 0),
        (2, 41, 41, 18, 1, 40),
        (2, 42, 42, 44, 1, 41),
        (3, 68, 206, 56, 1, 67),
        (3, 64, 202, 38, 1, 63),
        (3, 72, 210, 50, 1, 71),
        (3, 76, 214, 46, 1, 75),
        (4, 114, 252, 46, 1, 113),
        (3, 78, 216, 9, 0, 0),
        (3, 79, 217, 26, 2, 78),
        (3, 80, 218, 60, 1, 81),
        (3, 81, 219, 26, 1, 78),
        (6, 219, 216, 12, 0, 0),
        (6, 220, 219, 21, 0, 219),
        (9, 219, 216, 12, 0, 0),
        (9, 220, 219, 21, 0, 219),
        (6, 148, 84, 40, 0, 0),
        (9, 148, 84, 40, 0, 0),
        (7, 159, 95, 25, 0, 0),
        (7, 160, 96, 36, 1, 159),
        (7, 161, 97, 44, 1, 160),
        (7, 162, 98, 36, 2, 159),
        (1, 17, 17, 11, 0, 0),
        (1, 18, 18, 24, 1, 17),
        (1, 19, 19, 24, 2, 17),
        (2, 45, 45, 12, 0, 0),
        (2, 46, 46, 40, 1, 45),
        (2, 48, 48, 40, 2, 45),
        (4, 113, 251, 18, 0, 0),
        (6, 149, 85, 37, 0, 0),
        (6, 151, 87, 52, 0, 0),
        (9, 149, 85, 37, 0, 0),
        (9, 151, 87, 52, 0, 0),
    ] {
        let row = dol::slice(&dol, 0x80202f90 + menu * 88, 88).unwrap();
        assert_eq!(
            (
                half(row, 0).unwrap(),
                half(row, 0x3e).unwrap(),
                row[0x17],
                half(row, 0x18).unwrap()
            ),
            (native, level, route, prerequisite)
        );
        let learned = dol::slice(&dol, 0x80202dc8 + (character - 1) * 41, 41).unwrap();
        assert!(learned[1..=usize::from(learned[0])].contains(&(menu as u8)));
    }
}
