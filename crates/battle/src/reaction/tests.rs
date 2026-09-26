use super::*;
use crate::tests::{actor, prepared};
use crate::{
    ActionPhase, ActionRequest, ActorId, Battle, BattleInput, Cue, PreparedBattle, Rejection, Side,
};
use serde_json::Value;
use std::sync::Arc;

fn number(value: &Value) -> f32 {
    f32::from_bits(value.as_u64().unwrap() as u32)
}
fn vector(value: &Value) -> [f32; 3] {
    std::array::from_fn(|i| number(&value[i]))
}
fn battle(actor: Actor) -> Battle {
    Battle::new(Arc::new(
        PreparedBattle::new(
            vec![actor, crate::tests::actor(Side::Enemy)],
            vec![],
            0,
            vec![],
            vec![],
        )
        .unwrap(),
    ))
}

#[test]
fn ordinary_reset_stops_jitter_without_discarding_the_pending_model_sample() {
    let mut actor = actor(Side::Party);
    let mut random = crate::Random::from_state(1);
    actor.body.jitter.request(8);
    actor.body.jitter.advance(&mut random);
    recover(&mut actor, &mut random, &mut crate::Ledger::new(1, 0));
    assert!(!actor.body.jitter.active);
    assert_eq!(actor.body.jitter.remaining, 7);
    assert_eq!(actor.body.jitter.take_acceleration()[1], 0.2);
    let after_reset = random.state();
    actor.body.jitter.advance(&mut random);
    assert_eq!(random.state(), after_reset);
}

