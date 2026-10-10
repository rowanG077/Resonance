use super::*;
use crate::PreparedBattle;
use crate::{Actor, ActorAvailability, BattleInput};

fn candidate(actors: Vec<Actor>) -> PreparedBattle {
    PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| {
                (
                    actor,
                    crate::ActorSetup {
                        overlimit_gain: 15,
                        ..Default::default()
                    },
                )
            })
            .collect(),
        Default::default(),
        77,
    )
    .unwrap()
    .with_overlimit_boost(false)
}

pub(crate) fn battle(gauges: &[u16]) -> Battle {
    let actors = gauges
        .iter()
        .enumerate()
        .map(|(i, &gauge)| {
            let mut actor = crate::tests::actor(if i + 1 == gauges.len() {
                Side::Enemy
            } else {
                Side::Party
            });
            actor.overlimit = OverLimit::new(gauge).unwrap();
            actor.position[0] = i as f32 * 100.;
            actor
        })
        .collect();
    candidate(actors).finish().unwrap()
}

#[test]
fn gain_applies_the_boost_and_keeps_the_gauge_within_bounds() {
    for (gauge, gain, delta, boosted, expected) in [
        (0, 15, 1, false, 15),
        (0, 15, 1, true, 22),
        (0, 15, 10, true, 225),
        (995, 11, 1, false, 1000),
        (2, 15, -1, false, 0),
        (0, 15, -1, true, 0),
        (0, 255, 127, true, 1000),
    ] {
        let mut candidate = candidate(vec![crate::tests::actor(Side::Party)]);
        candidate.resources.actor_setup[0].overlimit_gain = gain;
        candidate.resources.overlimit_boosted_gain = boosted;
        let mut battle = candidate.finish().unwrap();
        battle.actors[0].overlimit = OverLimit::new(gauge).unwrap();
        battle.gain_overlimit(ActorId(0), delta);
        assert_eq!(
            battle.actors[0].overlimit.charge(),
            expected,
            "{gauge} {gain} {delta} {boosted}"
        );
        battle.actors[0].overlimit = OverLimit::active(750).unwrap();
        battle.gain_overlimit(ActorId(0), delta);
        assert_eq!(battle.actors[0].overlimit.remaining(), 750);
        assert_eq!(battle.actors[0].overlimit.charge(), 0);
    }
}

#[test]
fn activation_finishes_the_update_then_holds_every_actor_until_expiry() {
    let mut battle = battle(&[1000, 1000, 1000]);
    battle.actors[0].hit_stop = 7;
    battle.items.cooldown = 60;
    let entered = battle.step(BattleInput::default()).unwrap();
    assert_eq!(entered.clock, crate::BattleClock::Running);
    assert_eq!(entered.actors[0].hit_stop, 6);
    assert_eq!(entered.actors[0].overlimit.remaining(), 1000);
    assert!(!entered.actors[1].overlimit.is_active()); // One active member per side.
    assert!(entered.actors[2].overlimit.is_active()); // Both sides finish the update.
    let duration = battle.timed_hold_remaining().unwrap();
    let menu = battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(menu.clock, crate::BattleClock::Held);
    assert_eq!(battle.timed_hold_remaining(), Some(duration));
    for _ in 0..duration {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.clock, crate::BattleClock::Held);
        assert_eq!(frame.actors, entered.actors);
        assert_eq!(frame.item_cooldown, entered.item_cooldown);
    }
    assert!(battle.timed_hold_remaining().is_none());
    let resumed = battle.step(BattleInput::default()).unwrap();
    assert_eq!(resumed.clock, crate::BattleClock::Running);
    assert_eq!(resumed.actors[0].overlimit.remaining(), 998);
    assert_eq!(resumed.actors[2].overlimit.remaining(), 998);
    assert!(!resumed.actors[1].overlimit.is_active());
}

#[test]
fn extended_duration_expires_before_another_ally_can_enter_overlimit() {
    let mut candidate = candidate(vec![crate::tests::actor(Side::Party); 2]);
    candidate.actors[0].overlimit = OverLimit::new(1000).unwrap();
    candidate.actors[1].overlimit = OverLimit::new(1000).unwrap();
    candidate.resources.actor_setup[0].extended_overlimit = true;
    let mut battle = candidate.finish().unwrap();
    battle.advance_actor_common(0, &mut vec![]).unwrap();
    assert_eq!(battle.actors[0].overlimit.remaining(), 1250);
    while battle.timed_hold.is_some() {
        battle.advance_timed_hold();
    }
    battle.actors[0].overlimit = OverLimit::active(2).unwrap();
    let mut cues = vec![];
    battle.advance_actor_common(0, &mut cues).unwrap();
    assert_eq!(battle.actors[0].overlimit, OverLimit::default());
    assert!(!battle.actors[0].overlimit.is_active());

    battle.advance_actor_common(1, &mut cues).unwrap();
    assert!(battle.actors[1].overlimit.is_active());
    assert!(cues.iter().any(|c| matches!(
        c,
        Cue::OverLimitEntered {
            actor: ActorId(1),
            ..
        }
    )));
}

