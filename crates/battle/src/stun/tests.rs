use super::*;
use crate::Activity;
use crate::conditions::{Condition, ConditionSet};
use crate::tests::{prepare_actors, prepared, step_idle};
use crate::{
    ActionId, ActionRequest, BattleFrame, BattleInput, GuardResult, HitRule, MeleeDefinition,
    Power, PreparedBattle, ReactionRule, Side,
};
fn actor(side: Side) -> Actor {
    let mut actor = crate::tests::actor(side);
    actor.body.collider = Some(crate::Collider::sphere(1.));

    actor
}

fn battle() -> Battle {
    let mut prepared = prepared(vec![actor(Side::Party), actor(Side::Enemy)], 120);
    prepared.resources.actor_setup[0] = Default::default();
    crate::tests::assign_action(&mut prepared, 1, crate::ActionKey(0));
    prepared.finish().unwrap()
}

fn hit() -> HitResult {
    HitResult {
        amount: 8,
        hp_change: -8,
        critical: false,
        boosted: false,
        reaction_suppressed: false,
        reaction_forced: false,
        affinity: crate::Affinity::Normal,
        guard: GuardResult::None,
        protection: crate::HitProtection::None,
    }
}

fn melee() -> MeleeDefinition {
    MeleeDefinition {
        hit: HitRule {
            overlimit_pause: true,
            power: Power::Fixed(8),
            reaction: ReactionRule {
                stun_chance: 100,
                hitstun: 20,
                ..Default::default()
            },
            ..crate::tests::hit()
        },
        trail: None,
        volume: crate::MeleeVolume::Fixed {
            offset: [0.; 3],
            radius: 2.,
            half_height: 2.,
        },
    }
}

fn contact(battle: &mut Battle, rule: MeleeDefinition) -> Vec<Cue> {
    let mut contacts = crate::contact::Contacts::default();
    contacts
        .melee(ActorId(0), ActionId(999), &rule, &[])
        .unwrap();
    let mut cues = vec![];
    contacts.resolve(battle, &mut cues, &[]).unwrap();
    cues
}

fn present(
    battle: &mut Battle,
    effects: &mut crate::Effects,
    mut frame: BattleFrame,
) -> BattleFrame {
    battle
        .advance_models(&mut frame, crate::BattleClock::Running, true)
        .unwrap();
    effects.advance(&frame, false).unwrap();
    frame
}

#[test]
fn stun_chance_respects_immunity_resistance_bonuses_and_guarding() {
    let mut owner = actor(Side::Party);
    let mut target = actor(Side::Enemy);
    for chance in [0, 100] {
        target.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
            immunity: ConditionSet::of(&[Condition::Stun]),
            ..Default::default()
        });
        let mut random = crate::state::Random::new(1);
        assert!(!roll(&owner, &target, chance, hit(), |_| Ok(random.next_u16())).unwrap());
        assert_ne!(random.state(), 1);
    }
    target.conditions = Default::default();
    target.reaction.stun.resistance = 255;
    owner.equipment.stun_ex_bonus = true;
    assert_eq!(chance_percent(&owner, &target, 0), 5);
    owner.reaction.stun.chance_bonus = 100;
    target.reaction.stun.resistance = 0;
    assert!(
        roll(&owner, &target, 0, hit(), |chance| {
            assert_eq!(chance, 105);
            Ok(0)
        })
        .unwrap()
    );
    for case in 0..4 {
        let mut target = target.clone();
        let mut result = hit();
        match case {
            0 => target.time_stop = u16::MAX,
            1 => result.protection = crate::HitProtection::Armored,
            2 => result.affinity = crate::Affinity::Absorb,
            _ => {
                result.guard = GuardResult::Blocked {
                    first: true,
                    special: false,
                }
            }
        }
        let mut random = crate::state::Random::new(1);
        assert!(!roll(&owner, &target, 100, result, |_| Ok(random.next_u16())).unwrap());
        assert_eq!(random.state(), 1);
    }
    let mut broken = hit();
    broken.guard = GuardResult::Broken;
    assert!(
        roll(&owner, &target, 100, broken, |_| Ok(
            crate::state::Random::new(1).next_u16()
        ))
        .unwrap()
    );
    assert!(
        roll(&owner, &target, 10, hit(), |_| anyhow::bail!(
            "missing draw"
        ))
        .is_err()
    );
    owner.reaction.stun.chance_bonus = 0;
    owner.equipment.stun_ex_bonus = false;
    for (value, expected) in [(3403, true), (30906, true), (47559, false)] {
        assert_eq!(
            roll(&owner, &target, 10, hit(), |chance| {
                assert_eq!(chance, 10);
                Ok(value)
            })
            .unwrap(),
            expected
        );
    }
    let mut draws = 0;
    assert!(
        !roll(&owner, &target, 0, hit(), |_| {
            draws += 1;
            Ok(0)
        })
        .unwrap()
    );
    assert_eq!(draws, 1);
}

