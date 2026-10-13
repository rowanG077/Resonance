use super::*;
use crate::{BattleInput, Control, Side};

const UPDATE_BUDGET: usize = 300;

fn fixture(distance: f32, minimum: f32, maximum: f32) -> Battle {
    controlled_fixture(distance, minimum, maximum, Control::Auto)
}

fn controlled_fixture(distance: f32, minimum: f32, maximum: f32, control: Control) -> Battle {
    let enemy = control == Control::Enemy;
    let mut owner = crate::tests::actor(if enemy { Side::Enemy } else { Side::Party });
    owner.control = control;
    owner.heading = 90.;
    owner.movement.direction = [1., 0., 0.];
    owner.movement.braking = 1.;
    owner.body.collider = Some(crate::Collider::sphere(10.));
    let mut target = crate::tests::actor(if enemy { Side::Party } else { Side::Enemy });
    target.position = [distance, 0., 0.];

    target.body.collider = Some(crate::Collider::sphere(10.));
    let mut prepared = crate::PreparedBattle::new(
        vec![(owner, Default::default()), (target, Default::default())],
        (vec![crate::ActionDefinition {
            normal: None,
            tp_cost: 7,
            execution: crate::ActionExecution::Attack(crate::PreparedAttack {
                contact_recovery: None,
                chain_at: None,
                end_at: 60,
                opening: None,
                events: vec![],
                recovery: 0,
            }),
        }])
        .into(),
        0,
    )
    .unwrap();
    if !enemy {
        prepared.resources.actor_setup[0].techniques =
            vec![crate::tests::technique(crate::ActionKey(0), 1)];
        prepared = prepared
            .with_technique_learning_members(vec![crate::tests::counted_techniques(
                ActorId(0),
                &[1],
                &[(1, 49)],
            )])
            .unwrap();
    }
    let mut battle = prepared.finish().unwrap();
    assert!(
        battle
            .request_approach(
                ActorId(0),
                ActorId(1),
                crate::ActionKey(0),
                ApproachParameters {
                    minimum,
                    maximum,
                    motion: Some(MotionBinding { model: 7, clip: 99 }),
                    motion_rate: 0.5,
                    speed: 6.,
                    turn_ticks: 8,
                }
            )
            .unwrap()
    );
    battle
}

fn step(battle: &mut Battle) -> Result<crate::BattleFrame> {
    battle.step(BattleInput::default())
}