#[test]
fn charge_and_duration_have_distinct_bounds_and_saved_values() {
    for charge in [0, 9, 10, 999, 1000] {
        let gauge = OverLimit::new(charge).unwrap();
        assert_eq!(gauge.charge(), charge);
        assert_eq!(gauge.remaining(), 0);
        assert_eq!(gauge.saved_percent(), (charge / 10) as u8);
    }
    for invalid in [1001, 1250, u16::MAX] {
        assert!(OverLimit::new(invalid).is_err());
    }
    for duration in [0, 1, 1000, 1250] {
        let gauge = OverLimit::active(duration).unwrap();
        assert_eq!(gauge.charge(), 0);
        assert_eq!(gauge.remaining(), duration);
        assert_eq!(gauge.saved_percent(), 0);
    }
    assert!(OverLimit::active(1251).is_err());
}

#[test]
fn results_keep_partial_charge_and_discard_full_charge_and_active_duration() {
    let mut battle = battle(&[999, 1000, 0, 0, 750]);
    battle.actors[2].overlimit = OverLimit::active(1250).unwrap();
    battle.actors[3].overlimit = OverLimit::active(17).unwrap();
    battle.actors[2].availability = ActorAvailability::Petrified;
    assert!(battle.normalize_result_overlimit().is_err());
    battle.terminal.result = Some(crate::BattleResult::Victory);
    battle.retire_combat().unwrap();
    battle.normalize_result_overlimit().unwrap();
    assert_eq!(
        battle
            .actors
            .iter()
            .map(|actor| actor.overlimit.charge())
            .collect::<Vec<_>>(),
        [999, 0, 0, 0, 750]
    );
    assert!(
        battle
            .actors
            .iter()
            .all(|actor| !actor.overlimit.is_active())
    );
    assert_eq!(battle.actors[2].availability, ActorAvailability::Petrified);
}

#[test]
fn equipment_refresh_preserves_active_duration_and_contact_exhausts_it() {
    let mut battle = battle(&[0, 0]);
    battle.actors[0].overlimit = OverLimit::active(5).unwrap();
    let mut replacement = battle.actors[0].clone();
    replacement.equipment.max_hp += 1;
    crate::tests::equip(&mut battle, ActorId(0), replacement).unwrap();
    assert_eq!(battle.actors[0].overlimit.remaining(), 5);
    battle.contact_overlimit(ActorId(0), true);
    assert_eq!(battle.actors[0].overlimit, OverLimit::default());
}

#[test]
fn admitted_actor_group_pushes_real_bodies_after_entry_but_held_visits_keep_prior_placement() {
    let mut actors = vec![
        crate::tests::actor(Side::Party),
        crate::tests::actor(Side::Enemy),
    ];
    actors[0].overlimit = OverLimit::new(1000).unwrap();
    actors[0].movement.forward = 2.;
    actors[1].position = [40., 0., 0.];
    for actor in &mut actors {
        actor.body.collider = Some(crate::Collider::sphere(30.));
    }
    let prepared = PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| {
                (
                    actor,
                    crate::ActorSetup {
                        overlimit_gain: 15,
                        ..Default::default()
                    },
                )
            })
            .collect(),
        Default::default(),
        77,
    )
    .unwrap()
    .with_overlimit_boost(false);
    let mut battle = prepared.finish().unwrap();
    let admission = battle.step(BattleInput::default()).unwrap();
    assert!(admission.actors[0].overlimit.is_active());
    assert!(
        admission.actors[0].position[0] < -1.,
        "body postpass did not push: {:?}",
        admission.actors[0].position
    );

    let retained = admission
        .actors
        .iter()
        .map(|actor| actor.position)
        .collect::<Vec<_>>();
    for _ in 0..2 {
        let held = battle.step(BattleInput::default()).unwrap();
        assert_eq!(
            held.actors
                .iter()
                .map(|actor| actor.position)
                .collect::<Vec<_>>(),
            retained
        );
        // The held update keeps the last published placement.
    }
}
