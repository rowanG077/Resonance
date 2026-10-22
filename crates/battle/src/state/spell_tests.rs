use super::*;
use crate::{
    Activity, SpellSlot,
    tests::{actor, prepared},
};
use std::sync::Arc;

fn battle() -> Battle {
    let p = prepared(
        vec![actor(Side::Party), actor(Side::Enemy), actor(Side::Party)],
        30,
    );
    p.finish().unwrap()
}

fn request() -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: crate::ActionKey(0),
            target: ActorId(1),
        }],
        ..Default::default()
    }
}

#[test]
fn summon_keeps_arrival_and_heading_and_visits_each_ally_once() {
    use crate::conditions::Condition;
    use crate::{
        PreparedSummon, SummonBlessing, SummonContact, SummonEffect, SummonHeading, SummonOrigin,
        SummonPoint,
    };
    let mut battle = battle();
    battle.actors[0].position = [0.; 3];
    battle.actors[0].heading = 90.;
    battle.actors[1].position = [200., 40., 100.];
    battle.actors[2].position = [1000., 0., 0.];
    battle.actors[0]
        .conditions
        .apply_stat_condition(Condition::AttackDown, 10, 100, false);
    let effect = crate::EffectAppearance {
        resource: 2,
        member: 1,
    };
    let mut definition = crate::tests::volley();
    let projectile = Arc::clone(&definition.projectiles[0].definition);
    definition.projectiles.clear();
    definition.duration = 100;
    definition.summon = Some(Arc::new(PreparedSummon {
        entry: crate::SummonEntryDefinition {
            effect,
            sound: None,
        },
        origin: SummonOrigin::TargetGround {
            height: 0.,
            nudge: 0.,
            direction_threshold: 0.5,
        },
        effects: vec![SummonEffect {
            at: 0,
            appearance: effect,
            point: SummonPoint::Arrival([0.; 3]),
            heading: SummonHeading::Captured(0.),
        }],
        contacts: vec![SummonContact {
            at: 5,
            projectile,
            point: SummonPoint::Arrival([0., 0., 20.]),
            heading: SummonHeading::Captured(45.),
        }],
        voices: vec![],
        sounds: vec![],
        scale: 1.,
        title: "Summon".into(),
        focus_distance: 1850.,
        focus_ticks: 20,
        focus_world: false,
        blessing: SummonBlessing::Attack(15),
        blessing_radius: 500.,
    }));
    definition.validate().unwrap();
    let mut volley = crate::release::Volley {
        transfer_phase: Default::default(),
        definition: Arc::new(definition),
        actor: ActorId(0),
        target: ActorId(1),
        projectile_target: battle.actors[1].effect_origin(),
        slot: SpellSlot::Primary,
        age: 0,
        origin: None,
        effect_origin: None,
        heading: 0.,
        emitted: 0,
        summon: None,
        recipient: None,
        cast_pose: None,
        scene_action: None,
    };
    let mut cues = Vec::new();
    volley.step(&mut battle, ActionId(42), &mut cues).unwrap();
    assert!(
        cues.iter().any(
            |c| matches!(c, Cue::Effect(e) if e.origin == [200., 0., 100.] && e.heading == 90.)
        )
    );
    battle.actors[0].heading = 0.;
    battle.actors[1].position = [900., 0., 900.];
    for age in 1..=100 {
        if age == 32 {
            assert!(
                !battle.actors[0]
                    .conditions
                    .effective()
                    .contains(Condition::AttackDown)
            );
            battle.actors[0]
                .conditions
                .apply_stat_condition(Condition::AttackDown, 10, 100, false);
        }
        if age == 40 {
            battle.actors[2].position = [10., 0., 0.];
        }
        assert_eq!(
            volley.step(&mut battle, ActionId(42), &mut cues).unwrap(),
            age == 100
        );
        if age == 5 {
            let flight = battle.projectiles.values().next().unwrap();
            assert_eq!(flight.frame.heading, 135.);
            assert_eq!(flight.frame.position, [220., 0., 100.]);
            battle
                .emit(
                    Arc::clone(&flight.definition),
                    ActionId(43),
                    ActorId(0),
                    ActorId(1),
                    [0.; 3],
                )
                .unwrap();
        }
    }
    assert!(
        battle.actors[0]
            .conditions
            .effective()
            .contains(Condition::AttackDown)
    );
    assert_eq!(
        battle.actors[2].conditions.magnitude(Condition::AttackUp),
        15
    );
    assert_eq!(
        battle.actors[2].conditions.remaining(Condition::AttackUp),
        None
    );
    assert_eq!(
        cues.iter()
            .filter(|c| matches!(c, Cue::CustomLabel { .. }))
            .count(),
        2
    );
    assert_eq!(
        cues.iter()
            .filter(|cue| matches!(cue, Cue::EffectNotice { .. }))
            .collect::<Vec<_>>(),
        [&Cue::EffectNotice {
            actor: ActorId(0),
            text: "Summon".into(),
            duration: 100,
        }]
    );
    assert_eq!(
        battle.actors[1].conditions.magnitude(Condition::AttackUp),
        0
    );
    assert_eq!(battle.projectiles.len(), 1);
    assert_eq!(
        battle.projectiles.values().next().unwrap().action,
        ActionId(43)
    );
    assert_eq!(
        cues.iter()
            .filter(|cue| matches!(
                cue,
                Cue::ClearEffectBank {
                    actor: ActorId(0),
                    resource: 2
                }
            ))
            .count(),
        1
    );

    // Over Limit admits the cast; its expiry cannot undo an already accepted chant.
    let mut casting = self::battle();
    let mut model = crate::tests::model_definition(7, [(0, 500.), (3, 500.), (12, 500.)]);
    model.reactions.chant = Some(0);
    model.reactions.chant_rate = 0.5;
    model.reactions.cast = Some(12);
    let model = Arc::new(model);
    casting.set_models(
        crate::Models::new(
            casting.actors(),
            vec![Some(model); 3],
            Default::default(),
            casting.diagnostics.clone(),
        )
        .unwrap(),
    );
    casting.prepared.actions.entries[0] = Arc::new(crate::ActionDefinition {
        normal: None,
        tp_cost: 7,
        execution: crate::ActionExecution::Casting(Arc::new(crate::CastingDefinition {
            armor: None,
            incantation_lead: None,
            chant: vec![],
            release_pose: Default::default(),
            hide_primary_weapons: false,
            duration: 8,
            profile_ticks: 8,
            spell_ticks: 0,
            fixed_timing: false,
            recovery: 90,
            release: Arc::clone(&volley.definition),
            threat: None,
        })),
    });
    assert_eq!(
        casting.action_candidate(ActorId(0), crate::ActionKey(0), [0., 1000.]),
        Err(crate::Rejection::Unavailable)
    );
    casting.actors[0].equipment.casting = crate::CastingTraits {
        speed_cast: true,
        angel_song: true,
        rhythm: true,
        spell_save: true,
        reprise: true,
        quick: true,
        random: true,
        ..Default::default()
    };
    casting.actors[0].casting_state.previous_spell = Some(crate::ActionKey(0));
    casting.actors[0].casting_state.interrupted = Some((crate::ActionKey(0), 1));
    casting.actors[0]
        .conditions
        .reload_layers(crate::conditions::Layers {
            intrinsic: Condition::CastingSpeed.into(),
            ..Default::default()
        });
    casting.actors[0].overlimit = crate::OverLimit::active(1000).unwrap();
    casting.step(request()).unwrap();
    assert_eq!(casting.casting_remaining(ActorId(0)), Some(8));
    casting.actors[0].overlimit = Default::default();
    for remaining in (0..8).rev() {
        casting.step(BattleInput::default()).unwrap();
        assert_eq!(casting.casting_remaining(ActorId(0)), Some(remaining));
        assert!(!casting.summon_entry.active());
    }
    casting.step(BattleInput::default()).unwrap();
    assert!(casting.summon_entry.active());
    assert_eq!(casting.actors[0].tp, 33);
}

