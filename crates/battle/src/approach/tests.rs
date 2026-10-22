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
    let mut battle = fixture(100., 0., 200.);
    battle.actors[0].heading = 90.;
    battle.actors[1].position = [100., 0., 100.];
    let tp = battle.actors[0].tp;
    let mut turning_updates = 0;
    let mut started = false;
    for _ in 0..UPDATE_BUDGET {
        let frame = step(&mut battle)?;
        if !frame.actions.is_empty() {
            assert_eq!(frame.actions.len(), 1);
            started = true;
            break;
        }
        turning_updates += 1;
        assert_eq!(battle.actors[0].tp, tp);
    }
    assert!(started && turning_updates > 0);
    assert!((battle.actors[0].heading - 45.).abs() < 0.001);
    assert_eq!(battle.actors[0].tp, tp - 7);
    step(&mut battle)?;
    assert_eq!(battle.actors[0].tp, tp - 7);

    let mut enemy = controlled_fixture(400., 0., 100., Control::Enemy);
    for position in [0., 0., 0.5, 1.5] {
        step(&mut enemy)?;
        assert_eq!(enemy.actors[0].position[0], position);
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
        let mut coasted = false;
        for _ in 0..UPDATE_BUDGET {
            reached_range |= battle.body_gap(ActorId(0), ActorId(1)) >= minimum;
            let coasting = control == Control::Enemy
                && battle.actors[0].movement.locomotion == crate::Locomotion::Action
                && battle.actors[0].movement.forward > 0.;
            let previous_position = battle.actors[0].position;
            started |= step(&mut battle)?.cues.iter().any(|cue| {
                matches!(
                    cue,
                    Cue::Started {
                        actor: ActorId(0),
                        ..
                    }
                )
            });
            if coasting {
                assert!(battle.actors[0].position[0] < previous_position[0]);
                coasted = true;
            }
            if let Some(position) = alignment_position {
                assert_eq!(battle.actors[0].position, position);
            } else if battle.actors[0].movement.locomotion == crate::Locomotion::Action
                && (control != Control::Enemy || battle.actors[0].movement.forward == 0.)
            {
                alignment_position = Some(battle.actors[0].position);
            }
            if started {
                break;
            }
        }
        assert!(started, "{control:?} thresholds {minimum}/{maximum}");
        assert!(reached_range);
        assert!(battle.actors[0].position[0] < 0.);
        if control == Control::Enemy {
            assert!(coasted);
            assert_eq!(battle.actors[0].movement.forward, 0.);
        } else if turning_body {
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
fn enemy_retreat_checks_stopping_distance_after_acceleration() -> Result<()> {
    let mut battle = controlled_fixture(118.75, 100., 150., Control::Enemy);
    step(&mut battle)?;
    battle.actors[0].movement.forward = 1.;
    assert_eq!(battle.body_gap(ActorId(0), ActorId(1)), 98.75);

    step(&mut battle)?;
    assert_eq!(battle.actors[0].position, [-1., 0., 0.]);
    assert_eq!(battle.actors[0].movement.forward, 1.5);
    let approach = battle.runtime[0].task().approach().unwrap();
    // 98.75 is outside the old 100 - 1 threshold, but inside 100 - 1.5.
    assert!(approach.retreating);
    assert!(!approach.range_required);
    assert_eq!(
        battle.actors[0].movement.locomotion,
        crate::Locomotion::Action
    );
    Ok(())
}

#[test]
fn enemy_retreat_becomes_forward_admission_when_target_enters_range() -> Result<()> {
    let mut battle = controlled_fixture(119., 100., 200., Control::Enemy);
    let tp = battle.actors[0].tp;
    step(&mut battle)?;
    assert_eq!(battle.actors[0].position, [0.; 3]);
    assert!(battle.runtime[0].task().approach().unwrap().retreating);

    // The next sampled gap is above the retreat threshold before movement.
    // Finish that walking visit, then face and admit on the following update.
    battle.actors[1].position[0] = 121.;
    let frame = step(&mut battle)?;
    assert!(
        !frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Started { .. }))
    );
    assert_eq!(battle.actors[0].position, [0.; 3]);
    assert_eq!(battle.actors[0].movement.direction, [1., 0., 0.]);
    let approach = battle.runtime[0].task().approach().unwrap();
    assert!(!approach.retreating);
    assert!(!approach.range_required);

    let frame = step(&mut battle)?;
    assert_eq!(
        frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Started { .. }))
            .count(),
        1
    );
    assert_eq!(battle.actors[0].tp, tp - 7);
    assert_eq!(battle.actors[0].heading, 90.);
    Ok(())
}

#[test]
fn enemy_retreat_near_arena_edge_finishes_braking_before_admission() -> Result<()> {
    for start_x in [0., 800.] {
        let mut battle = controlled_fixture(110., 100., 100., Control::Enemy);
        let tp = battle.actors[0].tp;
        step(&mut battle)?;
        battle.actors[0].position[0] = start_x;
        battle.actors[1].position[0] += start_x;
        battle.actors[0].movement.forward = 2.;
        let frame = step(&mut battle)?;
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Started { .. }))
        );
        let approach = battle.runtime[0].task().approach().unwrap();
        assert!(approach.retreating);
        assert_eq!(approach.range_required, start_x == 0.);
        if start_x == 0. {
            continue;
        }
        let mut started = false;
        let mut coasted = false;
        for _ in 0..UPDATE_BUDGET {
            let position = battle.actors[0].position;
            started = step(&mut battle)?
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Started { .. }));
            coasted |= battle.actors[0].position[0] < position[0];
            if started {
                break;
            }
        }
        assert!(coasted && started);
        assert_eq!(battle.actors[0].movement.forward, 0.);
        assert_eq!(battle.actors[0].tp, tp - 7);
    }
    Ok(())
}

#[test]
fn enemy_approach_rechecks_retreat_when_target_crosses_a_zero_width_range() -> Result<()> {
    let mut battle = controlled_fixture(550., 500., 500., Control::Enemy);
    let tp = battle.actors[0].tp;
    step(&mut battle)?;
    assert!(battle.body_gap(ActorId(0), ActorId(1)) > 500.);
    assert!(!battle.runtime[0].task().approach().unwrap().retreating);
    // Crossing the entire range does not authorize admission. The current
    // gap still needs ordinary retreat, even if the previous gap was above it.
    battle.actors[1].position[0] -= 60.;
    let frame = step(&mut battle)?;
    let approach = battle.runtime[0].task().approach().unwrap();
    assert!(approach.retreating && approach.range_required);
    assert!(
        !frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Started { .. }))
    );
    assert_eq!(battle.actors[0].tp, tp);
    Ok(())
}

#[test]
fn opposite_initial_direction_recovers_and_reaches_a_moving_target() -> Result<()> {
    let mut battle = fixture(300., 0., 100.);
    let parameters = battle.runtime[0].task().approach().unwrap().parameters;
    battle.actors[0].movement.direction = [-1., 0., 0.];
    assert!(battle.request_approach(ActorId(0), ActorId(1), crate::ActionKey(0), parameters,)?);
    step(&mut battle)?;
    assert!(battle.actors[0].position[0] > 0.);
    assert_eq!(battle.actors[0].heading, 90.);

    battle.actors[1].position[2] = 150.;
    // Steering consumes the target's sampled center on the following visit.
    step(&mut battle)?;
    step(&mut battle)?;
    assert!(battle.actors[0].movement.direction[2] > 0.);
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