#[test]
fn natural_contacts_match_original_direction_hurt_and_guard_entry() {
    let trace: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/opening-contact-entry.json"
    ))
    .unwrap();
    for row in trace["observations"].as_array().unwrap() {
        let before = &row["before"];
        let after = &row["after"];
        let mut actor = actor(Side::Enemy);
        actor.hp = row["wrapper"]["hp"].as_i64().unwrap() as i32;
        actor.position = vector(&before["position_bits"]);
        actor.activity = if before["activity"] == 11 {
            Activity::Guarding
        } else {
            Activity::Idle
        };
        actor.hit_stop = before["hit_stop"].as_u64().unwrap() as u8;
        actor.guard.auto_chance = before["auto_guard_chance"].as_u64().unwrap() as u8;
        let velocity = &before["velocity_bits"];
        actor.movement.forward = number(&velocity[0]);
        actor.movement.vertical = number(&velocity[1]);
        actor.movement.acceleration = number(&velocity[2]);
        actor.movement.gravity = number(&velocity[3]);
        actor.movement.braking = number(&before["braking_bits"]);
        actor.movement.flying = row["profile_flags"].as_u64().unwrap() & 1 != 0;
        actor.reaction.combo_hits = before["combo_hits"].as_i64().unwrap() as i32;
        actor.reaction.combo_damage = before["combo_damage"].as_i64().unwrap() as i32;
        actor.reaction.remaining = before["hitstun"].as_i64().unwrap() as i16;
        actor.reaction.direction = vector(&before["direction_bits"]);
        actor.reaction.profile.vertical = match row["weight"].as_u64().unwrap() {
            1 => crate::VerticalRecoil::Scale(
                trace["parameters"]["light_vertical_scale"]
                    .as_f64()
                    .unwrap() as f32,
            ),
            2 => crate::VerticalRecoil::Scale(
                trace["parameters"]["heavy_vertical_scale"]
                    .as_f64()
                    .unwrap() as f32,
            ),
            3 => crate::VerticalRecoil::Grounded,
            _ => crate::VerticalRecoil::Unchanged,
        };
        let impulse = &trace["parameters"]["impulses"][row["selector"].as_u64().unwrap() as usize];
        let flags = row["flags"].as_u64().unwrap();
        let rule = ReactionRule {
            hitstun: row["hitstun"].as_u64().unwrap() as u8,
            alternate_motion: flags & 2 != 0,
            armor_damage: 0,
            stun_chance: 0,
            stagger: 0,
            hits_down: false,
            recoil: crate::RecoilRule {
                impulse: std::array::from_fn(|i| impulse[i].as_f64().unwrap() as f32),
                delay: row["delay"].as_u64().unwrap() as u8,
                guard_speed: trace["parameters"]["guard_speed"].as_f64().unwrap() as f32,
                lift_guard: flags & 0x100 != 0,
                ..Default::default()
            },
            direction: match row["direction_mode"].as_u64().unwrap() {
                0 => RecoilDirection::Travel,
                1 => RecoilDirection::AwayFromOwner,
                2 => RecoilDirection::AwayFromContact,
                3 => RecoilDirection::TowardContact,
                _ => RecoilDirection::None,
            },
        };
        let direction = rule.direction(
            vector(&row["owner_position_bits"]),
            actor.position,
            vector(&row["contact_position_bits"]),
            vector(&row["incoming_direction_bits"]),
        );
        assert_eq!(
            direction.map(f32::to_bits),
            vector(&row["recoil_direction_bits"]).map(f32::to_bits),
            "direction at {}",
            row["index"]
        );
        let flags = row["result"].as_u64().unwrap();
        let guard = if flags & 0x400 != 0 {
            crate::GuardResult::Broken
        } else if flags & 0x10 != 0 {
            crate::GuardResult::Blocked {
                first: flags & 0x8000 != 0,
                special: false,
            }
        } else {
            crate::GuardResult::None
        };
        let hit = crate::HitResult {
            amount: row["amount"].as_i64().unwrap() as i32,
            hp_change: 0,
            critical: false,
            affinity: crate::Affinity::Normal,
            guard,
            auto_guard: false,
            armored: false,
            protection: crate::HitProtection::None,
        };
        actor.body.jitter.request(8);
        assert!(respond(&mut actor, rule, hit, direction).is_some());
        // 2A710 clears the jitter flag; the ordinary guard entry does not.
        assert_eq!(
            actor.body.jitter.active,
            actor.activity == Activity::Guarding
        );
        assert_eq!(
            actor.activity,
            if after["activity"] == 9 {
                Activity::Hurt
            } else {
                Activity::Guarding
            }
        );
        assert_eq!(
            actor.reaction.remaining,
            after["hitstun"].as_i64().unwrap() as i16
        );
        assert_eq!(
            actor.reaction.combo_hits,
            after["combo_hits"].as_i64().unwrap() as i32
        );
        assert_eq!(
            actor.reaction.combo_damage,
            after["combo_damage"].as_i64().unwrap() as i32
        );
        assert_eq!(actor.hit_stop, after["hit_stop"].as_u64().unwrap() as u8);
        assert_eq!(
            actor.guard.auto_chance,
            after["auto_guard_chance"].as_u64().unwrap() as u8
        );
        assert_eq!(
            actor.reaction.direction.map(f32::to_bits),
            vector(&after["direction_bits"]).map(f32::to_bits)
        );
        assert_eq!(
            [
                actor.movement.forward,
                actor.movement.vertical,
                actor.movement.acceleration,
                actor.movement.gravity
            ]
            .map(f32::to_bits),
            std::array::from_fn(|i| number(&after["velocity_bits"][i]).to_bits()),
            "velocity at {}",
            row["index"]
        );
        assert_eq!(
            actor.movement.braking.to_bits(),
            number(&after["braking_bits"]).to_bits()
        );
        assert_eq!(
            actor.reaction.recoil.pending.map(f32::to_bits),
            std::array::from_fn(|i| number(&after["pending_bits"][i]).to_bits())
        );
        assert_eq!(
            actor.reaction.recoil.delay,
            after["delay"].as_u64().unwrap() as u8
        );
    }
}

