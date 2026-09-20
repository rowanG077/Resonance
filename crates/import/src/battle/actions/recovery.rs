//! Targeted healing recipes and their caster-specific animation and voice bindings.
use super::*;
use resonance_content::battle::effects::{EffectBank, EffectId};

#[derive(Clone, Copy)]
pub(super) enum Release {
    Ordinary,
    Stored,
    Summon,
}

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    row: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    let native = row.native_id as u16;
    let flags = match native {
        236 => 0x00880286,
        238 => 0x0088028b,
        257 => 0x00880293,
        _ => bail!("unsupported targeted recovery spell {native}"),
    };
    ensure!(row.flags == flags, "unexpected recovery spell flags");
    if native == 236 {
        let source = tables.bundle(native)?;
        ensure!(
            source.phase(0)?.duration == 90
                && source.phases[1..].iter().all(|phase| phase.duration == 0),
            "unexpected recovery runner lifetime"
        );
        return Ok(TechniqueProgram::RecoverySpell {
            casters: casters(catalogue, tables, technique, row, Release::Ordinary)?,
            lifetime: source.phase(0)?.duration,
            pulses: vec![tables.recovery.first_aid],
        });
    }

    // Stored spells apply recovery in their initializer, after the shared prelude.
    // Their lifetime is an initializer argument, not a BTLusual phase duration.
    let parameters = if native == 238 {
        &tables.recovery.heal
    } else {
        &tables.recovery.cure
    };
    let casters = casters(catalogue, tables, technique, row, Release::Stored)?;
    ensure!(
        casters.len() == 1 && casters[0].character == 4,
        "unsupported stored recovery caster bindings"
    );
    let casting = &tables.actor(4)?.casting;
    ensure!(
        casting.resume_loop_start == 0,
        "unsupported stored recovery resume mode"
    );
    Ok(TechniqueProgram::StoredRecoverySpell {
        casters,
        resume: AnimationCommand::Play {
            clip: 12,
            blend: casting.resume_blend_ticks,
            start: 0,
            end: None,
            layer: 8,
            looping: casting.release_looping,
            mirror: false,
            resource: -1,
            rate: casting.animation_rate,
        },
        lifetime: parameters.lifetime,
        percent: parameters.percent,
        effect: EffectId {
            bank: EffectBank::Magic(native - 200),
            id: 1,
        },
        presentation: parameters.presentation,
    })
}

pub(super) fn casters(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    row: &crate::arte::Definition,
    release: Release,
) -> Result<Vec<CastRecipe>> {
    let native = row.native_id as u16;
    let (commands, loop_commands) = tables.programs.commands()?;
    let mut casters = Vec::new();
    for character in 1..=9u8 {
        let learned = catalogue.learned_by(character)?;
        if !learned.iter().any(|&id| u16::from(id) == technique) {
            continue;
        }
        let actor = tables.actor(character)?;
        let casting = &actor.casting;
        ensure!(
            casting.loop_start == 0,
            "unsupported recovery casting animation mode"
        );
        let voices = tables.voices.selected(character, native)?;
        let self_voices = tables.voices.self_voices(character, voices)?;
        let rate = casting.animation_rate;
        let animation = |clip, blend, looping, rate| AnimationCommand::Play {
            clip,
            blend,
            start: 0,
            end: None,
            layer: 8,
            looping,
            mirror: false,
            resource: -1,
            rate,
        };
        let release = match release {
            Release::Ordinary => Some(animation(12, 4, casting.release_looping, rate)),
            Release::Stored => Some(animation(
                13,
                4,
                casting.stored_release_looping,
                tables.recovery.stored_release_rate,
            )),
            Release::Summon => {
                ensure!(
                    character == 5 && matches!(native, 284..=293),
                    "unsupported absent summon release motion"
                );
                None
            }
        };
        casters.push(CastRecipe {
            character,
            tp: row.tp_cost,
            time_adjustment: row.cast_time_adjustment,
            voices,
            self_voices,
            animations: if character == 3 {
                tables.programs.animation()?
            } else {
                vec![AnimationStep {
                    trigger: AnimationTrigger::Tick(0),
                    command: animation(11, 8, casting.chant_looping, rate),
                }]
                .into()
            },
            commands: commands.clone(),
            loop_commands,
            pulse: if row.flags & 0x00400000 != 0 {
                3
            } else if row.flags & 0x00800000 != 0 {
                4
            } else {
                5
            },
            release,
            recovery_pose: tables.recovery.recovery_pose.animation(actor)?,
            release_effect: if row.flags & 0x00800000 != 0 { 8 } else { 7 },
        });
    }
    ensure!(!casters.is_empty(), "recovery spell has no authored caster");
    Ok(casters)
}