#[test]
fn stored_origins_capture_position_and_heading_before_later_placements() {
    use crate::{EffectAppearance, SpellOrigin, SpellStartup};
    for transition_ticks in [0, 2] {
        for (origin, target_height, expected) in [
            (
                SpellOrigin::SceneTarget {
                    minimum_height: 176.,
                },
                20.,
                [10., 176., 20.],
            ),
            (
                SpellOrigin::SceneTarget {
                    minimum_height: 176.,
                },
                200.,
                [10., 270., 20.],
            ),
            (SpellOrigin::SceneAboveTarget, 0., [0., 140., 0.]),
            (SpellOrigin::SceneAboveTarget, 200., [0., 740., 0.]),
            (
                SpellOrigin::SceneGround { toward_caster: 1. },
                20.,
                [-0.6, 0., -0.8],
            ),
            (
                SpellOrigin::SceneCasterAhead {
                    distance: 80.,
                    height: 100.,
                },
                20.,
                [84., 100., -9.],
            ),
        ] {
            let mut battle = battle();
            let mut definition = crate::tests::volley();
            definition.projectiles = vec![crate::SpellProjectile::new(
                transition_ticks + 3,
                Arc::clone(&definition.projectiles[0].definition),
            )];
            let projectile = &mut definition.projectiles[0];
            projectile.origin_offset = [0., 30., 40.];
            projectile.launch = crate::SpellLaunch::AtHeight(64.);
            Arc::make_mut(&mut projectile.definition).offset = [0., 0., 10.];
            definition.effects = vec![
                crate::SpellEffect {
                    at: transition_ticks + 3,
                    deferred: false,
                    appearance: EffectAppearance {
                        resource: 2,
                        member: 2,
                    },
                    placement: crate::SpellEffectPlacement::Scene([0., 30., 40.]),
                    captured_heading_offset: Some(20.),
                },
                crate::SpellEffect {
                    at: transition_ticks + 3,
                    deferred: false,
                    appearance: EffectAppearance {
                        resource: 2,
                        member: 3,
                    },
                    placement: crate::SpellEffectPlacement::Scene([0.; 3]),
                    captured_heading_offset: None,
                },
            ];
            definition.origin = origin;
            definition.transition_ticks = transition_ticks;
            definition.duration = transition_ticks + 4;
            definition.startup = Some((
                transition_ticks + 1,
                SpellStartup::Effect(EffectAppearance {
                    resource: 2,
                    member: 1,
                }),
            ));
            definition.validate().unwrap();
            let mut volley = crate::release::Volley {
                transfer_phase: Default::default(),
                summon: None,
                recipient: None,
                cast_pose: None,
                scene_action: None,
                definition: Arc::new(definition),
                actor: ActorId(0),
                target: ActorId(1),
                projectile_target: battle.actors[1].effect_origin(),
                slot: SpellSlot::Primary,
                age: 0,
                origin: None,
                effect_origin: None,
                heading: 0.,
                emitted: 0,
            };
            let mut cues = Vec::new();
            for _ in 0..=transition_ticks {
                assert!(!volley.step(&mut battle, ActionId(0), &mut cues).unwrap());
                assert!(volley.origin.is_none() && cues.is_empty());
            }
            battle.actors[0].position = [-3., 25., -4.];
            battle.actors[0].body.center_offset = [5., 30., 7.];
            battle.actors[0].heading = if matches!(origin, SpellOrigin::SceneCasterAhead { .. }) {
                90.
            } else {
                0.
            };
            battle.actors[1].position = [0., target_height, 0.];
            battle.actors[1].body.center_offset = [10., 70., 20.];
            battle.actors[1].heading = 0.;
            volley.step(&mut battle, ActionId(0), &mut cues).unwrap();
            let captured = volley.origin.unwrap();
            assert!(
                captured
                    .into_iter()
                    .zip(expected)
                    .all(|(a, b)| (a - b).abs() < 0.0001),
                "{origin:?}: {captured:?} != {expected:?}"
            );
            assert!(
                cues.iter()
                    .any(|cue| matches!(cue, Cue::Effect(request) if request.origin == captured))
            );
            let heading = battle.actors[0].heading;
            battle.actors[0].position = [500., 500., 500.];
            battle.actors[0].heading = 180.;
            battle.actors[1].position = [900., 900., 900.];
            cues.clear();
            volley.step(&mut battle, ActionId(0), &mut cues).unwrap();
            assert!(cues.is_empty() && battle.projectiles.is_empty());
            volley.step(&mut battle, ActionId(0), &mut cues).unwrap();
            assert_eq!(volley.origin, Some(captured));
            assert_eq!(volley.heading, heading);
            let offset = if heading == 90. {
                [40., 30., 0.]
            } else {
                [0., 30., 40.]
            };
            let point: [f32; 3] = std::array::from_fn(|axis| captured[axis] + offset[axis]);
            let [Cue::Effect(placed), Cue::Effect(world)] = cues.as_slice() else {
                panic!("unexpected delayed spell effects: {cues:?}");
            };
            assert!(
                placed
                    .origin
                    .into_iter()
                    .zip(point)
                    .all(|(a, b)| (a - b).abs() < 0.0001)
            );
            assert_eq!(placed.heading, heading + 20.);
            assert_eq!(world.origin, captured);
            assert_eq!(world.heading, 0.);
            let projectile = battle.projectiles.values().next().unwrap();
            let expected_birth = [point[0], point[1], point[2] - 10.];
            assert!(
                projectile
                    .frame
                    .position
                    .into_iter()
                    .zip(expected_birth)
                    .all(|(a, b)| (a - b).abs() < 0.0001)
            );
            assert_eq!(projectile.frame.heading, 180.);
            assert_eq!(projectile.target_point, [captured[0], 64., captured[2]]);
        }
    }
}