#[test]
fn actor_dispatch_matches_original_hurt_and_guard_recovery_visits() {
    for (source, activity, recovered_activity, expected_recoveries) in [
        (
            include_str!("../../tests/fixtures/opening-hurt.json"),
            Activity::Hurt,
            2,
            4,
        ),
        (
            include_str!("../../tests/fixtures/opening-guard-update.json"),
            Activity::Guarding,
            3,
            2,
        ),
    ] {
        let trace: Value = serde_json::from_str(source).unwrap();
        let mut recoveries = 0;
        for row in trace["observations"].as_array().unwrap() {
            let before = &row["before"];
            let after = &row["after"];
            let mut actor = actor(Side::Party);
            actor.control = match row["control"].as_str().unwrap() {
                "auto" => Control::Auto,
                "semi_auto" => Control::SemiAuto,
                "manual" => Control::Manual,
                "enemy" => Control::Enemy,
                _ => unreachable!(),
            };
            actor.activity = activity;
            if activity == Activity::Guarding {
                actor.guard.active = before["guard_state"].as_u64().unwrap() & 1 != 0;
            }
            actor.position = vector(&before["position_bits"]);
            let velocity = &before["velocity_bits"];
            actor.movement = crate::Movement {
                previous_position: vector(&before["origin_bits"]),
                previous_velocity: [number(&velocity[0]), number(&velocity[1])],
                forward: number(&velocity[2]),
                vertical: number(&velocity[3]),
                acceleration: number(&velocity[4]),
                gravity: number(&velocity[5]),
                braking: number(&before["braking_bits"]),
                flying: row["flying"].as_bool().unwrap(),
                fixed_height: row["fixed_height"].as_bool().unwrap(),
                ..Default::default()
            };
            actor.hit_stop = before["hit_stop"].as_u64().unwrap() as u8;
            actor.guard.auto_chance = before["auto_guard_chance"].as_u64().unwrap() as u8;
            actor.guard.recovery_bonus = row["guard_recovery_bonus"].as_u64().unwrap() as u8;
            actor.reaction = Reaction {
                stun: Default::default(),
                stagger: Default::default(),
                protection: Default::default(),
                armor: Default::default(),
                profile: Default::default(),
                unflinching: false,
                direction: vector(&row["direction_bits"]),
                recoil: Recoil {
                    delay: before["delay"].as_u64().unwrap() as u8,
                    pending: [
                        number(&before["pending_bits"][0]),
                        number(&before["pending_bits"][1]),
                    ],
                    ..Default::default()
                },
                remaining: before["hitstun"].as_i64().unwrap() as i16,
                combo_hits: before["combo_hits"].as_i64().unwrap() as i32,
                combo_damage: before["combo_damage"].as_i64().unwrap() as i32,
                normal_history: 0,
                recover_in_air: row["recover_in_air"].as_bool().unwrap(),
                idle_initialization: None,
            };
            let mut battle = battle(actor);
            battle.random.0 = row["random_before"].as_u64().unwrap() as u32;
            let frame = battle.step(BattleInput::default()).unwrap();
            let actor = &frame.actors[0];
            assert_eq!(
                actor.position.map(f32::to_bits),
                vector(&after["position_bits"]).map(f32::to_bits),
                "position at {}",
                row["index"]
            );
            assert_eq!(
                actor.movement.previous_position.map(f32::to_bits),
                vector(&after["origin_bits"]).map(f32::to_bits)
            );
            let actual = [
                actor.movement.previous_velocity[0],
                actor.movement.previous_velocity[1],
                actor.movement.forward,
                actor.movement.vertical,
                actor.movement.acceleration,
                actor.movement.gravity,
            ];
            assert_eq!(
                actual.map(f32::to_bits),
                std::array::from_fn(|i| number(&after["velocity_bits"][i]).to_bits()),
                "velocity at {}",
                row["index"]
            );
            assert_eq!(
                actor.movement.braking.to_bits(),
                number(&after["braking_bits"]).to_bits()
            );
            assert_eq!(
                actor.reaction.remaining,
                after["hitstun"].as_i64().unwrap() as i16
            );
            assert_eq!(
                actor.reaction.combo_hits,
                after["combo_hits"].as_i64().unwrap() as i32
            );
            assert_eq!(
                actor.reaction.combo_damage,
                after["combo_damage"].as_i64().unwrap() as i32
            );
            assert_eq!(
                actor.reaction.recoil.delay,
                after["delay"].as_u64().unwrap() as u8
            );
            assert_eq!(
                actor.reaction.recoil.pending.map(f32::to_bits),
                std::array::from_fn(|i| number(&after["pending_bits"][i]).to_bits())
            );
            assert_eq!(
                battle.random.0,
                row["random_after"].as_u64().unwrap() as u32
            );
            assert_eq!(
                actor.guard.auto_chance,
                after["auto_guard_chance"].as_u64().unwrap() as u8
            );
            if activity == Activity::Guarding {
                assert_eq!(
                    actor.guard.active,
                    after["guard_state"].as_u64().unwrap() & 1 != 0
                );
            }
            let recovered = after["activity"].as_u64().unwrap() == recovered_activity;
            assert_eq!(
                actor.activity,
                if recovered { Activity::Idle } else { activity }
            );
            recoveries += usize::from(recovered);
        }
        assert_eq!(recoveries, expected_recoveries);
    }
}

