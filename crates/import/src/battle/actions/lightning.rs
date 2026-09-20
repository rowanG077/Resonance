//! Recover the four lightning callbacks, their distinct rule rows and retained target data.
use super::*;
use resonance_content::battle::{actions::lightning::*, effects::EffectId};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    kind: LightningKind,
    lifetime: u16,
    origin: GroundSpellOrigin,
    pulses: Vec<stored_parameters::Pulse>,
    stored: Option<Stored>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    presentation: StoredSpellPresentation,
    scale: f32,
    tracking: Option<SpellTracking>,
}

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    definition: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    let native = definition.native_id as u16;
    let flags = match native {
        216 => 0x00440186,
        217 | 219 => 0x0044018b,
        218 => 0x00440193,
        _ => bail!("unsupported lightning native {native}"),
    };
    ensure!(
        definition.flags == flags,
        "unexpected lightning technique flags"
    );
    let p = &tables.elemental.lightning[usize::from(native - 216)];
    ensure!(
        p.kind.native() == native,
        "wrong cooked lightning controller"
    );
    let release = if p.stored.is_some() {
        recovery::Release::Stored
    } else {
        recovery::Release::Ordinary
    };
    let casters = recovery::casters(catalogue, tables, technique, definition, release)?;
    let bundle = tables.bundle(native)?;
    ensure!(
        bundle.phase(0)?.duration == if p.stored.is_some() { 0 } else { p.lifetime }
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0),
        "unexpected lightning action phases"
    );
    let recipe = LightningRecipe {
        kind: p.kind,
        lifetime: p.lifetime,
        origin: p.origin,
        pulses: p
            .pulses
            .iter()
            .map(|pulse| {
                Ok(LightningPulse {
                    tick: pulse.tick,
                    projectile: EffectId {
                        bank: p.kind.bank(),
                        id: pulse.projectile,
                    },
                    rule: bundle.rule(pulse.rule)?,
                })
            })
            .collect::<Result<_>>()?,
        stored: p
            .stored
            .as_ref()
            .map(|stored| -> Result<_> {
                Ok(StoredLightning {
                    resume: stored_resume::shared(tables, &casters)?,
                    presentation: stored.presentation,
                    effect: EffectId {
                        bank: p.kind.bank(),
                        id: 1,
                    },
                    scale: stored.scale,
                    tracking: stored.tracking,
                })
            })
            .transpose()?,
    };
    recipe.validate()?;
    Ok(TechniqueProgram::Lightning { casters, recipe })
}

