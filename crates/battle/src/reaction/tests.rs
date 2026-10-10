use super::*;
use crate::tests::{actor, prepared};
use crate::{ActionRequest, ActorId, Battle, BattleInput, Cue, PreparedBattle, Rejection, Side};
use std::sync::Arc;

fn battle(actor: Actor) -> Battle {
    PreparedBattle::new(
        vec![
            (actor, Default::default()),
            (crate::tests::actor(Side::Enemy), Default::default()),
        ],
        Default::default(),
        0,
    )
    .unwrap()
    .finish()
    .unwrap()
}

#[test]
fn hurt_recovery_waits_for_movement_and_landing() {
    for flying in [false, true] {
        let mut owner = actor(Side::Party);
        owner.movement.flying = flying;
        owner.reaction.recover_in_air = flying;
        let mut battle = battle(owner);
        battle.begin_hurt(ActorId(0), 3, &mut vec![]);
        battle.actors[0].position[1] = 20.;
        battle.actors[0].movement.gravity = if flying { 0. } else { crate::movement::GRAVITY };
        battle.actors[0].hit_stop = 2;
        let before = battle.actors[0].clone();
        for _ in 0..10 {
            battle
                .step(BattleInput {
                    paused: true,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(battle.actors[0], before);
        }
        for _ in 0..2 {
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.actors[0].position, before.position);
            assert_eq!(battle.activity(ActorId(0)), Activity::Hurt);
        }
        for _ in 0..30 {
            battle.step(BattleInput::default()).unwrap();
            if battle.activity(ActorId(0)) == Activity::Idle {
                break;
            }
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert_eq!(battle.actors[0].hp, before.hp);
        assert_eq!(battle.actors[0].position[1] > 0., flying);
    }
}

#[test]
fn hurt_interrupts_actor_tasks_and_rejects_commands_without_spending_tp() {
    let mut prepared = prepared(vec![actor(Side::Party), actor(Side::Enemy)], 30);
    crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0])).events =
        vec![(2, crate::AttackEvent::Sound(Some(crate::Sound::Stream(1))))];
    let mut battle = prepared.finish().unwrap();
    let input = || BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: crate::ActionKey(0),
            target: ActorId(1),
        }],
        ..Default::default()
    };
    let started = battle.step(input()).unwrap();
    let id = started.actions[0].0;
    let mut cues = vec![];
    battle.begin_hurt(ActorId(0), 4, &mut cues);
    assert_eq!(cues, vec![Cue::Interrupted { action: id }]);
    let interrupted = battle.step(input()).unwrap();
    assert_eq!(
        interrupted.cues,
        vec![Cue::Rejected {
            actor: ActorId(0),
            reason: Rejection::Busy
        }]
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
                .any(|c| matches!(c, Cue::Completed { .. } | Cue::Sound { .. }))
        );
    }
}

fn suppression_battle() -> Battle {
    let mut actors = [Side::Party, Side::Party, Side::Enemy, Side::Enemy].map(actor);
    actors[0].control = Control::SemiAuto;
    actors[1].control = Control::Auto;
    let mut battle = prepared(actors.into(), 1).finish().unwrap();
    battle.runtime[0].target = ActorId(2);
    battle.actors[2].position = [100., 0., 0.];
    battle.actors[3].position = [100., 0., 0.];
    battle
}

#[test]
fn leader_recoil_suppression_uses_live_planar_distance_and_strict_range() {
    let mut battle = suppression_battle();
    for (distance, expected) in [(399.9999, true), (400., false), (400.0001, false)] {
        battle.actors[2].position = [distance, 1000., 0.];
        assert_eq!(
            battle.suppress_recoil(ActorId(1), ActorId(2), 400.),
            expected
        );
    }
    battle.actors[2].position = [0.; 3];
    assert!(!battle.suppress_recoil(ActorId(1), ActorId(2), 0.));
}

#[test]
fn leader_recoil_suppression_preserves_control_target_and_current_kind_gates() {
    let mut battle = suppression_battle();
    assert!(battle.suppress_recoil(ActorId(1), ActorId(2), 400.));
    assert!(
        !battle.suppress_recoil(ActorId(0), ActorId(2), 400.),
        "leader's own hit"
    );
    assert!(
        !battle.suppress_recoil(ActorId(1), ActorId(3), 400.),
        "unselected target"
    );
    assert!(
        !battle.suppress_recoil(ActorId(2), ActorId(0), 400.),
        "selected enemy hits leader"
    );
    assert!(
        battle.suppress_recoil(ActorId(3), ActorId(0), 400.),
        "unselected enemy hits leader"
    );
    for kind in [RecoilKind::Down, RecoilKind::Launched] {
        battle.actors[2].reaction.recoil.kind = kind;
        assert!(!battle.suppress_recoil(ActorId(1), ActorId(2), 400.));
    }
    battle.actors[2].reaction.recoil.kind = RecoilKind::Normal;
    for availability in [
        crate::ActorAvailability::Absent,
        crate::ActorAvailability::Dead,
        crate::ActorAvailability::Petrified,
    ] {
        battle.actors[0].availability = availability;
        battle.actors[1].control = Control::Manual;
        battle.runtime[1].target = ActorId(2);
        assert!(
            !battle.suppress_recoil(ActorId(0), ActorId(2), 400.),
            "do not skip unavailable leader"
        );
    }
    battle.actors[0].availability = crate::ActorAvailability::Active;
    battle.actors[0].control = Control::Auto;
    assert!(
        battle.suppress_recoil(ActorId(0), ActorId(2), 400.),
        "next non-auto member leads"
    );
    battle.actors[1].control = Control::Auto;
    assert!(
        !battle.suppress_recoil(ActorId(1), ActorId(2), 400.),
        "all auto"
    );
    for actor in &mut battle.actors {
        actor.side = Side::Enemy;
    }
    battle.actors[0].control = Control::Manual;
    assert!(
        !battle.suppress_recoil(ActorId(1), ActorId(2), 400.),
        "no party leader"
    );
}