#[test]
fn healing_cast_pays_once_and_recovers_a_living_ally_at_its_current_position() {
    for target_dies in [false, true] {
        let mut prepared = prepared(
            vec![actor(Side::Party), actor(Side::Enemy), actor(Side::Party)],
            30,
        );
        for index in [0, 2] {
            prepared.actors[index].control = crate::Control::Manual;
            prepared.actors[index].hp = 100;
            prepared.actors[index].equipment.max_hp = 1000;
        }
        let effect = crate::EffectAppearance {
            resource: 2,
            member: 20,
        };
        let release = crate::PreparedSpell {
            policy: Default::default(),
            summon: None,
            effects: vec![],
            projectiles: vec![],
            recovery: vec![crate::SpellRecovery {
                duration: 1,
                effect_placement: Default::default(),
                flash: None,
                at: 10,
                benefit: crate::SpellBenefit::Hp(30),
                targets: crate::RecoveryTargets::Target,
                effect: Some((effect, Default::default())),
            }],
            origin: crate::SpellOrigin::Target,
            transition_ticks: 0,
            stage_tint: None,
            camera: None,
            duration: 90,
            startup: None,
            sound: None,
        };
        release.validate().unwrap();
        prepared.resources.actions.entries[0] = Arc::new(crate::ActionDefinition {
            normal: None,
            tp_cost: 8,
            execution: crate::ActionExecution::Casting(Arc::new(crate::CastingDefinition {
                armor: None,
                incantation_lead: None,
                chant: vec![],
                release_pose: Default::default(),
                hide_primary_weapons: false,
                duration: 1,
                profile_ticks: 1,
                spell_ticks: 0,
                fixed_timing: false,
                recovery: 90,
                release: Arc::new(release),
                threat: None,
            })),
        });
        let mut battle = prepared.finish().unwrap();
        let tp = battle.actors[0].tp;
        let enemy_hp = battle.actors[1].hp;
        let mut input = request();
        input.actions[0].target = ActorId(2);
        let mut frame = battle.step(input).unwrap();
        let mut released = None;
        let mut recovered = None;
        for tick in 0..120 {
            if frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Released { .. }))
            {
                assert!(released.replace(tick).is_none());
                battle.actors[2].position[0] += 100.;
                if target_dies {
                    battle.actors[2].hp = 0;
                    battle.actors[2].availability = crate::ActorAvailability::Dead;
                }
            }
            if let Some(recipient) = frame.cues.iter().find_map(|cue| match cue {
                Cue::Recovered {
                    actor,
                    nominal: 300,
                    applied: 300,
                    ..
                } => Some(*actor),
                _ => None,
            }) {
                assert!(recovered.replace(tick).is_none());
                assert_eq!(recipient, if target_dies { ActorId(0) } else { ActorId(2) });
                let appearance = frame
                    .cues
                    .iter()
                    .find_map(|cue| match cue {
                        Cue::DeferredEffect(request) if request.appearance == effect => {
                            Some(request)
                        }
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(appearance.owner, recipient);
                assert_eq!(appearance.target, recipient);
                assert_eq!(
                    appearance.origin,
                    battle.actors[recipient.index()].effect_origin()
                );
                assert_eq!(
                    appearance.follow,
                    Some(crate::EffectFollow::Center(recipient))
                );
            }
            assert!(frame.projectiles.is_empty());
            assert_eq!(
                battle.actors[0].tp,
                tp - if released.is_some() { 8 } else { 0 }
            );
            assert_eq!(battle.actors[1].hp, enemy_hp);
            frame = battle.step(BattleInput::default()).unwrap();
        }
        assert_eq!(recovered.unwrap() - released.unwrap(), 10);
        assert_eq!(battle.actors[2].hp, if target_dies { 0 } else { 400 });
        assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
    }
}

