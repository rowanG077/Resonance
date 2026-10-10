use super::*;
use crate::conditions::{Condition, ConditionSet};
use crate::{
    ActionRequest, ActorId, Battle, BattleInput, Cue, Side,
    tests::{actor, prepared},
};
use std::sync::Arc;

#[test]
fn movement_bonuses_and_heavy_condition_scale_walk_run_and_animation() {
    let mut movement = actor(Side::Party);
    movement.equipment.speed_multiplier = 1.1;
    for (conditions, walk, run, motion) in [
        (ConditionSet::EMPTY, 6.875, 11., 0.55),
        (
            ConditionSet::of(&[Condition::MovementBoost, Condition::Heavy]),
            3.75,
            6.5,
            0.3,
        ),
    ] {
        crate::tests::assert_close(movement.walk_speed(5., 1.25, conditions), walk, 0.0001);
        crate::tests::assert_close(movement.run_limit(10., conditions), run, 0.0001);
        crate::tests::assert_close(movement.motion_rate(0.5, conditions), motion, 0.0001);
    }
    assert_eq!(actor(Side::Party).equipment.speed_multiplier, 1.);
}

fn battle(attack: crate::PreparedAttack) -> Battle {
    let mut owner = actor(Side::Party);
    owner.movement.direction = [1., 0., 0.];
    let mut prepared = prepared(vec![owner, actor(Side::Enemy)], attack.end_at);
    prepared.actors[0].equipment.combo_traits.aerial_arte = true;
    prepared.resources.actor_setup[0].techniques[0]
        .capabilities
        .aerial = true;
    Arc::make_mut(&mut prepared.resources.actions.entries[0]).execution =
        crate::ActionExecution::Attack(attack);
    prepared.finish().unwrap()
}

fn start() -> BattleInput {
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
fn braking_stops_without_reversing_and_preserves_airborne_hurt() {
    for speed in [-10., -1., 0., 1., 10.] {
        let mut movement = Movement {
            forward: speed,
            braking: 3.,
            ..Default::default()
        };
        for _ in 0..4 {
            let previous = movement.forward;
            movement.brake(0., Activity::Idle);
            assert!(movement.forward.abs() <= previous.abs());
            assert!(movement.forward == 0. || movement.forward.signum() == speed.signum());
        }
        assert_eq!(movement.forward, 0.);
    }
    let mut movement = Movement {
        flying: true,
        forward: 0.5,
        braking: 3.,
        ..Default::default()
    };
    assert!(!movement.brake(1., Activity::Hurt));
    assert_eq!(movement.forward, 0.5);
    assert!(movement.brake(1., Activity::Idle));
    assert_eq!(movement.forward, 0.);
}

#[test]
fn ground_clamp_uses_strict_negative_height() {
    let mut owner = actor(Side::Party);
    owner.movement.vertical = -1.;
    floor(&mut owner);
    assert_eq!(owner.movement.vertical, -1.);
    owner.position[1] = -f32::MIN_POSITIVE;
    floor(&mut owner);
    assert_eq!((owner.position[1], owner.movement.vertical), (0., 0.));
}

#[test]
fn local_hit_stop_holds_actor_commands_and_motion_but_ticks_common_timers() {
    let mut battle = battle(crate::PreparedAttack {
        events: [(0, 3.), (1, 6.)]
            .map(|(at, speed)| {
                (
                    at,
                    crate::AttackEvent::Move {
                        forward: Some(speed),
                        vertical: None,
                    },
                )
            })
            .into(),
        ..crate::tests::attack(1)
    });
    battle.actors[0].hit_stop = 2;
    for remaining in [1, 0] {
        let frame = battle
            .step(if remaining == 1 {
                start()
            } else {
                BattleInput::default()
            })
            .unwrap();
        assert_eq!(frame.actors[0].position, [0.; 3]);
        assert_eq!(frame.actors[0].hit_stop, remaining);
        assert_eq!(frame.actions[0].2, 0);
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.actors[0].position[0], 3.);
    assert_eq!(frame.actions[0].2, 1);
    let resumed = battle.step(BattleInput::default()).unwrap();
    assert_eq!(resumed.actors[0].position[0], 9.);
    assert_eq!(resumed.actors[0].activity, Activity::Recovering);
}

#[test]
fn recovery_pauses_under_hit_stop_and_completes_once() {
    let mut battle = battle(crate::PreparedAttack {
        events: vec![(
            0,
            crate::AttackEvent::Move {
                forward: Some(3.),
                vertical: None,
            },
        )],
        recovery: 2,
        ..crate::tests::attack(2)
    });
    let id = battle.step(start()).unwrap().actions[0].0;
    for _ in 0..2 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.activity(ActorId(0)), Activity::Recovering);
    let remaining = battle.action_recovery_remaining(id);
    battle.actors[0].hit_stop = 4;
    for _ in 0..4 {
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.action_recovery_remaining(id), remaining);
    }
    let mut completions = 0;
    for _ in 0..32 {
        let frame = battle.step(BattleInput::default()).unwrap();
        completions += frame
            .cues
            .iter()
            .filter(|c| matches!(c, Cue::Completed { .. }))
            .count();
    }
    assert_eq!(completions, 1);
    assert!(battle.sequences().next().is_none());
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    assert_eq!(battle.actors[0].movement.forward, 0.);
}

#[test]
fn airborne_recovery_waits_for_the_floor_and_menu_pause_preserves_the_countdown() {
    let mut battle = battle(crate::PreparedAttack {
        recovery: 2,
        ..crate::tests::attack(0)
    });
    battle.actors[0].position[1] = 2.;
    let started = battle.step(start()).unwrap();
    let action = started.actions[0].0;
    let before = battle.actors[0].clone();
    let remaining = battle.action_recovery_remaining(action);
    assert_eq!(remaining, Some(2));
    let paused = battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(paused.actors[0].state, before);
    assert_eq!(battle.action_recovery_remaining(action), remaining);
    let mut completed = false;
    for _ in 0..12 {
        let frame = battle.step(BattleInput::default()).unwrap();
        if frame.actions.is_empty() {
            assert_eq!(frame.actors[0].position[1], 0.);
            assert_eq!(frame.actors[0].activity, Activity::Idle);
            assert!(frame.cues.contains(&Cue::Completed { action }));
            completed = true;
            break;
        }
        assert_eq!(frame.actors[0].activity, Activity::Recovering);
    }
    assert!(completed, "airborne recovery must finish after landing");
}

#[test]
fn floating_actors_finish_recovery_without_landing() {
    for (flying, hover_height, fixed_height, completes) in [
        (true, 0., false, true),
        (false, 20., false, false),
        (false, 0., true, true),
    ] {
        let mut battle = battle(crate::tests::attack(0));
        battle.actors[0].position[1] = 20.;
        battle.actors[0].movement.flying = flying;
        battle.actors[0].movement.hover_height = hover_height;
        battle.actors[0].movement.fixed_height = fixed_height;
        battle.step(start()).unwrap();
        assert_eq!(
            battle
                .step(BattleInput::default())
                .unwrap()
                .actions
                .is_empty(),
            completes
        );
    }
}