#[test]
fn recoil_suppression_tracks_the_live_target() -> Result<()> {
    let mut battle = suppression_battle();
    assert!(battle.suppress_recoil(ActorId(1), ActorId(2), 400.));
    battle.set_decision_target(ActorId(0), ActorId(3))?;
    assert_eq!(battle.target(ActorId(0)), Some(ActorId(3)));
    assert!(!battle.suppress_recoil(ActorId(1), ActorId(2), 400.));
    assert!(battle.suppress_recoil(ActorId(1), ActorId(3), 400.));
    Ok(())
}

#[test]
fn lethal_damage_preserves_leader_suppression_operands_until_response() {
    let mut battle = suppression_battle();
    battle.actors[0].hp = 1;
    let previous_direction = [0., 0., -1.];
    battle.actors[0].reaction.direction = previous_direction;
    let rule = crate::HitRule {
        overlimit_pause: true,
        kind: crate::DamageKind::Slash,
        arte: true,
        power: crate::Power::Fixed(25),
        element: crate::HitElement::Neutral,
        prevents_defeat: false,
        reaction: ReactionRule {
            recoil: crate::RecoilRule {
                impulse: [4., 0.],
                suppression_distance: 400.,
                ..Default::default()
            },
            ..Default::default()
        },
        guard: Default::default(),
        condition: None,
    };
    let suppressed = battle.suppress_recoil(ActorId(3), ActorId(0), 400.);
    assert!(suppressed);
    let [owner, target] = battle.actors.get_disjoint_mut([3, 0]).unwrap();
    let hit = crate::damage::resolve(
        owner,
        target,
        rule,
        100,
        [1., 0., 0.],
        &mut |_| battle.random.next_u16(),
        false,
    );
    assert_eq!(battle.actors[0].hp, 0);
    assert!(
        battle.actors[0].available(),
        "death admission is in the later contact tail"
    );
    assert_eq!(battle.actors[0].reaction.recoil.kind, RecoilKind::Normal);
    assert_eq!(
        battle.suppress_recoil(ActorId(3), ActorId(0), 400.),
        suppressed
    );
    assert_eq!(
        respond(
            &crate::tests::actor(Side::Enemy),
            &mut battle.actors[0],
            rule.reaction,
            hit,
            [1., 0., 0.],
            suppressed
        ),
        None
    );
    assert_eq!(battle.actors[0].movement.forward, 0.);
    assert_eq!(battle.actors[0].reaction.direction, previous_direction);
}

#[test]
fn frozen_targets_take_no_guard_or_hurt_reaction() {
    for guard in [
        crate::GuardResult::None,
        crate::GuardResult::Blocked {
            first: true,
            special: false,
        },
        crate::GuardResult::Broken,
    ] {
        let mut target = actor(Side::Enemy);
        target.time_stop = 30;
        let hit = crate::HitResult {
            amount: 1,
            hp_change: -1,
            critical: false,
            boosted: false,
            affinity: crate::Affinity::Normal,
            guard,
            protection: crate::HitProtection::None,
        };
        assert_eq!(
            respond(
                &actor(Side::Party),
                &mut target,
                ReactionRule::default(),
                hit,
                [1., 0., 0.],
                false
            ),
            None
        );
        assert_eq!(target.guard.recovery, 0);
        assert_eq!(target.time_stop, 30);
    }
}

#[test]
fn recent_hurt_timer_uses_common_visits_including_hit_stop_but_excluding_pauses() {
    let mut battle = battle(actor(Side::Party));
    battle.actors[0].guard.recent_hurt_ticks = 3;
    battle.actors[0].hit_stop = 8;
    battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.actors[0].guard.recent_hurt_ticks, 3);
    battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.actors[0].guard.recent_hurt_ticks, 3);
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.actors[0].guard.recent_hurt_ticks, 2);
    battle.actors[0].availability = crate::ActorAvailability::Petrified;
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.actors[0].guard.recent_hurt_ticks, 1);
    battle.actors[0].availability = crate::ActorAvailability::Active;
    for _ in 0..3 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.actors[0].guard.recent_hurt_ticks, 0);
}