#[test]
fn delayed_volley_preserves_shot_spacing() {
    for (side, hold_ticks) in [(Side::Party, 0), (Side::Party, 71), (Side::Enemy, 71)] {
        let mut battle = battle();
        battle.actors[0].side = side;
        battle.actors[1].side = if side == Side::Party {
            Side::Enemy
        } else {
            Side::Party
        };
        battle.actors[0].position = [500., 0., 500.];
        battle.actors[0].heading = 0.;
        battle.actors[0].body.center_offset = [0., 12., 0.];
        let trail = crate::EffectAppearance {
            resource: 37,
            member: 25,
        };
        let mut volley = crate::tests::volley();
        for projectile in &mut volley.projectiles {
            projectile.at += 4;
            projectile.launch = crate::SpellLaunch::CasterAbove {
                center_height_scale: 2.,
                height_offset: 5.,
            };
            let definition = Arc::make_mut(&mut projectile.definition);
            definition.velocity = [1., 2., 3.];
            definition.effects.trail = Some((trail, 1));
        }
        volley.duration = 20;
        if hold_ticks > 0 {
            volley.policy = crate::SpellPolicy::Cutin {
                at: 8,
                catalogue: 251,
                duration: 71,
                hold_ticks,
                reveal: Some((12, "Holy Judgment")),
            };
        }
        let resident = battle
            .release_volley(
                Arc::new(volley),
                ActorId(0),
                ActorId(1),
                SpellSlot::Primary,
                None,
                &mut vec![],
            )
            .unwrap()
            .unwrap();
        let mut shots = Vec::new();
        let mut portraits = Vec::new();
        let mut reveals = Vec::new();
        let mut paused = 0;
        let hold_ticks = if side == Side::Party { hold_ticks } else { 0 };
        for elapsed in 0..=20 + hold_ticks {
            let frame = battle.step(BattleInput::default()).unwrap();
            paused += u16::from(frame.clock.paused());
            if let Some(projectile) = frame.cues.iter().find_map(|cue| match cue {
                Cue::ProjectileStarted {
                    projectile, action, ..
                } if *action == resident => Some(*projectile),
                _ => None,
            }) {
                shots.push(elapsed);
                let first = &battle.projectiles[&projectile];
                assert_eq!(first.frame.position, [501., 31., 503.]);
                assert!(
                    !first.frame.contact_active,
                    "birth does not submit a contact"
                );
                assert!(frame.cues.iter().any(|cue| matches!(cue,
                    Cue::InitializedEffect(effect) if effect.appearance == trail && effect.origin == first.frame.position)));
            }
            if frame.cues.iter().any(
                |cue| matches!(cue, Cue::Cutin { action, catalogue: 251, .. } if *action == resident),
            ) {
                portraits.push(elapsed);
            }
            if frame.cues.iter().any(|cue| {
                matches!(cue,
                Cue::EffectNotice { actor: ActorId(0), text, .. } if text == "Holy Judgment")
            }) {
                reveals.push(elapsed);
            }
        }
        assert_eq!(shots, [4, 12 + hold_ticks, 20 + hold_ticks]);
        assert_eq!(paused, hold_ticks);
        assert_eq!(portraits, if hold_ticks == 0 { vec![] } else { vec![8] });
        assert_eq!(
            reveals,
            if hold_ticks == 0 {
                vec![]
            } else {
                vec![12 + hold_ticks]
            }
        );
        assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
    }
}

#[test]
fn target_spells_retain_the_next_update_origin_through_the_authored_duration() {
    for origin_kind in [crate::SpellOrigin::Target, crate::SpellOrigin::TargetGround] {
        for (owner, target) in [(ActorId(0), ActorId(1)), (ActorId(1), ActorId(0))] {
            let mut battle = battle();
            battle.actors[target.index()].body.center_offset[1] = 80.;
            let mut definition = crate::tests::volley();
            definition.origin = origin_kind;
            definition.duration = 6;
            let mut second = definition.projectiles[0].definition.as_ref().clone();
            second.offset = [0., 12., 0.];
            definition.projectiles = vec![
                crate::SpellProjectile::new(2, Arc::clone(&definition.projectiles[0].definition)),
                crate::SpellProjectile::new(4, Arc::new(second)),
            ];
            let resident = battle
                .release_volley(
                    Arc::new(definition),
                    owner,
                    target,
                    SpellSlot::Primary,
                    None,
                    &mut vec![],
                )
                .unwrap()
                .unwrap();
            battle.step(BattleInput::default()).unwrap();
            assert!(battle.volleys[&resident].origin.is_none());
            battle.actors[target.index()].position[0] += 100.;
            battle.step(BattleInput::default()).unwrap();
            let origin = battle.volleys[&resident].origin.unwrap();
            assert_eq!(origin[0], battle.actors[target.index()].position[0]);
            assert_eq!(
                origin[1],
                if origin_kind == crate::SpellOrigin::Target {
                    80.
                } else {
                    0.
                }
            );
            battle.actors[target.index()].position[0] += 500.;
            let mut births = Vec::new();
            let mut emitted = std::collections::BTreeSet::new();
            for elapsed in 2..=6 {
                let frame = battle.step(BattleInput::default()).unwrap();
                for projectile in frame.projectiles.iter().filter(|p| emitted.insert(p.id)) {
                    assert_eq!(projectile.position[0], origin[0]);
                    assert_eq!(projectile.position[2], origin[2]);
                    let aim = battle.projectiles[&projectile.id].target_point;
                    let target_x = if elapsed == 2 {
                        origin[0]
                    } else {
                        origin[0] + 500.
                    };
                    assert_eq!(aim, [target_x, 80., origin[2]]);
                    births.push((elapsed, projectile.position[1]));
                }
                assert_eq!(battle.spell_active(owner, SpellSlot::Primary), elapsed < 6);
            }
            assert_eq!(births, [(2, origin[1]), (4, origin[1] + 12.)]);
        }
    }
}

