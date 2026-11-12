//! Haste runs one actor's action clock faster than the battle's.
use crate::{
    ActionRequest, ActorId, Battle, BattleInput, Cue, Side,
    conditions::Condition,
    tests::{actor, prepared},
};

fn battle(percent: u8) -> Battle {
    let mut battle = prepared(vec![actor(Side::Party), actor(Side::Enemy)], 60)
        .finish()
        .unwrap();
    if percent != 0 {
        assert!(battle.actors[0].conditions.hasten(percent, 10_000));
    }
    battle
}

/// Updates from requesting the fixture arte until it completes.
fn arte_updates(percent: u8) -> u32 {
    let mut battle = battle(percent);
    let mut frame = battle
        .step(BattleInput::from(ActionRequest {
            actor: ActorId(0),
            target: ActorId(1),
            action: crate::ActionKey(0),
        }))
        .unwrap();
    for updates in 1..2000 {
        if frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Completed { .. }))
        {
            return updates;
        }
        frame = battle.step(Default::default()).unwrap();
    }
    panic!("the fixture arte never completed");
}

#[test]
fn a_hastened_arte_finishes_sooner_in_proportion_to_its_percent() {
    let ordinary = arte_updates(0);
    assert!(
        ordinary >= 20,
        "fixture arte is too short to measure: {ordinary}"
    );
    for percent in [20, 50, 100] {
        let hastened = arte_updates(percent);
        let expected = ordinary as f32 * 100. / (100. + f32::from(percent));
        assert!(
            (hastened as f32 - expected).abs() <= 2.,
            "{percent}%: {hastened} updates, expected about {expected} of {ordinary}"
        );
    }
}

#[test]
fn haste_expires_and_then_owes_no_more_ticks() {
    let mut battle = battle(0);
    assert_eq!(battle.actors[0].conditions.haste_percent(), 0);
    assert!(battle.actors[0].conditions.hasten(20, 30));
    assert_eq!(battle.actors[0].conditions.haste_percent(), 20);
    assert!((battle.actors[0].haste_scale() - 1.2).abs() < 1e-6);
    for _ in 0..40 {
        battle.step(Default::default()).unwrap();
    }
    assert_eq!(battle.actors[0].conditions.haste_percent(), 0);
    assert!(
        !battle.actors[0]
            .conditions
            .effective()
            .contains(Condition::Haste)
    );
    assert_eq!(battle.actors[0].haste_scale(), 1.);
    assert!(!battle.haste_tick_due(0));
    assert_eq!(battle.runtime[0].haste_phase, 0);
}

#[test]
fn haste_moves_an_actor_faster_by_the_same_percent() {
    let mut owner = actor(Side::Party);
    let conditions = owner.conditions.effective();
    let (walk, run) = (
        owner.walk_speed(10., 1., conditions),
        owner.run_limit(10., conditions),
    );
    assert!(owner.conditions.hasten(20, 100));
    let conditions = owner.conditions.effective();
    assert!((owner.walk_speed(10., 1., conditions) - walk * 1.2).abs() < 1e-4);
    assert!((owner.run_limit(10., conditions) - run * 1.2).abs() < 1e-4);
}