#[test]
fn ordinary_guard_holds_during_hit_stop_and_recovers_before_integration() {
    let mut actor = actor(Side::Party);
    actor.control = Control::Auto;
    actor.activity = Activity::Guarding;
    actor.guard.pressure = 7;
    actor.hit_stop = 2;
    actor.reaction.remaining = 1;
    actor.reaction.direction = [1., 0., 0.];
    actor.reaction.recoil.delay = 2;
    actor.movement.forward = 3.;
    let mut battle = battle(actor);
    let initial = battle.actors[0].clone();
    let paused = battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(paused.actors[0], initial);
    for remaining_stop in [1, 0] {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.actors[0].hit_stop, remaining_stop);
        assert_eq!(frame.actors[0].reaction.remaining, 1);
        assert_eq!(frame.actors[0].movement.forward, 3.);
        assert_eq!(frame.actors[0].position, [0.; 3]);
        assert!(frame.actors[0].guard.active);
        assert_eq!(battle.random_state(), 0);
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.actors[0].reaction.remaining, 0);
    assert_eq!(frame.actors[0].reaction.recoil.delay, 2);
    assert_eq!(frame.actors[0].position, [3., 0., 0.]);
    assert_eq!(frame.actors[0].movement.forward, 2.45);
    assert_eq!(frame.actors[0].activity, Activity::Guarding);
    let frame = battle.step(BattleInput::default()).unwrap();
    let actor = &frame.actors[0];
    assert_eq!(actor.activity, Activity::Idle);
    assert!(!actor.guard.active);
    assert_eq!(actor.guard.pressure, 0);
    assert_eq!(actor.reaction.remaining, 0);
    assert_eq!(actor.position, [3., 0., 0.]);
    assert_eq!(actor.movement.previous_velocity, [0.; 2]);
    assert_eq!(actor.movement.vertical, -1.);
    assert_eq!(battle.random_state(), 0x12_d687);
}

#[test]
fn delay_releases_during_hit_stop_but_menu_holds_every_clock() {
    let mut actor = actor(Side::Party);
    actor.activity = Activity::Hurt;
    actor.hit_stop = 2;
    actor.reaction.remaining = 7;
    actor.reaction.direction = [0., 0., -1.];
    actor.reaction.recoil = Recoil {
        pending: [4., 2.],
        delay: 2,
        ..Default::default()
    };
    let mut battle = battle(actor);
    let first = battle.step(BattleInput::default()).unwrap();
    assert_eq!(first.actors[0].reaction.recoil.delay, 1);
    assert_eq!(first.actors[0].reaction.remaining, 6);
    let paused = battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(paused.actors, first.actors);
    let second = battle.step(BattleInput::default()).unwrap();
    assert_eq!(second.actors[0].position, [0.; 3]);
    assert_eq!(second.actors[0].movement.forward, 4.);
    assert_eq!(second.actors[0].movement.vertical, 2.);
    assert_eq!(second.actors[0].reaction.remaining, 5);
    let third = battle.step(BattleInput::default()).unwrap();
    assert_eq!(third.actors[0].position, [0., 2., -4.]);
    assert_eq!(third.actors[0].reaction.remaining, 4);
}

#[test]
fn hurt_waits_for_landing_and_recovery_decrements_the_new_signed_clock() {
    for (control, remaining) in [
        (Control::Manual, 29),
        (Control::Enemy, -1),
        (Control::Auto, -1),
        (Control::SemiAuto, 29),
    ] {
        let mut actor = actor(Side::Party);
        actor.control = control;
        actor.activity = Activity::Hurt;
        actor.position[1] = 1.;
        actor.movement.gravity = -1.;
        let mut battle = battle(actor);
        assert_eq!(
            battle.step(BattleInput::default()).unwrap().actors[0].activity,
            Activity::Hurt
        );
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.actors[0].activity, Activity::Idle);
        assert_eq!(frame.actors[0].reaction.remaining, remaining);
        assert_eq!(frame.actors[0].position[1], 0.);
        assert_eq!(frame.actors[0].movement.gravity, -1.);
    }
    let mut actor = actor(Side::Party);
    actor.activity = Activity::Hurt;
    actor.position[1] = 40.;
    actor.movement.flying = true;
    actor.reaction.recover_in_air = true;
    let frame = battle(actor).step(BattleInput::default()).unwrap();
    assert_eq!(frame.actors[0].activity, Activity::Idle);
    assert_eq!(
        frame.actors[0].movement.gravity.to_bits(),
        (-0.0_f32).to_bits()
    );
}