#[test]
fn released_volley_aim_uses_the_prior_center_in_both_roster_orders() {
    for (owner, target) in [(ActorId(0), ActorId(1)), (ActorId(1), ActorId(0))] {
        let mut battle = battle();
        battle.actors[target.index()].body.center_offset = [0., 80., 0.];
        battle.actors[target.index()].position = [100., 0., 200.];
        battle.actors[owner.index()].position = [-500., 0., 0.];
        battle.step(BattleInput::default()).unwrap();

        let mut definition = crate::tests::volley();
        definition.origin = crate::SpellOrigin::SceneFixed { position: [0.; 3] };
        definition.duration = 4;
        let mut projectile = definition.projectiles[0].definition.as_ref().clone();
        projectile.motion.aim_at_target = true;
        projectile.contact = None;
        definition.projectiles = vec![crate::SpellProjectile::new(2, Arc::new(projectile))];
        let resident = battle
            .release_volley(
                Arc::new(definition),
                owner,
                target,
                SpellSlot::Primary,
                None,
                &mut vec![],
            )
            .unwrap()
            .unwrap();
        battle.step(BattleInput::default()).unwrap();
        battle.actors[target.index()].position = [200., 0., 300.];
        battle.actors[owner.index()].hit_stop = 3;
        let age = battle.volleys[&resident].age;
        let aim = battle.volleys[&resident].projectile_target;
        let centers = battle.actor_start_centers.clone();
        battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(battle.volleys[&resident].age, age);
        assert_eq!(battle.volleys[&resident].projectile_target, aim);
        assert_eq!(battle.actor_start_centers, centers);

        battle.step(BattleInput::default()).unwrap();
        battle.actors[target.index()].position = [300., 0., 400.];
        let frame = battle.step(BattleInput::default()).unwrap();
        let id = frame
            .cues
            .iter()
            .find_map(|cue| match cue {
                Cue::ProjectileStarted {
                    projectile, action, ..
                } if *action == resident => Some(*projectile),
                _ => None,
            })
            .unwrap();
        let projectile = &battle.projectiles[&id];
        assert_eq!(projectile.target_point, [200., 80., 300.]);
        assert_eq!(
            projectile.velocity,
            crate::distance::normalize([200., 80., 300.])
        );
        assert!(battle.actors[owner.index()].hit_stop > 0);
    }
}

#[test]
fn following_spell_keeps_its_visual_target_and_samples_each_pulse_center() {
    let mut battle = battle();
    battle.actors[1].body.center_offset = [0., 80., 0.];
    let mut definition = crate::tests::volley();
    definition.origin = crate::SpellOrigin::FollowingTarget;
    definition.transition_ticks = 2;
    definition.duration = 8;
    definition.projectiles = vec![
        crate::SpellProjectile::new(4, Arc::clone(&definition.projectiles[0].definition)),
        crate::SpellProjectile::new(6, Arc::clone(&definition.projectiles[0].definition)),
    ];
    definition.startup = Some((
        3,
        crate::SpellStartup::Effect(crate::EffectAppearance {
            resource: 2,
            member: 1,
        }),
    ));
    let resident = battle
        .release_volley(
            Arc::new(definition),
            ActorId(0),
            ActorId(1),
            SpellSlot::Primary,
            None,
            &mut vec![],
        )
        .unwrap()
        .unwrap();
    let mut seen = std::collections::BTreeSet::new();
    let mut births = Vec::new();
    for elapsed in 0..=8 {
        battle.actors[1].position = [100. + elapsed as f32 * 100., 0., 50.];
        let frame = battle.step(BattleInput::default()).unwrap();
        let center = battle.actors[1].effect_origin();
        if elapsed < 3 {
            assert!(battle.volleys[&resident].origin.is_none());
        }
        let effects: Vec<_> = frame
            .cues
            .iter()
            .filter_map(|cue| match cue {
                Cue::Effect(request) => Some(request),
                _ => None,
            })
            .collect();
        assert_eq!(effects.len(), usize::from(elapsed == 3));
        if let Some(effect) = effects.first() {
            assert_eq!(effect.origin, center);
            assert_eq!(effect.follow, Some(crate::EffectFollow::Center(ActorId(1))));
        }
        for projectile in frame.projectiles.iter().filter(|p| seen.insert(p.id)) {
            assert_eq!(projectile.position, center);
            births.push((elapsed, projectile.position));
        }
        assert_eq!(
            battle.spell_active(ActorId(0), SpellSlot::Primary),
            elapsed < 8
        );
    }
    assert_eq!(births, [(4, [500., 80., 50.]), (6, [700., 80., 50.])]);
}