#[test]
#[ignore = "requires original extracted party metadata and summon records; no asset encoding"]
fn original_party_chant_modes_keep_sheenas_summon_animation_non_looping() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
    let catalogue = crate::arte::read(&executable).unwrap();
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    let tables =
        Tables::original(&extracted, &rel, &usual, &(284..=293).collect::<Vec<_>>()).unwrap();
    for character in 1..=9 {
        let casting = &tables.actor(character).unwrap().casting;
        assert_eq!(casting.loop_start, 0);
        assert_eq!(!casting.chant_looping, character == 5);
    }
    for (technique, release) in [
        (62, Release::Ordinary),
        (63, Release::Stored),
        (70, Release::Ordinary),
        (71, Release::Stored),
    ] {
        let row = catalogue.definition(usize::from(technique)).unwrap();
        let recipes = casters(&catalogue, &tables, technique, row, release).unwrap();
        let genis = recipes.iter().find(|recipe| recipe.character == 3).unwrap();
        let expected = [(30, 8, false), (31, 2, false), (32, 2, true)];
        assert_eq!(genis.animations.commands().count(), expected.len());
        for (command, (clip, blend, looping)) in genis.animations.commands().zip(expected) {
            assert!(matches!(command, AnimationCommand::Play {
                clip: value, blend: frames, start: 0, end: None, layer: 8,
                looping: repeats, mirror: false, resource: -1, rate: 0.5,
            } if (value, frames, repeats) == (clip, blend, looping)));
        }
        assert!(
            matches!(&genis.animations.instructions[&1], AnimationInstruction::Step(step) if matches!(step.trigger, AnimationTrigger::Tick(52)))
        );
        assert!(
            matches!(&genis.animations.instructions[&2], AnimationInstruction::Step(step) if matches!(step.trigger, AnimationTrigger::Tick(80)))
        );
    }
    let archive =
        crate::battle::visual::party::archive(&executable, &extracted.join("files"), 5, 0).unwrap();
    assert!(archive.sections[13].is_some() && archive.sections[14].is_some());
    assert!(
        archive.sections[15].is_none(),
        "Sheena has no release clip 13"
    );
    for technique in [235, 236, 237, 238, 239, 240, 241, 242, 243, 244] {
        let row = catalogue.definition(usize::from(technique)).unwrap();
        let recipes = casters(&catalogue, &tables, technique, row, Release::Summon).unwrap();
        assert_eq!(recipes.len(), 1);
        let cast = &recipes[0];
        assert_eq!((cast.character, cast.tp), (5, 100));
        assert!(cast.release.is_none());
        assert!(matches!(
            cast.animations.initial.unwrap(),
            AnimationCommand::Play {
                clip: 11,
                blend: 8,
                looping: false,
                rate: 0.5,
                ..
            }
        ));
        use resonance_content::battle::actions::ground_summon::{
            GroundSummonKind as Kind, GroundSummonOrigin, SummonBlessing,
        };
        let (kind, voices, timing, color, camera, blessing, expected) = match technique {
            235 => (
                Kind::Efreet,
                (33322, 33323),
                (315, 80, Some(34115)),
                [24, 16, 16, 255],
                (3150., 15.5, 2150.),
                SummonBlessing::Attack(15),
                &[(128, 2, 1100, 60, 90, 0, 32)][..],
            ),
            236 => (
                Kind::Undine,
                (33318, 33319),
                (385, 90, Some(34059)),
                [16, 16, 24, 255],
                (3350., 15.5, 1850.),
                SummonBlessing::Heal(50),
                &[
                    (115, 1, 400, 45, 5, 0, 30),
                    (160, 1, 400, 45, 5, 0, 30),
                    (205, 1, 400, 45, 5, 0, 30),
                    (250, 1, 400, 45, 5, 0, 30),
                ][..],
            ),
            237 => (
                Kind::Sylph,
                (33320, 33321),
                (375, 75, Some(34076)),
                [16, 24, 16, 255],
                (2950., 15.5, 2000.),
                SummonBlessing::Speed,
                &[
                    (103, 1, 200, 90, 8, 0, 40),
                    (166, 2, 200, 90, 8, 4, 120),
                    (170, 2, 200, 90, 8, 4, 120),
                    (174, 2, 200, 90, 8, 4, 120),
                    (178, 2, 200, 90, 8, 4, 120),
                    (182, 2, 200, 90, 8, 4, 120),
                    (235, 3, 600, 90, 100, 4, 30),
                ][..],
            ),
            240 => (
                Kind::Celsius,
                (33326, 33327),
                (310, 90, Some(34148)),
                [16, 16, 24, 255],
                (3150., 17., 2000.),
                SummonBlessing::Accuracy(15),
                &[(156, 1, 500, 45, 60, 0, 40), (176, 2, 500, 45, 60, 0, 40)][..],
            ),
            239 => (
                Kind::Gnome,
                (33330, 33331),
                (255, 50, Some(34132)),
                [20, 20, 16, 255],
                (3150., 15.5, 2450.),
                SummonBlessing::Defense(15),
                &[(110, 1, 600, 45, 5, 0, 5), (115, 2, 100, 45, 8, 2, 36)][..],
            ),
            241 => (
                Kind::Volt,
                (33328, 33329),
                (315, 0, None),
                [16, 16, 20, 255],
                (3200., 16.5, 2650.),
                SummonBlessing::PhysicalImmunity,
                &[(165, 1, 200, 45, 5, 0, 60)][..],
            ),
            242 => (
                Kind::Shadow,
                (33332, 33333),
                (330, 90, Some(34168)),
                [8, 8, 8, 255],
                (3150., 15.5, 1850.),
                SummonBlessing::MagicalImmunity,
                &[(120, 1, 150, 90, 10, 9, 100)][..],
            ),
            243 => (
                Kind::Origin,
                (33334, 33335),
                (315, 100, Some(34205)),
                [16, 16, 16, 255],
                (3150., 19.5, 2250.),
                SummonBlessing::AttackDefense(10),
                &[(120, 1, 250, 60, 8, 0, 110)][..],
            ),
            _ => continue,
        };
        let recipe = super::ground_summon::cook(&tables, technique, row).unwrap();
        assert_eq!(recipe.kind, kind);
        assert_eq!((cast.voices.begin, cast.voices.release), voices);
        assert_eq!((recipe.lifetime, recipe.voice_tick, recipe.voice), timing);
        assert_eq!(
            (
                recipe.effect_scale,
                recipe.blessing_radius,
                recipe.focus_ticks
            ),
            (
                1.,
                512.,
                if matches!(kind, Kind::Sylph | Kind::Celsius) {
                    150
                } else {
                    120
                }
            )
        );
        assert_eq!(recipe.blessing, blessing);
        assert_eq!(recipe.presentation.color, color);
        assert_eq!(
            (
                recipe.presentation.camera_distance,
                recipe.presentation.camera_elevation,
                recipe.focus_distance
            ),
            camera
        );
        match recipe.origin {
            GroundSummonOrigin::World => assert_eq!(kind, Kind::Celsius),
            GroundSummonOrigin::TargetDirection { height, distance } => {
                assert!(matches!(
                    kind,
                    Kind::Efreet | Kind::Origin | Kind::Shadow | Kind::Sylph
                ));
                assert_eq!(
                    (height, distance),
                    (
                        0.,
                        match kind {
                            Kind::Origin => -400.,
                            Kind::Shadow => -200.,
                            _ => -150.,
                        }
                    )
                );
            }
            GroundSummonOrigin::TargetGround(origin) => {
                assert!(matches!(kind, Kind::Gnome | Kind::Undine | Kind::Volt));
                assert_eq!(
                    (origin.height, origin.nudge, origin.direction_threshold),
                    (if kind == Kind::Volt { 300. } else { 0. }, 1., 0.5)
                );
            }
        }
        if matches!(kind, Kind::Sylph | Kind::Celsius) {
            assert_eq!(cast.time_adjustment, 180);
            if kind == Kind::Sylph {
                assert_eq!(
                    recipe.voices().collect::<Vec<_>>(),
                    [(75, 34076), (135, 34091), (210, 34099)]
                );
                assert!(recipe.lanes.is_none() && recipe.sound.is_none());
                let mut invalid = recipe.clone();
                invalid.pulses.last_mut().unwrap().rule.element =
                    HitElement::Element(resonance_content::menu_data::Element::Wind);
                assert!(invalid.validate().is_err());
            } else {
                let lanes = recipe.lanes.as_ref().unwrap();
                assert_eq!(lanes.visual_tick, 150);
                assert_eq!(
                    lanes.heading_offsets,
                    [45f32, 135., 225., 315.].map(f32::to_radians)
                );
                let sound = recipe.sound.unwrap();
                assert_eq!(
                    (sound.id, sound.first_tick, sound.interval, sound.count),
                    (126, 156, 8, 6)
                );
                let mut invalid = recipe.clone();
                invalid.lanes = None;
                assert!(invalid.validate().is_err());
            }
        }
        if kind == Kind::Undine {
            let waves = recipe.waves.as_ref().unwrap();
            assert_eq!(
                (waves.first_tick, waves.interval, waves.projectile_delay),
                (90, 45, 25)
            );
            assert_eq!(
                waves.offsets,
                [
                    [0., 0., 400.],
                    [-300., 0., -250.],
                    [300., 0., -250.],
                    [0.; 3]
                ]
            );
            let mut invalid = recipe.clone();
            invalid.waves.as_mut().unwrap().projectile_delay += 1;
            assert!(invalid.validate().is_err());
        } else {
            let old = serde_json::to_value(&recipe).unwrap();
            assert!(old.get("waves").is_none());
            if !matches!(kind, Kind::Sylph | Kind::Celsius) {
                for field in ["lanes", "sound", "extra_voices"] {
                    assert!(old.get(field).is_none());
                }
            }
            // Existing cooked voices remain plain numbers; Volt has no callback voice.
            assert_eq!(
                old.get("voice").and_then(serde_json::Value::as_u64),
                recipe.voice.map(u64::from)
            );
            let decoded: resonance_content::battle::actions::ground_summon::GroundSummonRecipe =
                serde_json::from_value(old).unwrap();
            assert!(decoded.waves.is_none());
            assert_eq!(decoded.voice, recipe.voice);
            decoded.validate().unwrap();
        }
        if matches!(kind, Kind::Volt | Kind::Shadow) {
            assert_eq!(
                cast.time_adjustment,
                if kind == Kind::Volt { 210 } else { 200 }
            );
            let mut invalid = recipe.clone();
            invalid.voice = if kind == Kind::Volt {
                Some(cast.voices.release)
            } else {
                None
            };
            assert!(invalid.validate().is_err());
        }
        let archive = crate::battle::effect_program::MagicArchive::read(&extracted).unwrap();
        let source = crate::battle::effect_program::magic_member(
            archive.package(kind.native() - 200).unwrap(),
            252,
        )
        .unwrap()
        .unwrap();
        assert_eq!(recipe.pulses.len(), expected.len());
        for (pulse, &(tick, id, power, stun, cooldown, delay, lifetime)) in
            recipe.pulses.iter().zip(expected)
        {
            assert_eq!(
                (
                    pulse.tick,
                    pulse.projectile.id,
                    pulse.rule.power,
                    pulse.rule.hitstun,
                    pulse.rule.contact_cooldown,
                    pulse.rule.knockback_delay
                ),
                (tick, id, power, stun, cooldown, delay)
            );
            assert_eq!(
                (
                    pulse.rule.flags,
                    pulse.rule.conditions,
                    pulse.rule.condition_chance,
                    pulse.rule.condition_parameter,
                    pulse.rule.impact_bank
                ),
                (
                    if kind == Kind::Efreet {
                        0x2228
                    } else if kind == Kind::Sylph && id == 3 {
                        0x2224
                    } else {
                        0x2220
                    },
                    0,
                    0,
                    0,
                    6
                )
            );
            assert_eq!(
                pulse.rule.element,
                if kind == Kind::Sylph && id == 3 {
                    HitElement::Neutral
                } else {
                    kind.element()
                        .map_or(HitElement::Neutral, HitElement::Element)
                }
            );
            let start = usize::from(id) * 400;
            if matches!(kind, Kind::Volt | Kind::Shadow) {
                assert_eq!(
                    crate::digest(&source[start..start + 400]),
                    if kind == Kind::Volt {
                        "1b0824bde47ba1cacbc404609d57da23882e81ea7369805a8ec58938c843ef7c"
                    } else {
                        "322b9bd8860a2784546703097124a1183df4e87329991cfa29d4dc484633ec40"
                    }
                );
            }
            let flight = crate::battle::effects::projectile(
                &source[start..start + 400],
                pulse.projectile,
                1.,
            )
            .unwrap();
            assert_eq!(flight.lifetime, lifetime);
            assert_eq!(flight.birth_bank, Some(pulse.projectile.bank));
            assert_eq!(
                flight.spawn_effect,
                (kind == Kind::Sylph && id == 2).then_some(kind.effect(3))
            );
            assert!(flight.trail_effect.is_none());
            let (expected_velocity, expected_acceleration) = match (kind, id) {
                (Kind::Celsius, _) => ([0., 0., 37.5], [0.; 3]),
                (Kind::Sylph, 1) => ([0., 15., 0.], [0.; 3]),
                (Kind::Sylph, 2) => ([0., -0.5, 36.], [0., -0.2, 0.]),
                (Kind::Sylph, 3) => ([0., 0., 11.], [0., 0., -0.25]),
                _ => ([0.; 3], [0.; 3]),
            };
            assert!(
                matches!(flight.movement, resonance_content::battle::effects::ProjectileMovement::Ballistic {velocity, acceleration, steering: None}
                if velocity == expected_velocity && acceleration == expected_acceleration)
            );
            if matches!(kind, Kind::Sylph | Kind::Celsius) {
                assert_eq!(
                    (
                        pulse.rule.stun_chance,
                        pulse.rule.stagger,
                        pulse.rule.guard_pressure,
                        pulse.rule.power_mode,
                        pulse.rule.sound,
                        pulse.rule.stagger_resistance,
                        pulse.rule.impact_effect
                    ),
                    (
                        if kind == Kind::Celsius { 5 } else { 0 },
                        u8::from(kind == Kind::Celsius),
                        1,
                        1,
                        0,
                        0,
                        0
                    )
                );
                let (shape, radius, height) = match (kind, id) {
                    (Kind::Sylph, 1) => (HitShapeKind::Cylinder, 275., 300.),
                    (Kind::Sylph, 2) => (HitShapeKind::Box, 32., 32.),
                    (Kind::Sylph, 3) => (HitShapeKind::Cylinder, 100., 100.),
                    _ => (HitShapeKind::Cylinder, 150., 200.),
                };
                assert_eq!(
                    (flight.shape.kind, flight.shape.radius, flight.shape.height),
                    (shape, radius, height)
                );
                let hash = match (kind, id) {
                    (Kind::Sylph, 1) => {
                        "da0af26cf9bfac227ac1f737819fca5bd2642ad3e73eb4c3a42599c65b2be3a8"
                    }
                    (Kind::Sylph, 2) => {
                        "efbe80a68fc4a5b673e10e1f4f2ee88f2e88cabc87807ae61fa3ebf4afa2b2e0"
                    }
                    (Kind::Sylph, 3) => {
                        "97ce2a006f9449907ac6a4008823d5717d1bca84a04fb774e9ad471c5bdf38b0"
                    }
                    (Kind::Celsius, 1) => {
                        "7e6d0bd876365aae9c3d5964286539589d47dfc3f49d66e1d004a869005df5ff"
                    }
                    _ => "d63778bd6175cb91178802416c187c80e65790387dd3fc435ce690632c9c3d98",
                };
                assert_eq!(crate::digest(&source[start..start + 400]), hash);
            }
            if matches!(
                kind,
                Kind::Undine | Kind::Origin | Kind::Volt | Kind::Shadow
            ) {
                assert_eq!(flight.shape.kind, HitShapeKind::Cylinder);
                assert_eq!(
                    (flight.shape.radius, flight.shape.height),
                    match kind {
                        Kind::Undine => (170., 400.),
                        Kind::Volt => (400., 500.),
                        Kind::Shadow => (350., 250.),
                        _ => (350., 200.),
                    }
                );
            }
        }
        if kind == Kind::Origin {
            let mut invalid = recipe.clone();
            invalid.pulses[0].rule.element = HitElement::Inherit;
            assert!(invalid.validate().is_err());
        }
    }
}