#[test]
fn hitstun_uses_the_previous_combo_and_guard_break_keeps_its_own_clock() {
    let hit = crate::HitResult {
        amount: 10,
        hp_change: -10,
        critical: false,
        affinity: crate::Affinity::Normal,
        guard: crate::GuardResult::None,
        auto_guard: false,
        armored: false,
        protection: crate::HitProtection::None,
    };
    let rule = ReactionRule {
        hitstun: 100,
        ..Default::default()
    };
    for (previous, remaining) in [(0, 100), (1, 100), (2, 99), (100, 50), (200, 2)] {
        let mut target = actor(Side::Enemy);
        target.reaction.combo_hits = previous;
        target.reaction.combo_damage = 20;
        assert_eq!(
            respond(&mut target, rule, hit, [0.; 3]),
            Some(ContactReaction::Hurt { alternate: false })
        );
        assert_eq!(target.reaction.remaining, remaining);
        assert_eq!(target.reaction.combo_hits, previous + 1);
        assert_eq!(target.reaction.combo_damage, 30);
    }
    let mut target = actor(Side::Enemy);
    target.hit_stop = 4;
    let broken = crate::HitResult {
        guard: crate::GuardResult::Broken,
        ..hit
    };
    assert_eq!(
        respond(&mut target, rule, broken, [0.; 3]),
        Some(ContactReaction::Hurt { alternate: true })
    );
    assert_eq!(target.reaction.remaining, 45);
    assert_eq!(target.reaction.combo_hits, 0);
    assert_eq!(target.hit_stop, 0);
    for affinity in [crate::Affinity::Absorb, crate::Affinity::Immune] {
        let before = target.clone();
        assert_eq!(
            respond(
                &mut target,
                rule,
                crate::HitResult { affinity, ..hit },
                [0.; 3]
            ),
            None
        );
        assert_eq!(target, before);
    }
}

#[test]
fn guard_recovery_rejects_new_actor_actions_without_charging_tp() {
    let mut defender = actor(Side::Party);
    defender.control = Control::Auto;
    defender.activity = Activity::Guarding;
    defender.reaction.remaining = 1;
    let mut prepared = prepared(
        "pub task run() { battle::pay_tp(battle::tp_cost()); battle::finish(); }",
        vec![defender, actor(Side::Enemy)],
        2,
    );
    let action = &mut Arc::get_mut(&mut prepared).unwrap().actions[0];
    action.phase = ActionPhase::Actor;
    action.tp_cost = 5;
    let mut battle = Battle::new(prepared);
    let tp = battle.actors[0].tp;
    let request = || BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: 99,
            target: ActorId(1),
        }],
        ..Default::default()
    };
    for activity in [Activity::Guarding, Activity::Idle] {
        let frame = battle.step(request()).unwrap();
        assert_eq!(frame.actors[0].tp, tp);
        assert_eq!(frame.actors[0].activity, activity);
        assert_eq!(
            frame.cues,
            vec![Cue::Rejected {
                actor: ActorId(0),
                reason: Rejection::Busy
            }]
        );
    }
    let frame = battle.step(request()).unwrap();
    assert_eq!(frame.actors[0].tp, tp - 5);
    assert!(
        frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Started { .. }))
    );
}

#[test]
fn hurt_interrupts_actor_tasks_and_rejects_commands_without_spending_tp() {
    let source = "pub task run() { spawn late(); await battle::at_age(ticks(20)); } task late() { await battle::at_age(ticks(2)); battle::heal_percent(battle::owner(), 50); }";
    let mut prepared = prepared(source, vec![actor(Side::Party), actor(Side::Enemy)], 30);
    Arc::get_mut(&mut prepared).unwrap().actions[0].phase = ActionPhase::Actor;
    let mut battle = Battle::new(prepared);
    let input = || BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: 99,
            target: ActorId(1),
        }],
        ..Default::default()
    };
    let started = battle.step(input()).unwrap();
    let id = started.actions[0].0;
    battle.actors[0].activity = Activity::Hurt;
    battle.actors[0].reaction.remaining = 4;
    let interrupted = battle.step(input()).unwrap();
    assert_eq!(
        interrupted.cues,
        vec![
            Cue::Rejected {
                actor: ActorId(0),
                reason: Rejection::Busy
            },
            Cue::Interrupted { action: id }
        ]
    );
    assert_eq!(interrupted.actors[0].tp, started.actors[0].tp);
    assert!(interrupted.actions.is_empty());
    for _ in 0..5 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.actors[0].hp, started.actors[0].hp);
        assert!(
            !frame
                .cues
                .iter()
                .any(|c| matches!(c, Cue::Completed { .. } | Cue::Recovered { .. }))
        );
    }
}