#[test]
fn scene_transition_holds_gameplay_while_release_feedback_advances() {
    let mut battle = battle();
    let parent = battle.step(request()).unwrap().actions[0].0;
    let parent_age = battle.action_age(parent);
    let mut definition = crate::tests::volley();
    definition.transition_ticks = 3;
    let tint = crate::StageTint {
        color: [8, 12, 16],
        duration: 20,
    };
    definition.stage_tint = Some(tint);
    definition.duration = 8;
    let gesture = crate::AttackPose {
        motion: crate::MotionBinding { model: 7, clip: 80 },
        start: 0.,
        end: None,
        repeat: true,
        rate: 0.5,
        blend: 12,
    };
    definition.policy = crate::SpellPolicy::CasterPose {
        at: 5,
        pose: Some(gesture),
        lift: None,
    };
    definition.projectiles.truncate(1);
    definition.projectiles[0].at = 4;
    definition.startup = Some((
        2,
        crate::SpellStartup::DeferredEffect(crate::EffectAppearance {
            resource: 1,
            member: 1,
        }),
    ));
    let resident = battle
        .release_volley(
            Arc::new(definition),
            ActorId(0),
            ActorId(1),
            SpellSlot::Primary,
            Some(parent),
            &mut vec![],
        )
        .unwrap()
        .unwrap();
    battle.volleys.get_mut(&resident).unwrap().scene_action = Some(crate::ActionKey(0));
    let release = Cue::Casting {
        actor: ActorId(0),
        action: crate::ActionKey(0),
        phase: crate::CastPhase::Released,
    };
    let positions: Vec<_> = battle.actors().iter().map(|a| a.position).collect();
    // A cast normally visits release age zero at the end of its gameplay update.
    battle.advance_volleys(&mut vec![], false).unwrap();
    for age in 1..=3 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.clock, BattleClock::SpellTransition);
        assert!(!frame.cues.contains(&release));
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::StageTint(_)))
        );
        assert!(frame.clock.paused());
        assert!(!frame.clock.feedback_paused());
        assert_eq!(battle.action_age(parent), parent_age);
        assert_eq!(battle.action_age(resident), Some(age + 1));
        assert_eq!(
            frame.actors.iter().map(|a| a.position).collect::<Vec<_>>(),
            positions
        );
        assert!(frame.projectiles.is_empty());
        assert_eq!(
            frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::DeferredEffect(_))),
            age == 2
        );
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.clock, BattleClock::Running);
    assert!(frame.cues.contains(&release));
    assert_eq!(frame.projectiles.len(), 1);
    assert!(frame.cues.contains(&Cue::StageTint(tint)));
    assert!(battle.action_age(parent) > parent_age);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert!(!frame.cues.contains(&release));
    let gesture = crate::ModelRequest::Play {
        actor: ActorId(0),
        motion: gesture.motion,
        pose: gesture.playback(),
    };
    assert!(frame.model_requests.contains(&gesture));
    assert!(
        !frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::StageTint(_)))
    );
    for age in 6..=8 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert!(!frame.model_requests.contains(&gesture));
        assert_eq!(
            frame.cues.contains(&Cue::Completed { action: resident }),
            age == 8
        );
    }
    assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
}

#[test]
fn independent_slots_survive_caster_actions_and_reject_replacement() {
    let mut battle = battle();
    let mut cues = vec![];
    let primary = battle
        .release_volley(
            Arc::new(crate::tests::volley()),
            ActorId(0),
            ActorId(1),
            SpellSlot::Primary,
            None,
            &mut cues,
        )
        .unwrap()
        .unwrap();
    let secondary = battle
        .release_volley(
            Arc::new(crate::tests::volley()),
            ActorId(0),
            ActorId(2),
            SpellSlot::Secondary,
            None,
            &mut cues,
        )
        .unwrap()
        .unwrap();
    assert_ne!(primary, secondary);
    let frame = battle.step(request()).unwrap();
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Started {
            actor: ActorId(0),
            ..
        }
    )));
    let age = battle.action_age(primary);
    cues.clear();
    assert_eq!(
        battle
            .release_volley(
                Arc::new(crate::tests::volley()),
                ActorId(0),
                ActorId(2),
                SpellSlot::Primary,
                None,
                &mut cues
            )
            .unwrap(),
        None
    );
    assert!(cues.is_empty());
    assert_eq!(battle.volleys[&primary].target, ActorId(1));
    assert_eq!(battle.volleys[&secondary].target, ActorId(2));
    assert_eq!(battle.action_age(primary), age);
    battle
        .step(BattleInput {
            interrupt: vec![primary],
            ..Default::default()
        })
        .unwrap();
    assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
    assert!(battle.spell_active(ActorId(0), SpellSlot::Secondary));
    assert!(
        battle
            .release_volley(
                Arc::new(crate::tests::volley()),
                ActorId(0),
                ActorId(1),
                SpellSlot::Primary,
                None,
                &mut cues
            )
            .unwrap()
            .is_some()
    );
}