#[test]
fn recorded_random_inputs_require_matching_admission_and_each_draw_exactly_once() {
    use crate::random_inputs::{
        Ailment, Contact, Critical, CriticalRole, HammerRingLaunch, Key, RecordedInputs, Scatter,
        Stun, Velocity,
    };
    let key = Key {
        clock: 2,
        owner: ActorId(0),
        target: ActorId(1),
        action: 1,
        contact: Contact::Projectile(1),
    };
    let distribution = crate::ProjectileScatter {
        position_range: 4,
        position_step: 35.,
        lateral_range: 40,
        lateral_step: 0.1,
    };
    let scatter = Scatter {
        key,
        values: [55693, 23658, 13173],
        distribution,
    };
    let ring = crate::HammerRing {
        count: 10,
        angle_step: 0.6,
        speed: 4.,
        speed_jitter: 0.05,
        lift: 7.,
        lift_jitter: 0.1,
        jitter_range: 50,
    };
    let stun = Stun {
        key,
        value: 3403,
        chance_percent: 10,
    };
    let velocity = Velocity {
        key,
        axis: 1,
        amplitude: 2.,
        value: 33995,
    };
    let condition = crate::HitCondition {
        condition: crate::conditions::Condition::Curse,
        chance: 50,
        value: 0,
    };
    let mut fixture = battle();
    fixture.next_action = 7;
    fixture
        .install_random_inputs(
            10,
            RecordedInputs {
                scatter: vec![scatter.clone()],
                hammer_ring: vec![HammerRingLaunch {
                    key,
                    index: 2,
                    values: [44012, 6446, 53151],
                    distribution: ring,
                }],
                velocity: vec![velocity],
                stun: vec![stun.clone()],
                ailments: Some(
                    [(false, 4343), (true, 65000)]
                        .map(|(resistance, value)| Ailment {
                            key,
                            condition,
                            resistance,
                            value,
                        })
                        .into(),
                ),
                criticals: Some(
                    [(CriticalRole::Chance, 58405), (CriticalRole::Roll, 3204)]
                        .map(|(role, value)| Critical { key, role, value })
                        .into(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(fixture.finish_random_inputs().is_err());
    fixture.ledger.elapsed_ticks = 10;
    assert!(
        fixture
            .verify_random_input_admission(ActorId(0), ActionId(2))
            .is_err()
    );
    fixture
        .verify_random_input_admission(ActorId(0), ActionId(7))
        .unwrap();
    assert!(fixture.finish_random_inputs().is_err());
    let inputs = fixture.random_inputs.as_mut().unwrap();
    let actual = Key {
        clock: 12,
        action: 9,
        ..key
    };
    inputs.released(ActorId(1), Some(ActionId(7)), ActionId(8));
    inputs.released(ActorId(0), Some(ActionId(6)), ActionId(9));
    for action in [8, 9] {
        assert!(inputs.stun(Key { action, ..actual }, 10).is_err());
    }
    inputs.released(ActorId(0), Some(ActionId(7)), ActionId(9));
    for key in [
        Key {
            clock: 11,
            ..actual
        },
        Key {
            contact: Contact::Projectile(2),
            ..actual
        },
    ] {
        assert!(inputs.stun(key, 10).is_err());
    }
    assert!(inputs.stun(actual, 11).is_err());
    assert_eq!(inputs.stun(actual, 10).unwrap(), 3403);
    assert!(inputs.stun(actual, 10).is_err());
    assert!(
        inputs
            .scatter(
                actual,
                crate::ProjectileScatter {
                    position_range: 5,
                    ..distribution
                }
            )
            .is_err()
    );
    assert_eq!(
        inputs.scatter(actual, distribution).unwrap(),
        scatter.values
    );
    assert!(inputs.scatter(actual, distribution).is_err());
    assert!(inputs.hammer_ring(actual, 1, ring).is_err());
    assert!(
        inputs
            .hammer_ring(actual, 2, crate::HammerRing { count: 9, ..ring })
            .is_err()
    );
    assert_eq!(
        inputs.hammer_ring(actual, 2, ring).unwrap(),
        [44012, 6446, 53151]
    );
    assert!(inputs.hammer_ring(actual, 2, ring).is_err());
    for (axis, amplitude) in [(0, 2.), (1, 3.)] {
        assert!(inputs.velocity(actual, axis, amplitude).is_err());
    }
    assert_eq!(
        inputs
            .velocity(
                Key {
                    action: 7,
                    ..actual
                },
                1,
                2.
            )
            .unwrap(),
        33995
    );
    assert!(inputs.velocity(actual, 1, 2.).is_err());
    assert!(inputs.ailment(actual, Some(condition), true).is_err());
    assert!(inputs.critical(actual, CriticalRole::Roll).is_err());
    for (role, value) in [(CriticalRole::Chance, 58405), (CriticalRole::Roll, 3204)] {
        let inputs = fixture.random_inputs.as_mut().unwrap();
        assert_eq!(inputs.critical(actual, role).unwrap(), Some(value));
        assert!(inputs.critical(actual, role).is_err());
        assert!(fixture.finish_random_inputs().is_err());
    }
    let inputs = fixture.random_inputs.as_mut().unwrap();
    for condition in [
        None,
        Some(crate::HitCondition {
            chance: 49,
            ..condition
        }),
    ] {
        assert!(inputs.ailment(actual, condition, false).is_err());
    }
    for (resistance, value) in [(false, 4343), (true, 65000)] {
        assert!(fixture.finish_random_inputs().is_err());
        let inputs = fixture.random_inputs.as_mut().unwrap();
        assert_eq!(
            inputs.ailment(actual, Some(condition), resistance).unwrap(),
            Some(value)
        );
        assert!(inputs.ailment(actual, Some(condition), resistance).is_err());
    }
    fixture.finish_random_inputs().unwrap();
    let mut attached = battle();
    let attached_key = Key {
        contact: Contact::Attached,
        ..key
    };
    attached
        .install_random_inputs(
            10,
            RecordedInputs {
                stun: vec![Stun {
                    key: attached_key,
                    ..stun.clone()
                }],
                ..Default::default()
            },
        )
        .unwrap();
    attached.ledger.elapsed_ticks = 10;
    attached
        .verify_random_input_admission(ActorId(0), ActionId(1))
        .unwrap();
    let inputs = attached.random_inputs.as_mut().unwrap();
    assert!(inputs.stun(actual, 10).is_err());
    assert_eq!(
        inputs
            .stun(
                Key {
                    clock: 12,
                    ..attached_key
                },
                10
            )
            .unwrap(),
        3403
    );
    attached.finish_random_inputs().unwrap();
    for invalid in [
        RecordedInputs {
            scatter: vec![Scatter {
                key: attached_key,
                ..scatter.clone()
            }],
            ..Default::default()
        },
        RecordedInputs {
            scatter: vec![scatter.clone(), scatter],
            stun: vec![stun],
            ..Default::default()
        },
    ] {
        assert!(battle().install_random_inputs(10, invalid).is_err());
    }
}

#[test]
fn stun_interrupts_work_and_recovers_without_resuming_it() {
    for (height, lift) in [(0., 0.), (0., 5.), (1., 0.)] {
        let mut battle = battle();
        let mut model = crate::tests::model_definition(7, [(0, 10.), (1, 10.), (2, 10.)]);
        model.reactions.hurt = [Some(1); 2];
        model.reactions.stunned = Some(2);
        model.idle_motions[0] = Some(0);
        battle.set_models(
            crate::Models::new(
                battle.actors(),
                vec![None, Some(std::sync::Arc::new(model))],
                Default::default(),
                Default::default(),
            )
            .unwrap(),
        );
        let mut effects = crate::Effects::new(vec![], None, 1, Default::default())
            .unwrap()
            .with_stun_particle(Some(std::sync::Arc::new(
                crate::tests::particle_definition(77, 19),
            )))
            .unwrap()
            .with_head_anchors(vec![
                crate::HeadAnchor {
                    bone: 0,
                    offset: [0., 1., 0.],
                };
                2
            ]);
        let start = battle
            .step(BattleInput::from(ActionRequest {
                actor: ActorId(1),
                target: ActorId(1),
                action: crate::ActionKey(0),
            }))
            .unwrap();
        let action = start.actions[0].0;
        battle.actors[1].position[1] = height;
        let mut rule = melee();
        rule.hit.reaction.recoil.impulse[1] = lift;
        rule.hit.reaction.recoil.launch = lift != 0.;
        let cues = contact(&mut battle, rule);
        assert_eq!(
            cues.iter()
                .filter(|cue| **cue == Cue::Interrupted { action })
                .count(),
            1
        );
        assert_eq!(battle.activity(ActorId(1)), Activity::Stunned);
        let frame = battle.publish(cues);
        let frame = present(&mut battle, &mut effects, frame);
        assert_eq!(frame.models[0].clip, 1, "contact retains the hurt pose");
        assert!(effects.frames().is_empty());
        let mut selected_stun = false;
        for _ in 0..4 {
            let was_grounded = battle.actors[1].position[1] == 0.;
            let frame = battle.step(BattleInput::default()).unwrap();
            let frame = present(&mut battle, &mut effects, frame);
            assert_eq!(effects.frames().len(), 1);
            assert_eq!(effects.frames()[0].member, 19);
            assert_eq!(
                frame.models[0].clip,
                if was_grounded { 2 } else { 1 },
                "stun pose begins before the grounded actor moves; landing retains the hurt pose"
            );
            if was_grounded {
                selected_stun = true;
                if lift != 0. {
                    assert!(
                        frame.actors[1].position[1] > 0.,
                        "launch still moves upward"
                    );
                }
                break;
            }
        }
        assert!(selected_stun, "landed actor selects its stun pose");
        let hp = battle.actors[1].hp;
        let mut animated = false;
        for _ in 0..DURATION + 10 {
            let frame = battle.step(BattleInput::default()).unwrap();
            let frame = present(&mut battle, &mut effects, frame);
            animated |= frame.models[0].clip == 2 && frame.models[0].frame > 0.;
            assert_eq!(
                frame.actors[1].hp, hp,
                "interrupted healing must not resume"
            );
            if frame.actors[1].activity == Activity::Recovering {
                assert!(!battle.actor_command_ready(ActorId(1)));
                assert_eq!(
                    frame.models[0].clip, 2,
                    "recovery retains the outgoing pose"
                );
                assert!(
                    effects.frames().is_empty(),
                    "recovery clears the stun birds"
                );
                break;
            }
        }
        assert!(animated, "continuing stun does not restart its animation");
        assert_eq!(battle.activity(ActorId(1)), Activity::Recovering);
        let frame = battle.step(BattleInput::default()).unwrap();
        assert!(battle.actor_command_ready(ActorId(1)));
        let frame = present(&mut battle, &mut effects, frame);
        assert_eq!(
            frame.models[0].clip, 0,
            "idle starts on the following update"
        );
    }
}

#[test]
fn stun_recovery_respects_pause_immunity_and_manual_struggle() {
    fn recovery_time(control: Control, short: bool, struggle: bool) -> u32 {
        let mut target = actor(Side::Party);
        target.control = control;
        if short {
            target.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
                immunity: ConditionSet::of(&[Condition::ShortStun]),
                ..Default::default()
            });
        }
        let mut actions = crate::ActionDefinitions::default();
        let normals =
            crate::tests::normal_controls(&mut actions, crate::tests::attack(1), [0., 1.]);
        let setup = crate::ActorSetup {
            control: Some(std::sync::Arc::new(crate::ControlDefinition {
                normals,
                shortcuts: [None; 4],
                walk_speed: 1.,
                run_speed: 1.,
                turn_ticks: 1,
                motions: None,
            })),
            ..Default::default()
        };
        let mut battle = PreparedBattle::new(
            vec![(target, setup), (actor(Side::Enemy), Default::default())],
            actions,
            0,
        )
        .unwrap()
        .finish()
        .unwrap();
        battle.enter_stun(ActorId(0), &mut vec![]);
        let input = |paused, struggle| BattleInput {
            paused,
            controllers: vec![crate::ControlInput {
                horizontal_pressed: i8::from(struggle),
                ..crate::ControlInput::neutral(ActorId(0))
            }],
            ..Default::default()
        };
        for _ in 0..DURATION + 1 {
            battle.step(input(true, true)).unwrap();
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::Stunned);
        for elapsed in 1..=DURATION + 1 {
            battle.step(input(false, struggle)).unwrap();
            if battle.activity(ActorId(0)) == Activity::Idle {
                return elapsed;
            }
        }
        panic!("stun did not recover");
    }
    let ordinary = recovery_time(Control::Manual, false, false);
    assert!(recovery_time(Control::Manual, true, false) < ordinary);
    assert!(recovery_time(Control::SemiAuto, false, true) < ordinary);
    assert_eq!(recovery_time(Control::Auto, false, true), ordinary);
}

#[test]
fn stun_recovers_without_body_artwork() {
    let mut battle = prepare_actors(vec![actor(Side::Party), actor(Side::Enemy)], 1)
        .finish()
        .unwrap();
    battle.set_diagnostics(Default::default());
    contact(&mut battle, melee());
    assert_eq!(battle.activity(ActorId(1)), Activity::Stunned);
    step_idle(&mut battle, DURATION - 1).unwrap();
    assert_eq!(battle.activity(ActorId(1)), Activity::Stunned);
    assert_eq!(battle.frame(vec![], None).actors[1].stun_remaining, 1);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.actors[1].activity, Activity::Recovering);
    assert_eq!(frame.actors[1].stun_remaining, 0);
    assert!(!battle.actor_command_ready(ActorId(1)));
    battle.step(BattleInput::default()).unwrap();
    assert!(battle.actor_command_ready(ActorId(1)));
    assert_eq!(frame.actors[1].hp, 42);
    assert!(!battle.is_diagnostic());
    battle.recognize_escape(true).unwrap();
    battle.recognize_result();
    assert!(battle.finish_result().unwrap().outcome.is_some());
}
