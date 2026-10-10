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
    let mut battle = fixture(50., 100., 150.);
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
    assert!(battle.actors[0].position[0] < 0.);
    Ok(())
}

#[test]
fn opposite_initial_direction_recovers_and_reaches_a_moving_target() -> Result<()> {
    let mut battle = fixture(300., 0., 100.);
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