#[test]
fn released_spell_survives_caster_interruption_hurt_and_death_then_cleans_up() {
    let mut battle = battle();
    let parent = battle.step(request()).unwrap().actions[0].0;
    let resident = battle
        .release_volley(
            Arc::new(crate::tests::volley()),
            ActorId(0),
            ActorId(1),
            SpellSlot::Primary,
            Some(parent),
            &mut vec![],
        )
        .unwrap()
        .unwrap();
    let frame = battle
        .step(BattleInput {
            interrupt: vec![parent, parent],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        frame
            .cues
            .iter()
            .filter(|cue| **cue == Cue::Interrupted { action: parent })
            .count(),
        1
    );
    assert_eq!(battle.action_age(parent), None);
    assert_eq!(frame.actors[0].activity, Activity::Idle);
    assert!(battle.spell_active(ActorId(0), SpellSlot::Primary));

    battle.begin_hurt(ActorId(0), 10, &mut vec![]);
    battle.step(BattleInput::default()).unwrap();
    battle.actors[0].hp = 0;
    battle.enter_death(ActorId(0), &mut vec![]);
    let mut completed = false;
    for _ in 0..90 {
        let frame = battle.step(BattleInput::default()).unwrap();
        completed |= frame.cues.contains(&Cue::Completed { action: resident });
    }
    assert!(completed);
    assert_eq!(battle.actors[0].hp, 0);
    assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
}

#[test]
fn party_recovery_keeps_roster_models_separate_from_living_recipients() {
    let mut battle = prepared(
        vec![
            actor(Side::Party),
            actor(Side::Enemy),
            actor(Side::Party),
            actor(Side::Party),
            actor(Side::Party),
        ],
        30,
    )
    .finish()
    .unwrap();
    for actor in &mut battle.actors {
        actor.control = crate::Control::Manual;
        actor.hp = 100;
        actor.equipment.max_hp = 1000;
    }
    battle.actors[3].hp = 0;
    battle.actors[3].availability = crate::ActorAvailability::Dead;
    battle.actors[4].availability = crate::ActorAvailability::Petrified;
    battle.actors[2].availability = crate::ActorAvailability::Absent;
    battle.actors[0].heading = 90.;
    let effect = |member| crate::EffectAppearance {
        resource: 2,
        member,
    };
    let definition = crate::PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![],
        projectiles: vec![],
        recovery: vec![crate::SpellRecovery {
            duration: 1,
            effect_placement: Default::default(),
            flash: None,
            at: 182,
            benefit: crate::SpellBenefit::Hp(40),
            targets: crate::RecoveryTargets::LivingAllies,
            effect: Some((effect(6), Default::default())),
        }],
        origin: crate::SpellOrigin::Caster,
        transition_ticks: 60,
        stage_tint: None,
        camera: None,
        duration: 313,
        startup: Some((
            61,
            crate::SpellStartup::AllyRing {
                effects: [effect(2), effect(3), effect(4), effect(5)],
                first_heading: 15.,
                heading_step: 90.,
            },
        )),
        sound: None,
    };
    definition.validate().unwrap();
    battle
        .release_volley(
            Arc::new(definition),
            ActorId(0),
            ActorId(0),
            SpellSlot::Primary,
            None,
            &mut vec![],
        )
        .unwrap();
    battle.advance_volleys(&mut vec![], false).unwrap();
    let mut recovered = Vec::new();
    for elapsed in 1..=313 {
        if elapsed == 100 {
            // This reserve joins after the model ring starts, before recovery.
            battle.actors[2].availability = crate::ActorAvailability::Active;
            battle.actors[2].position[0] += 100.;
        }
        let frame = battle.step(BattleInput::default()).unwrap();
        let effects: Vec<_> = frame
            .cues
            .iter()
            .filter_map(|cue| match cue {
                Cue::Effect(request) | Cue::DeferredEffect(request) => Some(request),
                _ => None,
            })
            .collect();
        if elapsed == 61 {
            assert_eq!(effects.len(), 3);
            assert_eq!(
                frame
                    .cues
                    .iter()
                    .filter(|cue| matches!(cue, Cue::DeferredEffect(_)))
                    .count(),
                3
            );
            for (slot, request) in effects.into_iter().enumerate() {
                assert_eq!(request.appearance, effect(2 + slot as u16));
                assert_eq!(request.origin, [0.; 3]);
                assert_eq!(request.heading, 105. + slot as f32 * 90.);
            }
        } else if elapsed == 182 {
            assert_eq!(effects.len(), 2);
            for request in effects {
                assert_eq!(
                    request.origin,
                    battle.actors[request.owner.index()].effect_origin()
                );
                assert_eq!(
                    request.follow,
                    Some(crate::EffectFollow::Center(request.owner))
                );
            }
        } else {
            assert!(effects.is_empty());
        }
        for cue in frame.cues {
            if let Cue::Recovered {
                actor,
                nominal: 400,
                applied: 400,
                ..
            } = cue
            {
                assert_eq!(elapsed, 182);
                recovered.push(actor);
            }
        }
        assert_eq!(
            battle.spell_active(ActorId(0), SpellSlot::Primary),
            elapsed < 313
        );
    }
    assert_eq!(recovered, [ActorId(0), ActorId(2)]);
    assert_eq!(battle.actors[1].hp, 100);
    assert_eq!(battle.actors[3].hp, 0);
    assert_eq!(battle.actors[4].hp, 100);
}

