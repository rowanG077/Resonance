use super::*;
use crate::{
    Activity, SpellSlot,
    tests::{actor, prepared},
};
use std::sync::Arc;

fn battle() -> Battle {
    let p = prepared(
        vec![actor(Side::Party), actor(Side::Enemy), actor(Side::Party)],
        30,
    );
    p.finish().unwrap()
}

fn request() -> BattleInput {
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
fn independent_slots_survive_caster_actions_and_reject_replacement() {
    let mut battle = battle();
    let mut cues = vec![];
    let primary = battle
        .release_volley(
            Arc::new(crate::tests::volley()),
            ActorId(0),
            ActorId(1),
            SpellSlot::Primary,
            None,
            &mut cues,
        )
        .unwrap()
        .unwrap();
    let secondary = battle
        .release_volley(
            Arc::new(crate::tests::volley()),
            ActorId(0),
            ActorId(2),
            SpellSlot::Secondary,
            None,
            &mut cues,
        )
        .unwrap()
        .unwrap();
    assert_ne!(primary, secondary);
    let frame = battle.step(request()).unwrap();
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Started {
            actor: ActorId(0),
            ..
        }
    )));
    let age = battle.action_age(primary);
    cues.clear();
    assert_eq!(
        battle
            .release_volley(
                Arc::new(crate::tests::volley()),
                ActorId(0),
                ActorId(2),
                SpellSlot::Primary,
                None,
                &mut cues
            )
            .unwrap(),
        None
    );
    assert!(cues.is_empty());
    assert_eq!(battle.volleys[&primary].target, ActorId(1));
    assert_eq!(battle.volleys[&secondary].target, ActorId(2));
    assert_eq!(battle.action_age(primary), age);
    battle
        .step(BattleInput {
            interrupt: vec![primary],
            ..Default::default()
        })
        .unwrap();
    assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
    assert!(battle.spell_active(ActorId(0), SpellSlot::Secondary));
    assert!(
        battle
            .release_volley(
                Arc::new(crate::tests::volley()),
                ActorId(0),
                ActorId(1),
                SpellSlot::Primary,
                None,
                &mut cues
            )
            .unwrap()
            .is_some()
    );
}

#[test]
fn released_spell_survives_caster_interruption_hurt_and_death_then_cleans_up() {
    let mut battle = battle();
    let parent = battle.step(request()).unwrap().actions[0].0;
    let resident = battle
        .release_volley(
            Arc::new(crate::tests::volley()),
            ActorId(0),
            ActorId(1),
            SpellSlot::Primary,
            Some(parent),
            &mut vec![],
        )
        .unwrap()
        .unwrap();
    let frame = battle
        .step(BattleInput {
            interrupt: vec![parent, parent],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        frame
            .cues
            .iter()
            .filter(|cue| **cue == Cue::Interrupted { action: parent })
            .count(),
        1
    );
    assert_eq!(battle.action_age(parent), None);
    assert_eq!(frame.actors[0].activity, Activity::Idle);
    assert!(battle.spell_active(ActorId(0), SpellSlot::Primary));

    battle.begin_hurt(ActorId(0), 10, &mut vec![]);
    battle.step(BattleInput::default()).unwrap();
    battle.actors[0].hp = 0;
    battle.enter_death(ActorId(0), &mut vec![]);
    let mut completed = false;
    for _ in 0..90 {
        let frame = battle.step(BattleInput::default()).unwrap();
        completed |= frame.cues.contains(&Cue::Completed { action: resident });
    }
    assert!(completed);
    assert_eq!(battle.actors[0].hp, 0);
    assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
}
