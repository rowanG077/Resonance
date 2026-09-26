use super::*;
use crate::{
    ActionPhase, ActionRequest, ActorId, Battle, BattleInput, Cue, Side,
    tests::{actor, prepared},
};
use std::sync::Arc;

fn battle(source: &str, duration: u16) -> Battle {
    let mut owner = actor(Side::Party);
    owner.movement.direction = [1., 0., 0.];
    let mut prepared = prepared(source, vec![owner, actor(Side::Enemy)], duration);
    Arc::get_mut(&mut prepared).unwrap().actions[0].phase = ActionPhase::Actor;
    Battle::new(prepared)
}

fn start() -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: 99,
            target: ActorId(1),
        }],
        ..Default::default()
    }
}

#[test]
fn integration_observes_old_velocity_before_acceleration_and_does_not_move_fixed_height() {
    let mut movement = Movement {
        direction: [0.6, 9., 0.8],
        forward: 5.,
        vertical: 3.,
        acceleration: -1.,
        gravity: -2.,
        fixed_height: true,
        ..Default::default()
    };
    let mut position = [7., 20., 4.];
    movement.integrate(&mut position, [2., -2.]);
    assert_eq!(position, [12., 20., 6.]);
    assert_eq!(movement.previous_position, [7., 0., 4.]);
    assert_eq!(movement.previous_velocity, [5., 3.]);
    assert_eq!((movement.forward, movement.vertical), (4., 1.));
}