pub(super) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (kind, initializer, callback, lifetime, settings, pulses): (
        _,
        _,
        _,
        _,
        _,
        &[(u16, u8, usize)],
    ) = match native {
        216 => (
            LightningKind::Lightning,
            0x649b8,
            0x64950,
            90,
            None,
            &[(20, 4, 0)],
        ),
        217 => (
            LightningKind::SparkWave,
            0x7bef0,
            0x7bdfc,
            180,
            Some(0x59a8),
            &[(30, 1, 0)],
        ),
        218 => (
            LightningKind::Indignation,
            0x7e8b4,
            0x7e84c,
            290,
            Some(0x6210),
            &[(170, 0, 0)],
        ),
        219 => (
            LightningKind::ThunderBlade,
            0x72fdc,
            0x72f04,
            170,
            Some(0x49e8),
            &[(10, 0, 0), (80, 2, 1)],
        ),
        _ => bail!("unsupported lightning native {native}"),
    };
    let dispatch = rel.pointer(DATA, 0x1238 + usize::from(native - 200) * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, initializer)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37e48)
            && rel.local_targets().contains(&(1, callback)),
        "unexpected lightning native dispatch"
    );
    if settings.is_some() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + 8)? == (1, 0x37dd8),
            "unexpected lightning cleanup"
        );
    }
    let instructions: &[(usize, u32)] = match kind {
        LightningKind::Lightning => &[
            (0x64968, 0x2c000014),
            (0x64980, 0x38600001),
            (0x64988, 0x38e00004),
            (0x649cc, 0x4bfd3505),
        ],
        LightningKind::SparkWave => &[
            (0x7be48, 0x2c00001e),
            (0x7be5c, 0x38e00001),
            (0x7be64, 0x38600002),
            (0x7be7c, 0x907e00d8),
            (0x7be88, 0x2c00006e),
            (0x7be94, 0x386518c0),
            (0x7be98, 0x389e0020),
            (0x7beb8, 0x387f0018),
            (0x7bec8, 0x387e0020),
            (0x7bf40, 0x38c000b4),
            (0x7bf70, 0xd01f0024),
            (0x7bf7c, 0x391f0020),
            (0x7bf84, 0x38a00001),
        ],
        LightningKind::Indignation => &[
            (0x7e864, 0x2c0000aa),
            (0x7e880, 0x38600002),
            (0x7e888, 0x38e00000),
            (0x7e904, 0x38c00122),
            (0x7e940, 0x391f0020),
            (0x7e944, 0x38a00001),
        ],
        LightningKind::ThunderBlade => &[
            (0x72f30, 0x2c00000a),
            (0x72f5c, 0x38600002),
            (0x72f60, 0x38e00000),
            (0x72f7c, 0x2c000050),
            (0x72f98, 0x391f001c),
            (0x72fa0, 0x38600002),
            (0x72fa4, 0x38e00002),
            (0x7302c, 0x38c000aa),
            (0x73068, 0x391f0020),
            (0x7306c, 0x38a00001),
        ],
    };
    for &(offset, expected) in instructions {
        ensure!(
            word(rel.at((1, offset))?, 0)? == expected,
            "unexpected lightning callback at {offset:#x}"
        );
    }
    let pulses = pulses
        .iter()
        .map(|&(tick, projectile, rule)| stored_parameters::Pulse {
            tick,
            projectile,
            rule,
        })
        .collect();
    let mut origin = GroundSpellOrigin {
        height: float(rel.at((4, 0x1c80))?, 0)?,
        nudge: 1.,
        direction_threshold: float(rel.at((4, 0x2800))?, 0)?,
    };
    ensure!(
        rel.at((4, 0x1c4c))?[..12].iter().all(|&b| b == 0),
        "unexpected ground spell fallback direction"
    );
    let stored = settings
        .map(|settings| -> Result<_> {
            let settings = rel.at((4, settings))?;
            let tracking = if kind == LightningKind::SparkWave {
                ensure!(
                    settings[4..16].iter().all(|&b| b == 0),
                    "unexpected Spark Wave fallback direction"
                );
                origin.height = float(settings, 28)?;
                Some(SpellTracking {
                    first_tick: 31,
                    end_tick: 110,
                    speed: float(settings, 16)?,
                })
            } else {
                None
            };
            let presentation_offset = if tracking.is_some() { 20 } else { 4 };
            Ok(Stored {
                presentation: StoredSpellPresentation {
                    color: settings[..4].try_into()?,
                    camera_distance: float(settings, presentation_offset)?,
                    camera_elevation: float(settings, presentation_offset + 4)?,
                },
                scale: float(settings, if tracking.is_some() { 32 } else { 12 })?,
                tracking,
            })
        })
        .transpose()?;
    Ok(Parameters {
        kind,
        lifetime,
        origin,
        pulses,
        stored,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::effect_program::{MagicArchive, magic_member};
    use resonance_content::battle::effects::EffectBank;
    #[test]
    #[ignore = "requires privately extracted original action records; no asset encoding"]
    fn original_lightning_family_keeps_each_callback_rule_and_caster() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let catalogue =
            crate::arte::read(&fs::read(extracted.join("sys/main.dol")).unwrap()).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let tables = Tables::original(
            &extracted,
            &rel,
            &usual,
            &[
                216, 217, 218, 219, 223, 224, 226, 228, 229, 230, 231, 232, 233, 251, 252, 253,
                278, 283,
            ],
        )
        .unwrap();
        for (menu, native, duration, expected, characters) in [
            (78, 216, 90, vec![(20, 4, 130)], vec![3]),
            (219, 216, 90, vec![(20, 4, 130)], vec![6, 9]),
            (79, 217, 180, vec![(30, 1, 85)], vec![3]),
            (80, 218, 290, vec![(170, 0, 1000)], vec![3]),
            (81, 219, 170, vec![(10, 0, 145), (80, 2, 125)], vec![3]),
            (220, 219, 170, vec![(10, 0, 145), (80, 2, 125)], vec![6, 9]),
        ] {
            let definition = catalogue.definition(usize::from(menu)).unwrap();
            let TechniqueProgram::Lightning { casters, recipe } =
                cook(&catalogue, &tables, menu, definition).unwrap()
            else {
                unreachable!()
            };
            assert_eq!((recipe.kind.native(), recipe.lifetime), (native, duration));
            assert_eq!(
                casters.iter().map(|c| c.character).collect::<Vec<_>>(),
                characters
            );
            assert_eq!(
                recipe
                    .pulses
                    .iter()
                    .map(|p| (p.tick, p.projectile.id, p.rule.power))
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(recipe.origin.height, if native == 217 { 200. } else { 0. });
            if native == 217 {
                let track = recipe.stored.unwrap().tracking.unwrap();
                assert_eq!(
                    (track.first_tick, track.end_tick, track.speed),
                    (31, 110, 1.25)
                );
            }
        }
        for (menu, native, lifetime, tick, tp, begin, release, power, stun, cooldown, flags) in [
            (85, 223, 220, 20, 38, 33111, 33086, 125, 35, 30, 0x4020),
            (86, 224, 180, 35, 34, 33016, 33087, 60, 45, 10, 0x20),
            (90, 228, 225, 45, 42, 33016, 33090, 95, 40, 10, 0x20),
            (91, 229, 170, 30, 42, 33016, 33091, 78, 45, 10, 0x20),
        ] {
            let definition = catalogue.definition(usize::from(menu)).unwrap();
            let TechniqueProgram::GroundPulse {
                casters, recipe, ..
            } = super::super::ground_pulse::cook(&catalogue, &tables, menu, definition).unwrap()
            else {
                panic!("ground pulse stored controller")
            };
            assert_eq!((recipe.kind as u16, recipe.kind.menu()), (native, menu));
            assert_eq!((recipe.lifetime, recipe.pulse_tick), (lifetime, tick));
            assert_eq!(recipe.effect_scale, 1.);
            assert_eq!(
                (
                    recipe.origin.height,
                    recipe.origin.nudge,
                    recipe.origin.direction_threshold
                ),
                if native == 229 {
                    (192., 0., 0.5)
                } else {
                    (0., 1., 0.5)
                }
            );
            assert_eq!(
                (
                    recipe.rule.power,
                    recipe.rule.hitstun,
                    recipe.rule.contact_cooldown,
                    recipe.rule.flags
                ),
                (power, stun, cooldown, flags)
            );
            assert_eq!(
                (recipe.rule.conditions, recipe.rule.condition_chance),
                match native {
                    224 => (0x200, 1),
                    228 => (0x200, 2),
                    _ => (0, 0),
                }
            );
            assert_eq!(
                recipe.rule.element,
                if native == 228 {
                    HitElement::Neutral
                } else if native == 229 {
                    HitElement::Element(resonance_content::menu_data::Element::Wind)
                } else if native == 224 {
                    HitElement::Element(resonance_content::menu_data::Element::Earth)
                } else {
                    HitElement::Element(resonance_content::menu_data::Element::Fire)
                }
            );
            assert_eq!(casters.len(), 1);
            assert_eq!(
                (
                    casters[0].character,
                    casters[0].tp,
                    casters[0].voices.begin,
                    casters[0].voices.release
                ),
                (3, tp, begin, release)
            );
            assert!(
                super::super::ground_pulse::cook(&catalogue, &tables, menu + 1, definition)
                    .is_err()
            );
            let archive = MagicArchive::read(&extracted).unwrap();
            let source = magic_member(archive.package(native - 200).unwrap(), 252)
                .unwrap()
                .unwrap();
            let effect = recipe.kind.effect(1);
            let flight = crate::battle::effects::projectile(&source[400..800], effect, 1.).unwrap();
            assert_eq!(
                flight.lifetime,
                match native {
                    223 => 160,
                    224 => 80,
                    229 => 90,
                    _ => 100,
                }
            );
            assert_eq!(flight.birth_bank, Some(effect.bank));
            assert!(flight.spawn_effect.is_none() && flight.trail_effect.is_none());
            assert!(flight.persist_after_hit);
            assert!(
                matches!(flight.movement, resonance_content::battle::effects::ProjectileMovement::Ballistic { velocity, acceleration, steering: None } if velocity == [0.; 3] && acceleration == [0.; 3])
            );
        }
        use resonance_content::menu_data::Element;
        for (menu, duration, tp, begin, release, expected_rules) in [
            (
                92,
                210,
                46,
                33109,
                33110,
                vec![
                    (0x2020, HitElement::Element(Element::Ice), 300, 90, 30, 60),
                    (0x2022, HitElement::Element(Element::Ice), 480, 50, 30, 0),
                ],
            ),
            (
                93,
                265,
                44,
                33016,
                33092,
                vec![
                    (0x20, HitElement::Element(Element::Lightning), 60, 35, 8, 0),
                    (0x22, HitElement::Element(Element::Earth), 150, 75, 30, 0),
                    (0x22, HitElement::Element(Element::Earth), 150, 75, 30, 0),
                ],
            ),
            (
                95,
                330,
                80,
                33129,
                33094,
                vec![(0x20, HitElement::Neutral, 425, 35, 30, 0)],
            ),
        ] {
            let definition = catalogue.definition(usize::from(menu)).unwrap();
            let program =
                super::super::genis_final::cook(&catalogue, &tables, menu, definition).unwrap();
            assert_eq!(program.stored().unwrap().lifetime, duration);
            let casters = match &program {
                TechniqueProgram::Absolute {
                    casters, recipe, ..
                } => {
                    assert_eq!(
                        (recipe.select_tick, recipe.second_tick, recipe.radius),
                        (62, 120, 300.)
                    );
                    assert_eq!((recipe.origin.height, recipe.origin.nudge), (0., 1.));
                    casters
                }
                TechniqueProgram::EarthBite {
                    casters, recipe, ..
                } => {
                    assert_eq!(
                        (
                            recipe.origin.height,
                            recipe.origin.nudge,
                            recipe.second_height
                        ),
                        (125., 1., 0.)
                    );
                    assert_eq!(
                        recipe.pulses.map(|p| (p.tick, p.projectile.id)),
                        [(30, 1), (100, 2), (110, 2)]
                    );
                    casters
                }
                TechniqueProgram::MeteorStorm {
                    casters, recipe, ..
                } => {
                    assert_eq!(recipe.heading_offset, 45f32.to_radians());
                    assert_eq!(
                        recipe.bursts.map(|b| b.tick),
                        std::array::from_fn::<_, 14, _>(|i| (i as u16 + 1) * 15)
                    );
                    assert_eq!(
                        recipe.bursts.map(|b| b.offset),
                        [
                            [0., 0., 0.],
                            [300., 0., 600.],
                            [-300., 0., -600.],
                            [300., 0., 0.],
                            [-600., 0., 300.],
                            [300., 0., -600.],
                            [-300., 0., 0.],
                            [600., 0., 300.],
                            [-600., 0., -300.],
                            [600., 0., -300.],
                            [0., 0., 300.],
                            [0., 0., -300.],
                            [-300., 0., 600.],
                            [0., 0., 0.],
                        ]
                    );
                    casters
                }
                _ => panic!("advanced Genis stored controller"),
            };
            assert_eq!(casters.len(), 1);
            assert_eq!(
                (
                    casters[0].character,
                    casters[0].tp,
                    casters[0].voices.begin,
                    casters[0].voices.release
                ),
                (3, tp, begin, release)
            );
            assert_eq!(
                program
                    .spell_rules()
                    .map(|(bank, r)| {
                        assert_eq!(bank, EffectBank::Magic(menu + 138 - 200));
                        assert_eq!(
                            (r.conditions, r.condition_chance, r.condition_parameter),
                            (0, 0, 0)
                        );
                        (
                            r.flags,
                            r.element,
                            r.power,
                            r.hitstun,
                            r.contact_cooldown,
                            r.knockback_delay,
                        )
                    })
                    .collect::<Vec<_>>(),
                expected_rules
            );
            assert!(
                super::super::genis_final::cook(&catalogue, &tables, menu + 1, definition).is_err()
            );
            let archive = MagicArchive::read(&extracted).unwrap();
            let source = magic_member(archive.package(menu - 62).unwrap(), 252)
                .unwrap()
                .unwrap();
            for (id, lifetime) in match menu {
                92 => &[(1, 10), (2, 10)][..],
                93 => &[(1, 75), (2, 5)][..],
                _ => &[(1, 40)][..],
            } {
                let effect = EffectId {
                    bank: EffectBank::Magic(menu - 62),
                    id: *id,
                };
                let start = usize::from(*id) * 400;
                let flight =
                    crate::battle::effects::projectile(&source[start..start + 400], effect, 1.)
                        .unwrap();
                assert_eq!(flight.lifetime, *lifetime);
                assert_eq!(flight.birth_bank, Some(effect.bank));
                assert_eq!(
                    flight.spawn_effect,
                    (menu == 95).then_some(EffectId { id: 2, ..effect })
                );
                assert!(flight.trail_effect.is_none());
            }
        }
        let definition = catalogue.definition(88).unwrap();
        let TechniqueProgram::SpiralFlare {
            casters, recipe, ..
        } = super::super::spiral_flare::cook(&catalogue, &tables, 88, definition).unwrap()
        else {
            panic!("Spiral Flare stored controller")
        };
        assert_eq!((recipe.lifetime, recipe.pulse_tick), (160, 30));
        assert_eq!(
            (recipe.forward_distance, recipe.height, recipe.effect_scale),
            (80., 100., 1.)
        );
        assert_eq!(recipe.presentation.color, [24, 16, 16, 255]);
        assert_eq!(
            (
                recipe.presentation.camera_distance,
                recipe.presentation.camera_elevation
            ),
            (2400., 8.)
        );
        assert_eq!(
            (
                recipe.rule.flags,
                recipe.rule.power,
                recipe.rule.hitstun,
                recipe.rule.contact_cooldown,
                recipe.rule.knockback_delay
            ),
            (0x28, 120, 35, 2, 2)
        );
        assert_eq!(
            (
                recipe.rule.conditions,
                recipe.rule.condition_chance,
                recipe.rule.condition_parameter
            ),
            (0, 0, 0)
        );
        assert_eq!(
            recipe.rule.element,
            HitElement::Element(resonance_content::menu_data::Element::Fire)
        );
        assert_eq!(casters.len(), 1);
        assert_eq!(
            (
                casters[0].character,
                casters[0].tp,
                casters[0].voices.begin,
                casters[0].voices.release
            ),
            (3, 38, 33016, 33088)
        );
        assert!(super::super::spiral_flare::cook(&catalogue, &tables, 89, definition).is_err());
        let archive = MagicArchive::read(&extracted).unwrap();
        let source = magic_member(archive.package(26).unwrap(), 252)
            .unwrap()
            .unwrap();
        let effect = resonance_content::battle::actions::spiral_flare::SpiralFlareRecipe::effect;
        let flight = crate::battle::effects::projectile(&source[400..800], effect(1), 1.).unwrap();
        assert_eq!((flight.lifetime, flight.trail_interval), (90, 3));
        assert_eq!(flight.shape.kind, HitShapeKind::Box);
        assert_eq!((flight.shape.radius, flight.shape.height), (175., 175.));
        assert_eq!(flight.birth_bank, Some(effect(1).bank));
        assert_eq!(
            (flight.spawn_effect, flight.trail_effect),
            (Some(effect(2)), Some(effect(3)))
        );
        assert!(flight.persist_after_hit);
        assert!(
            matches!(flight.movement, resonance_content::battle::effects::ProjectileMovement::Ballistic {velocity, acceleration, steering: None} if velocity == [0., 0., 35.] && acceleration == [0.; 3])
        );
        let definition = catalogue.definition(94).unwrap();
        let TechniqueProgram::PrismSword {
            casters, recipe, ..
        } = crate::battle::actions::prism::cook(&catalogue, &tables, 94, definition).unwrap()
        else {
            panic!("Prism Sword stored controller")
        };
        assert_eq!(recipe.lifetime, 210);
        assert_eq!(recipe.heading_offset, 20f32.to_radians());
        assert_eq!(recipe.bursts.map(|b| b.tick), [30, 38, 46, 54, 62, 70, 94]);
        assert_eq!(
            recipe.bursts.map(|b| b.offset),
            [
                [-207., 0., -445.],
                [408., 0., -204.],
                [69., 0., 328.],
                [-300., 0., 62.],
                [37., 0., -234.],
                [188., 0., 43.],
                [-30., 0., -26.],
            ]
        );
        assert_eq!(recipe.presentation.color, [12, 12, 12, 255]);
        assert_eq!(
            (
                recipe.presentation.camera_distance,
                recipe.presentation.camera_elevation
            ),
            (2950., 18.)
        );
        assert_eq!(
            recipe
                .rules
                .map(|r| (r.power, r.hitstun, r.contact_cooldown, r.flags)),
            [(300, 40, 30, 0x20), (600, 40, 30, 0x28)]
        );
        assert_eq!(casters.len(), 1);
        assert_eq!((casters[0].character, casters[0].tp), (3, 58));
        assert_eq!(
            (casters[0].voices.begin, casters[0].voices.release),
            (33124, 33093)
        );
        let archive = MagicArchive::read(&extracted).unwrap();
        let bytes = magic_member(archive.package(32).unwrap(), 252)
            .unwrap()
            .unwrap();
        for id in [1u8, 2] {
            let effect = resonance_content::battle::actions::prism::PrismRecipe::effect(id);
            let at = usize::from(id) * 400;
            let flight =
                crate::battle::effects::projectile(&bytes[at..at + 400], effect, 1.).unwrap();
            assert_eq!(flight.lifetime, 8);
            assert_eq!(flight.birth_bank, Some(effect.bank));
            assert!(flight.spawn_effect.is_none() && flight.trail_effect.is_none());
            let resonance_content::battle::effects::ProjectileMovement::Ballistic {
                velocity,
                acceleration,
                ..
            } = flight.movement
            else {
                panic!("Prism Sword contacts must remain stationary")
            };
            assert_eq!((velocity, acceleration), ([0.; 3], [0.; 3]));
        }
        // Ray keeps its nine rotated contact offsets separate from the root's retained heading.
        let definition = catalogue.definition(114).unwrap();
        let TechniqueProgram::Ray {
            casters, recipe, ..
        } = crate::battle::actions::ray::cook(&catalogue, &tables, 114, definition).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(recipe.lifetime, 230);
        assert_eq!(recipe.heading_offset, 20f32.to_radians());
        assert_eq!(
            recipe.bursts.map(|b| b.tick),
            [60, 68, 76, 84, 92, 100, 108, 116, 124]
        );
        assert_eq!(
            recipe.bursts.map(|b| b.offset),
            [
                [-300., 0., 0.],
                [400., 0., 400.],
                [0., 0., -300.],
                [-400., 0., 400.],
                [300., 0., 0.],
                [-400., 0., -400.],
                [0., 0., 300.],
                [400., 0., -400.],
                [0.; 3],
            ]
        );
        assert_eq!(recipe.presentation.color, [12, 12, 12, 255]);
        assert_eq!(
            (
                recipe.presentation.camera_distance,
                recipe.presentation.camera_elevation
            ),
            (3550., 11.5)
        );
        assert_eq!(
            (recipe.rule.power, recipe.rule.hitstun, recipe.rule.flags),
            (350, 45, 0x20)
        );
        assert_eq!(casters.len(), 1);
        assert_eq!((casters[0].character, casters[0].tp), (4, 35));
        assert_eq!(
            (casters[0].voices.begin, casters[0].voices.release),
            (33137, 33206)
        );
        let archive = MagicArchive::read(&extracted).unwrap();
        let bytes = magic_member(archive.package(52).unwrap(), 252)
            .unwrap()
            .unwrap();
        let effect = resonance_content::battle::actions::ray::RayRecipe::effect(1);
        let projectile = crate::battle::effects::projectile(&bytes[400..800], effect, 1.).unwrap();
        assert_eq!(projectile.lifetime, 8);
        assert_eq!(projectile.birth_bank, Some(effect.bank));
        assert!(projectile.spawn_effect.is_none() && projectile.trail_effect.is_none());
        let resonance_content::battle::effects::ProjectileMovement::Ballistic {
            velocity,
            acceleration,
            ..
        } = projectile.movement
        else {
            panic!("Ray contact must remain stationary")
        };
        assert_eq!((velocity, acceleration), ([0.; 3], [0.; 3]));
        // Shared lances retain their own elements and dynamic projectile-born model banks.
        for (menu, native, color) in [(115, 253, [12, 12, 12, 255]), (250, 283, [48, 48, 48, 255])]
        {
            let definition = catalogue.definition(usize::from(menu)).unwrap();
            let program =
                crate::battle::actions::lance::cook(&catalogue, &tables, menu, definition).unwrap();
            let settings = program.stored().unwrap();
            let TechniqueProgram::Lance {
                casters,
                resume,
                recipe,
            } = program
            else {
                unreachable!()
            };
            assert_eq!((recipe.kind as u16, recipe.lifetime), (native, 210));
            assert_eq!(recipe.presentation.color, color);
            assert_eq!(
                recipe.ring.map(|r| (r.marker_tick, r.projectile_tick)),
                [(0, 35), (10, 45), (20, 55), (30, 65)]
            );
            assert_eq!((recipe.final_tick, recipe.afterglow_tick), (85, 95));
            assert_eq!(recipe.rules.map(|r| r.power), [125, 300]);
            assert_eq!(recipe.rules[1].knockback_delay, 8);
            assert_eq!(
                recipe.origin.capture([10., 20., 0.], [0., 30., 0.]),
                [1., 0., 0.]
            );
            if native == 253 {
                assert_eq!(casters.len(), 1);
                assert_eq!((casters[0].character, casters[0].tp), (4, 40));
                assert_eq!(
                    (casters[0].voices.begin, casters[0].voices.release),
                    (33137, 33207)
                );
                assert!(resume.is_some() && settings.party_resume.is_some());
            } else {
                assert!(casters.is_empty() && resume.is_none() && settings.party_resume.is_none());
            }
            let archive = MagicArchive::read(&extracted).unwrap();
            let bytes = magic_member(archive.package(native - 200).unwrap(), 252)
                .unwrap()
                .unwrap();
            for id in 1..=2 {
                let offset = usize::from(id) * 400;
                let projectile = crate::battle::effects::projectile(
                    &bytes[offset..offset + 400],
                    recipe.kind.effect(id),
                    1.,
                )
                .unwrap();
                assert_eq!(projectile.birth_bank, Some(recipe.kind.effect(1).bank));
                assert_eq!(projectile.spawn_effect, Some(recipe.kind.effect(id + 3)));
                assert_eq!(
                    projectile.trail_effect,
                    (id == 2).then_some(recipe.kind.effect(6))
                );
            }
        }
        // Target-following orbs share the stored lifecycle but retain their two rule rows.
        for (menu, native, color, powers, first_stun) in [
            (113, 251, [16, 16, 16, 255], [150, 350], 120),
            (246, 278, [48, 48, 48, 255], [150, 250], 90),
        ] {
            let definition = catalogue.definition(usize::from(menu)).unwrap();
            let TechniqueProgram::Orb {
                casters, recipe, ..
            } = crate::battle::actions::orb::cook(&catalogue, &tables, menu, definition).unwrap()
            else {
                unreachable!()
            };
            assert_eq!((recipe.kind as u16, recipe.lifetime), (native, 165));
            assert_eq!(recipe.presentation.color, color);
            assert_eq!(
                (
                    recipe.presentation.camera_distance,
                    recipe.presentation.camera_elevation
                ),
                (2250., 8.)
            );
            assert_eq!(recipe.effect_scale, 1.);
            assert_eq!(recipe.pulses.map(|pulse| pulse.tick), [10, 60]);
            assert_eq!(recipe.pulses.map(|pulse| pulse.rule.power), powers);
            assert_eq!(
                recipe.pulses.map(|pulse| pulse.rule.hitstun),
                [first_stun, 50]
            );
            assert_eq!(
                recipe.pulses.map(|pulse| pulse.rule.flags),
                [0x2020, 0x2022]
            );
            if native == 251 {
                assert_eq!(casters.len(), 1);
                assert_eq!((casters[0].character, casters[0].tp), (4, 16));
                assert_eq!(
                    (casters[0].voices.begin, casters[0].voices.release),
                    (33224, 33205)
                );
            } else {
                assert!(
                    casters.is_empty(),
                    "the event-only party chant remains explicitly unbound"
                );
            }
            let archive = MagicArchive::read(&extracted).unwrap();
            let bytes = magic_member(archive.package(native - 200).unwrap(), 252)
                .unwrap()
                .unwrap();
            for id in [1u8, 2] {
                let start = usize::from(id) * 400;
                let flight = crate::battle::effects::projectile(
                    &bytes[start..start + 400],
                    recipe.kind.effect(id),
                    0.5,
                )
                .unwrap();
                assert_eq!(flight.id, Some(recipe.kind.effect(id)));
                assert_eq!(flight.birth_bank, Some(recipe.kind.effect(id).bank));
            }
        }
        let callback = rel.sections[1].0 + 0x72f98;
        rel.bytes[callback + 3] = 0;
        assert!(
            read_parameters(&rel, 219).is_err(),
            "changed second-rule selection must not reuse Thunder Blade's recipe"
        );
    }
}