#[test]
fn approach_enters_range_and_commits_payment_and_learning_once() -> Result<()> {
    for control in [Control::Auto, Control::Enemy] {
        let mut battle = controlled_fixture(400., 0., 100., control);
        let tp = battle.actors[0].tp;
        battle.actors[0].hit_stop = 4;
        let mut starts = 0;
        for _ in 0..UPDATE_BUDGET {
            starts += step(&mut battle)?
                .cues
                .iter()
                .filter(|cue| {
                    matches!(
                        cue,
                        Cue::Started {
                            actor: ActorId(0),
                            ..
                        }
                    )
                })
                .count();
            if starts != 0 {
                break;
            }
        }
        assert_eq!(starts, 1);
        assert!(battle.actors[0].position[0] > 200.);
        assert!(battle.runtime[0].task().approach().is_none());
        let mut completions = 0;
        for _ in 0..100 {
            for cue in step(&mut battle)?.cues {
                match cue {
                    Cue::Started { .. } => panic!("approach restarted its action"),
                    Cue::Completed { .. } => completions += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(completions, 1);
        assert_eq!(battle.actors[0].tp, tp - 7);
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
        if control == Control::Auto {
            assert_eq!(battle.technique_uses(ActorId(0), 1), Some(50));
        }
    }
    Ok(())
}

#[test]
fn minimum_range_retreats_and_then_attacks() -> Result<()> {
    for (control, minimum, maximum) in [
        (Control::Auto, 100., 150.),
        (Control::Auto, 200., 220.),
        (Control::Enemy, 300., 280.),
        (Control::Enemy, 500., 500.),
        (Control::Enemy, 200., 200.),
    ] {
        let mut battle = controlled_fixture(50., minimum, maximum, control);
        let turning_body = control == Control::Enemy || minimum == 200.;
        if turning_body {
            let mut owner = crate::tests::model_definition(7, [(99, 10.)]);
            owner.body_spheres = vec![crate::BodySphere {
                bone: 0,
                radius: 10.,
            }];
            let target = owner.clone();
            // The forward body marker moves toward the victim when the retreating actor turns.
            owner.skeleton.bones[0].bind.translation = [0., -10., 0.];
            battle.set_models(crate::Models::new(
                &battle.actors,
                vec![
                    Some(std::sync::Arc::new(owner)),
                    Some(std::sync::Arc::new(target)),
                ],
                Default::default(),
                resonance_content::diagnostics::Diagnostics::new(true),
            )?);
        }
        let tp = battle.actors[0].tp;
        let mut started = false;
        let mut reached_range = false;
        let mut alignment_position = None;
        for _ in 0..UPDATE_BUDGET {
            reached_range |= battle.body_gap(ActorId(0), ActorId(1)) >= minimum;
            started |= step(&mut battle)?.cues.iter().any(|cue| {
                matches!(
                    cue,
                    Cue::Started {
                        actor: ActorId(0),
                        ..
                    }
                )
            });
            if let Some(position) = alignment_position {
                assert_eq!(battle.actors[0].position, position);
            } else if battle.actors[0].movement.locomotion == crate::Locomotion::Action {
                alignment_position = Some(battle.actors[0].position);
            }
            if started {
                break;
            }
        }
        assert!(started, "{control:?} thresholds {minimum}/{maximum}");
        assert!(reached_range);
        assert!(battle.actors[0].position[0] < 0.);
        if turning_body {
            assert!(battle.body_gap(ActorId(0), ActorId(1)) < minimum);
        } else {
            assert!(battle.body_gap(ActorId(0), ActorId(1)) >= minimum);
        }
        assert_eq!(battle.actors[0].tp, tp - 7);
        assert!(battle.runtime[0].task().approach().is_none());
    }
    let mut party = fixture(50., 100., 150.);
    let mut parameters = party.runtime[0].task().approach().unwrap().parameters;
    parameters.maximum = parameters.minimum;
    assert!(
        party
            .request_approach(ActorId(0), ActorId(1), crate::ActionKey(0), parameters)
            .is_err()
    );
    Ok(())
}

#[test]
fn opposite_initial_direction_recovers_and_reaches_a_moving_target() -> Result<()> {
    let mut battle = fixture(300., 0., 100.);
    let parameters = battle.runtime[0].task().approach().unwrap().parameters;
    battle.actors[0].movement.direction = [-1., 0., 0.];
    assert!(battle.request_approach(
        ActorId(0),
        ActorId(1),
        crate::ActionKey(0),
        parameters,
    )?);
    step(&mut battle)?;
    assert!(battle.actors[0].position[0] > 0.);
    assert_eq!(battle.actors[0].heading, 90.);

    battle.actors[0].movement.direction = [-1., 0., 0.];
    battle.actors[0].heading = -90.;
    step(&mut battle)?;
    assert!(battle.actors[0].movement.direction[2].abs() > 0.1);
    battle.actors[1].position[2] = 150.;
    let mut started = false;
    for _ in 0..UPDATE_BUDGET {
        started |= step(&mut battle)?.cues.iter().any(|cue| {
            matches!(
                cue,
                Cue::Started {
                    actor: ActorId(0),
                    ..
                }
            )
        });
        if started {
            break;
        }
    }
    assert!(started);
    Ok(())
}

#[test]
fn cancellation_releases_the_operation_without_starting_or_spending_tp() -> Result<()> {
    let mut battle = fixture(400., 0., 100.);
    let tp = battle.actors[0].tp;
    battle.actors[0].input.motion = GroundMotion::Stop;
    assert!(
        !step(&mut battle)?
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Started { .. }))
    );
    assert!(battle.runtime[0].task().approach().is_none());
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    assert_eq!(battle.actors[0].tp, tp);
    Ok(())
}

#[test]
fn interruption_drops_approach_without_moving_twice() -> Result<()> {
    let mut battle = fixture(400., 0., 100.);
    battle.begin_hurt(ActorId(0), 10, &mut vec![]);
    let before = battle.actors[0].position;
    assert!(!battle.update_approach(ActorId(0), &mut vec![])?);
    assert!(battle.runtime[0].task().approach().is_none());
    assert_eq!(battle.actors[0].position, before);
    Ok(())
}

#[test]
fn turn_is_bounded_and_handles_zero_and_opposite_directions() {
    let current = [1., 0., 0.];
    assert_eq!(
        crate::control::turn_direction(current, [0.; 3], 15.),
        current
    );
    let turned = crate::control::turn_direction(current, [-1., 0., 0.], 15.);
    let angle = turned[0].atan2(turned[2]).to_degrees();
    assert!((angle - 75.).abs() < 0.001);
    assert!((crate::distance::length(turned) - 1.).abs() < 0.0001);
}