#[test]
fn braking_preserves_sequential_checks_and_flying_hurt_exception() {
    let mut movement = Movement {
        forward: 1.,
        braking: 3.,
        ..Default::default()
    };
    assert!(!movement.brake(0., Activity::Idle, false, false));
    assert_eq!(movement.forward, 1.); // 1 - 3 + 3, not a sign clamp.
    movement.forward = 0.55;
    assert!(movement.brake(50., Activity::Idle, false, false));
    assert_eq!(movement.forward, 0.);
    movement.flying = true;
    movement.forward = 0.5;
    assert!(!movement.brake(1., Activity::Hurt, false, false));
    assert_eq!(movement.forward, 0.5);
    assert!(!movement.brake(1., Activity::Idle, false, true));
    assert_eq!(movement.forward, 0.);
    movement.flying = false;
    movement.forward = 10.;
    assert!(movement.brake(50., Activity::Idle, true, false));
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
fn script_commands_precede_integration_and_minimum_speed_does_not_reduce_momentum() {
    let mut battle = battle(
        "pub task run() { battle::forward_speed(6.0, true); battle::acceleration(-1.0); battle::vertical_speed(3.0); battle::gravity(-2.0); await battle::at_age(ticks(2)); battle::forward_speed(-2.0, false); }",
        5,
    );
    battle.actors[0].movement.forward = 8.;
    let first = battle.step(start()).unwrap();
    assert_eq!(first.actors[0].position, [8., 3., 0.]);
    assert_eq!(first.actors[0].movement.forward, 7.);
    battle.step(BattleInput::default()).unwrap();
    let third = battle.step(BattleInput::default()).unwrap();
    assert_eq!(third.actors[0].position, [13., 3., 0.]);
    assert_eq!(third.actors[0].movement.forward, -3.);
}

#[test]
fn local_hit_stop_holds_actor_commands_and_motion_but_ticks_common_timers() {
    let mut battle = battle("pub task run() { battle::forward_speed(3.0, false); }", 10);
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
}

#[test]
fn recovery_is_independent_of_action_age_and_continues_under_local_stop() {
    let mut battle = battle(
        "pub task run() { spawn late(); battle::forward_speed(3.0, false); await battle::at_age(ticks(2)); await battle::recover(ticks(2)); battle::finish(); } task late() { await battle::at_age(ticks(3)); battle::heal_percent(battle::owner(), 40); }",
        2,
    );
    battle.step(start()).unwrap();
    battle.step(BattleInput::default()).unwrap();
    let entry = battle.step(BattleInput::default()).unwrap();
    assert_eq!(entry.actors[0].activity, Activity::Recovering);
    assert_eq!(entry.actions[0].2, 2);
    battle.actors[0].hit_stop = 4;
    for x in [12., 15.] {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.actors[0].position[0], x);
        assert_eq!(frame.actions[0].2, 2);
        assert!(
            !frame
                .cues
                .iter()
                .any(|c| matches!(c, Cue::Recovered { .. }))
        );
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    // 301A4 calls the 2B18C velocity reset before its final 244D0 integration.
    // Recovery moves under local stop, but completion adds no extra displacement.
    assert_eq!(frame.actors[0].position[0], 15.);
    assert_eq!(frame.actors[0].movement.forward, 0.);
    assert_eq!(frame.actors[0].movement.previous_velocity[0], 0.);
    assert_eq!(frame.actors[0].activity, Activity::Idle);
    assert!(frame.actions.is_empty());
    assert!(
        frame
            .cues
            .iter()
            .any(|c| matches!(c, Cue::Completed { .. }))
    );
}

#[test]
fn airborne_recovery_waits_for_the_floor_and_menu_pause_preserves_the_countdown() {
    let mut battle = battle(
        "pub task run() { await battle::recover(ticks(0)); battle::finish(); }",
        0,
    );
    battle.actors[0].position[1] = 2.;
    battle.step(start()).unwrap();
    let before = battle.actors[0].clone();
    let paused = battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(paused.actors[0], before);
    assert!(
        !battle
            .step(BattleInput::default())
            .unwrap()
            .actions
            .is_empty()
    );
    assert!(
        !battle
            .step(BattleInput::default())
            .unwrap()
            .actions
            .is_empty()
    );
    assert!(
        battle
            .step(BattleInput::default())
            .unwrap()
            .actions
            .is_empty()
    );
}

#[test]
fn preparation_and_native_calls_reject_invalid_movement() {
    let mut owner = actor(Side::Party);
    owner.movement.braking = -1.;
    assert!(owner.validate().is_err());
    let mut battle = battle(
        "pub task run() { battle::forward_speed(1.0 / 0.0, false); }",
        2,
    );
    assert!(battle.step(start()).is_err());
    assert!(battle.sequences.is_empty());
}

#[test]
fn original_dolphin_movement_calls_match_bit_for_bit() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/opening-movement.json")).unwrap();
    let f = |v: &serde_json::Value| f32::from_bits(v.as_u64().unwrap() as u32);
    let vector = |v: &serde_json::Value| std::array::from_fn(|i| f(&v[i]));
    let rows = fixture["observations"].as_array().unwrap();
    assert_eq!(rows.len(), 2048);
    for row in rows {
        let before = &row["before"];
        let velocity = &before["velocity_bits"];
        let mut movement = Movement {
            direction: if row["direction_bits"].is_null() {
                [0.; 3]
            } else {
                vector(&row["direction_bits"])
            },
            forward: f(&velocity[2]),
            vertical: f(&velocity[3]),
            acceleration: f(&velocity[4]),
            gravity: f(&velocity[5]),
            previous_position: vector(&before["origin_bits"]),
            previous_velocity: [f(&velocity[0]), f(&velocity[1])],
            braking: f(&row["braking_bits"]),
            flying: row["flying"].as_bool().unwrap(),
            fixed_height: row["fixed_height"].as_bool().unwrap(),
            hover_height: 0.,
            steering: Default::default(),
            ..Default::default()
        };
        let mut position = vector(&before["position_bits"]);
        if row["function"] != "brake" {
            movement.integrate(&mut position, [0.; 2]);
        }
        if row["function"] != "integrate" {
            let activity = if row["activity"] == 9 {
                Activity::Hurt
            } else {
                Activity::Idle
            };
            let result = movement.brake(
                position[1],
                activity,
                row["stop_reposition"].as_bool().unwrap(),
                row["unfinished_stop_motion"].as_bool().unwrap(),
            );
            assert_eq!(
                u64::from(result),
                row["result"].as_u64().unwrap(),
                "call {}",
                row["index"]
            );
        }
        let after = &row["after"];
        for (actual, expected) in [
            (position.to_vec(), &after["position_bits"]),
            (movement.previous_position.to_vec(), &after["origin_bits"]),
            (
                vec![
                    movement.previous_velocity[0],
                    movement.previous_velocity[1],
                    movement.forward,
                    movement.vertical,
                    movement.acceleration,
                    movement.gravity,
                ],
                &after["velocity_bits"],
            ),
        ] {
            for (i, value) in actual.iter().enumerate() {
                assert_eq!(
                    value.to_bits(),
                    expected[i].as_u64().unwrap() as u32,
                    "call {}, component {}",
                    row["index"],
                    i
                );
            }
        }
    }
}

#[test]
fn recovery_airborne_exit_depends_on_hover_height_not_flying_flag() {
    for (flying, hover_height, fixed_height, completes) in [
        (true, 0., false, false),
        (false, 20., false, true),
        (false, 0., true, true),
    ] {
        let mut battle = battle(
            "pub task run() { await battle::recover(ticks(0)); battle::finish(); }",
            0,
        );
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