#[test]
fn support_pulses_cap_tp_apply_buffs_and_cure_petrified_recipients_once() {
    use crate::RecoveryEffectPlacement;
    use crate::conditions::{Condition, ConditionLabel, Conditions, Cure, Layers};
    let effect = crate::EffectAppearance {
        resource: 0,
        member: 1,
    };
    for (benefit, placement, owner, follow, height, heading, scale) in [
        (
            crate::SpellBenefit::Tp(15),
            RecoveryEffectPlacement::RecipientCenter,
            ActorId(2),
            crate::EffectFollow::Center(ActorId(2)),
            80.,
            0.,
            1.25,
        ),
        (
            crate::SpellBenefit::Stat {
                condition: Condition::DefenseUp,
                percent: 10,
                duration: 1800,
            },
            RecoveryEffectPlacement::RecipientFacingCenter,
            ActorId(0),
            crate::EffectFollow::Center(ActorId(2)),
            80.,
            180.,
            1.25,
        ),
        (
            crate::SpellBenefit::Tp(15),
            RecoveryEffectPlacement::RecipientFacingCenterUnscaled,
            ActorId(0),
            crate::EffectFollow::Center(ActorId(2)),
            80.,
            180.,
            1.,
        ),
        (
            crate::SpellBenefit::Cure(Cure::Physical),
            RecoveryEffectPlacement::RecipientRoot,
            ActorId(2),
            crate::EffectFollow::Actor(ActorId(2)),
            0.,
            180.,
            1.,
        ),
        (
            crate::SpellBenefit::Tp(15),
            RecoveryEffectPlacement::CasterFacingCenter,
            ActorId(0),
            crate::EffectFollow::Center(ActorId(2)),
            80.,
            90.,
            1.,
        ),
    ] {
        let mut battle = battle();
        for actor in &mut battle.actors {
            actor.control = crate::Control::Manual;
        }
        battle.actors[0].heading = 90.;
        battle.actors[0].facing_direction = [1., 0., 0.];
        let target = &mut battle.actors[2];
        target.position = [300., 0., 0.];
        target.body.center_offset = [0., 80., 0.];
        target.heading = 180.;
        target.facing_direction = [0., 0., -1.];
        target.effect_scale = 1.25;
        target.equipment.max_tp = 200;
        target.tp = 195;
        if matches!(benefit, crate::SpellBenefit::Cure(_)) {
            target.availability = crate::ActorAvailability::Petrified;
            target.conditions = Conditions::new(Layers {
                base: [
                    Condition::Petrified,
                    Condition::PoisonMild,
                    Condition::AttackUp,
                ]
                .into_iter()
                .collect(),
                ..Default::default()
            });
        }
        let release = crate::PreparedSpell {
            policy: Default::default(),
            summon: None,
            effects: vec![],
            projectiles: vec![],
            recovery: vec![crate::SpellRecovery {
                duration: 1,
                effect_placement: placement,
                flash: None,
                at: 3,
                benefit,
                targets: crate::RecoveryTargets::Target,
                effect: Some((effect, Default::default())),
            }],
            origin: crate::SpellOrigin::Caster,
            transition_ticks: 0,
            stage_tint: None,
            camera: None,
            duration: 6,
            startup: None,
            sound: None,
        };
        release.validate().unwrap();
        battle
            .release_volley(
                Arc::new(release),
                ActorId(0),
                ActorId(2),
                SpellSlot::Primary,
                None,
                &mut vec![],
            )
            .unwrap();
        let mut benefits = 0;
        for tick in 0..=6 {
            let frame = battle.step(BattleInput::default()).unwrap();
            let effects: Vec<_> = frame
                .cues
                .iter()
                .filter_map(|cue| match cue {
                    Cue::DeferredEffect(request) => Some(request),
                    _ => None,
                })
                .collect();
            if tick == 3 {
                assert_eq!(
                    effects,
                    [&crate::EffectRequest {
                        owner,
                        target: ActorId(2),
                        appearance: effect,
                        origin: [300., height, 0.],
                        heading,
                        follow: Some(follow),
                        scale,
                        tint: Default::default(),
                    }]
                );
            } else {
                assert!(effects.is_empty());
            }
            let target = &battle.actors[2];
            match benefit {
                crate::SpellBenefit::Tp(_) => {
                    assert_eq!(target.tp, if tick < 3 { 195 } else { 200 });
                    benefits += frame
                        .cues
                        .iter()
                        .filter(|cue| {
                            matches!(
                                cue,
                                Cue::Recovered {
                                    floating: true,
                                    actor: ActorId(2),
                                    kind: crate::RecoveryKind::Tp,
                                    nominal: 15,
                                    applied: 5
                                }
                            )
                        })
                        .count();
                }
                crate::SpellBenefit::Stat { .. } => {
                    assert_eq!(
                        target.conditions.magnitude(Condition::DefenseUp),
                        if tick < 3 { 0 } else { 10 }
                    );
                    if tick >= 3 {
                        assert_eq!(
                            target.conditions.remaining(Condition::DefenseUp),
                            Some(1800 - (tick - 3))
                        );
                    }
                    benefits += frame
                        .cues
                        .iter()
                        .filter(|cue| {
                            matches!(
                                cue,
                                Cue::ConditionLabel {
                                    actor: ActorId(2),
                                    kind: ConditionLabel::StatUp(Condition::DefenseUp),
                                    ..
                                }
                            )
                        })
                        .count();
                }
                crate::SpellBenefit::Cure(_) => {
                    assert_eq!(target.is_petrified(), tick < 3);
                    assert_eq!(
                        target.conditions.base().contains(Condition::PoisonMild),
                        tick < 3
                    );
                    assert!(target.conditions.base().contains(Condition::AttackUp));
                }
                _ => unreachable!(),
            }
        }
        if !matches!(benefit, crate::SpellBenefit::Cure(_)) {
            assert_eq!(benefits, 1);
        }
        assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
    }
    let mut battle = battle();
    for actor in &mut battle.actors {
        actor.control = crate::Control::Manual;
    }
    battle.actors[0].body.center_offset = [0., 40., 0.];
    battle.actors[2].position = [300., 0., 0.];
    battle.actors[2].equipment.max_tp = 200;
    battle.actors[2].tp = 195;
    let mut release = crate::tests::volley();
    release.projectiles.clear();
    release.policy = crate::SpellPolicy::TransferRecovery {
        appearance: effect,
        apex: 120.,
        speed: 20.,
        benefit: crate::SpellBenefit::Tp(15),
    };
    release.origin = crate::SpellOrigin::Caster;
    release.duration = 14;
    release.validate().unwrap();
    battle
        .release_volley(
            Arc::new(release),
            ActorId(0),
            ActorId(2),
            SpellSlot::Primary,
            None,
            &mut vec![],
        )
        .unwrap();
    let mut benefits = 0;
    for tick in 0..=14 {
        battle.actors[2].position[0] += 10.;
        let frame = battle.step(BattleInput::default()).unwrap();
        let effects: Vec<_> = frame
            .cues
            .iter()
            .filter_map(|cue| match cue {
                Cue::DeferredEffect(request) => Some(request),
                _ => None,
            })
            .collect();
        if (1..=9).contains(&tick) {
            assert_eq!(effects.len(), 1);
            let expected = if tick <= 4 {
                [0., 40. + tick as f32 * 20., 0.]
            } else {
                [
                    battle.actors[2].position[0],
                    120. - (tick - 4) as f32 * 20.,
                    0.,
                ]
            };
            assert_eq!(effects[0].origin, expected);
            assert_eq!(effects[0].owner, ActorId(0));
            assert_eq!(effects[0].target, ActorId(2));
            assert_eq!(effects[0].follow, None);
        } else {
            assert!(effects.is_empty());
        }
        assert_eq!(battle.actors[2].tp, if tick < 9 { 195 } else { 200 });
        benefits += frame
            .cues
            .iter()
            .filter(|cue| {
                matches!(
                    cue,
                    Cue::Recovered {
                        actor: ActorId(2),
                        kind: crate::RecoveryKind::Tp,
                        nominal: 15,
                        applied: 5,
                        ..
                    }
                )
            })
            .count();
    }
    assert_eq!(benefits, 1);
    assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
}
