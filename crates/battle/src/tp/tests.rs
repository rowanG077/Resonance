use super::*;
use crate::{
    ActionRequest, ActorId, Battle, BattleInput, Cue, Rejection,
    tests::{actor, prepared},
};
use std::sync::Arc;

fn request() -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            target: ActorId(0),
            action: crate::ActionKey(0),
        }],
        ..Default::default()
    }
}

fn battle(boost: bool, side: Side, tp: u16) -> Battle {
    let mut owner = actor(side);
    owner.tp = tp;
    owner.equipment.damage.physical_arte_boost = boost;
    let mut prepared = prepared(vec![owner], 8);
    Arc::make_mut(&mut prepared.resources.actions.entries[0]).tp_cost = 28;
    prepared.finish().unwrap()
}

#[test]
fn physical_arte_boost_applies_to_party_martial_techniques_only() {
    let mut owner = actor(Side::Party);
    let mut action = crate::tests::action(5);
    action.tp_cost = 28;
    assert_eq!(action_quote(&owner, crate::ActionKey(0), &action), 28);
    owner.equipment.damage.physical_arte_boost = true;
    assert_eq!(action_quote(&owner, crate::ActionKey(0), &action), 35);
    assert_eq!(spell_quote(&owner, 28, false), 28);
    owner.side = Side::Enemy;
    assert_eq!(action_quote(&owner, crate::ActionKey(0), &action), 28);
}

#[test]
fn modified_cost_admission_rejects_insufficient_tp_without_payment() {
    for tp in [34, 35] {
        let mut battle = battle(true, Side::Party, tp);
        assert_eq!(
            battle.technique_tp_cost(ActorId(0), crate::ActionKey(0)),
            Some(35)
        );
        let frame = battle.step(request()).unwrap();
        if tp < 35 {
            assert!(frame.cues.contains(&Cue::Rejected {
                actor: ActorId(0),
                reason: Rejection::InsufficientTp
            }));
            assert!(frame.actions.is_empty());
            assert_eq!(battle.actors[0].tp, tp);
        } else {
            assert_eq!(frame.actions.len(), 1);
            assert_eq!(battle.actors[0].tp, 0);
            let paid = battle.actors[0].tp;
            for _ in 0..3 {
                battle.step(BattleInput::default()).unwrap();
            }
            assert_eq!(battle.actors[0].tp, paid);
        }
        assert_eq!(battle.prepared.actions.entries[0].tp_cost, 28);
    }
    for (side, boost) in [
        (Side::Party, false),
        (Side::Enemy, false),
        (Side::Enemy, true),
    ] {
        let mut battle = battle(boost, side, 28);
        let frame = battle.step(request()).unwrap();
        assert_eq!(frame.actions.len(), 1);
        assert_eq!(battle.actors[0].tp, 0);
    }
}

#[test]
fn native_admission_commits_cost_and_use_before_paused_choreography() {
    let mut owner = actor(Side::Party);
    owner.hit_stop = 3;
    let action = crate::ActionDefinition {
        normal: None,
        tp_cost: 7,
        execution: crate::ActionExecution::Attack(crate::PreparedAttack {
            chain_at: None,
            end_at: 8,
            opening: None,
            recovery: 0,
            events: vec![(0, crate::AttackEvent::Sound(Some(crate::Sound::Stream(1))))],
        }),
    };
    let mut battle = crate::PreparedBattle::new(
        vec![(
            owner,
            crate::ActorSetup {
                techniques: vec![crate::tests::technique(crate::ActionKey(0), 7)],
                ..Default::default()
            },
        )],
        (vec![action]).into(),
        1,
    )
    .unwrap()
    .with_technique_learning_members(vec![crate::tests::counted_techniques(
        ActorId(0),
        &[7],
        &[(7, 49)],
    )])
    .unwrap()
    .finish()
    .unwrap();
    battle.actors[0].time_stop = 1;
    let mut cues = vec![];
    battle.start(request().actions[0], &mut cues).unwrap();
    assert!(cues.contains(&Cue::Rejected {
        actor: ActorId(0),
        reason: Rejection::Busy
    }));
    assert_eq!(battle.actors[0].tp, 40);
    assert_eq!(battle.technique_uses(ActorId(0), 7), Some(49));
    battle.actors[0].time_stop = 0;
    battle.start(request().actions[0], &mut vec![]).unwrap();
    assert_eq!(battle.actors[0].tp, 33);
    assert_eq!(battle.technique_uses(ActorId(0), 7), Some(50));
    assert_eq!(battle.actors[0].proficiency, 1);
    let id = *battle.sequences().map(|(id, _)| id).next().unwrap();
    for _ in 0..3 {
        let held = battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.action_age(id), Some(0));
        assert!(!held.cues.iter().any(|cue| matches!(cue, Cue::Sound { .. })));
    }
    let resumed = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        resumed
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Sound { .. }))
            .count(),
        1
    );
    battle
        .step(BattleInput {
            interrupt: vec![id],
            ..Default::default()
        })
        .unwrap();
    for _ in 0..3 {
        assert!(
            !battle
                .step(BattleInput::default())
                .unwrap()
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Sound { .. }))
        );
    }
    assert_eq!(battle.actors[0].tp, 33);
    assert_eq!(battle.technique_uses(ActorId(0), 7), Some(50));
    assert!(battle.sequences().next().is_none());
}

#[test]
fn technical_and_rings_reduce_cost_without_signed_narrowing() {
    use crate::conditions::{Condition, ConditionSet, Conditions, Layers};
    for (raw, technical, boost, overlay, quote, spell) in [
        (9, false, true, ConditionSet::EMPTY, 11, 9),
        (8, true, false, ConditionSet::of(&[Condition::TpHalf]), 3, 4),
        (
            13,
            true,
            false,
            ConditionSet::of(&[Condition::TpThird]),
            8,
            7,
        ),
        (1, true, false, ConditionSet::of(&[Condition::TpHalf]), 0, 0),
        (0, true, false, ConditionSet::of(&[Condition::TpHalf]), 0, 0),
        (
            28,
            true,
            true,
            ConditionSet::of(&[Condition::TpHalf, Condition::TpThird]),
            15,
            13,
        ),
        (
            13,
            false,
            false,
            ConditionSet::of(&[Condition::TpHalf, Condition::TpThird]),
            6,
            6,
        ),
    ] {
        let mut owner = actor(Side::Party);
        owner.equipment.tp_cost_reduction = technical;
        owner.equipment.damage.physical_arte_boost = boost;
        owner.conditions = Conditions::new(Layers {
            equipment_overlay: overlay,
            ..Default::default()
        });
        assert_eq!(catalogue_quote(&owner, raw), quote);
        assert_eq!(spell_quote(&owner, raw, false), spell);
        owner.side = Side::Enemy;
        assert_eq!(catalogue_quote(&owner, raw), u32::from(raw));
        assert_eq!(spell_quote(&owner, raw, false), u32::from(raw));
    }
    let mut owner = actor(Side::Party);
    owner.equipment.damage.physical_arte_boost = true;
    assert_eq!(catalogue_quote(&owner, u16::MAX), 81_918);
    assert!(catalogue_quote(&owner, u16::MAX) > u32::from(u16::MAX));
    owner.equipment.damage.physical_arte_boost = false;
    owner.equipment.tp_cost_reduction = true;
    for raw in [0, 1, 32767, 32768, u16::MAX] {
        assert!(catalogue_quote(&owner, raw) <= u32::from(raw));
        assert!(spell_quote(&owner, raw, false) <= u32::from(raw));
    }
    assert_eq!(technical(0), 0);
}

#[test]
fn special_guard_uses_its_modified_max_tp_cost_or_catalogue_fallback() {
    use crate::conditions::{Condition, ConditionSet, Conditions, Layers};
    let mut owner = actor(Side::Party);
    owner.equipment.max_tp = 5;
    assert_eq!(special_guard_debit(&owner, 2), 2);
    owner.equipment.max_tp = 10;
    owner.conditions = Conditions::new(Layers {
        equipment_overlay: ConditionSet::of(&[Condition::TpHalf]),
        ..Default::default()
    });
    assert_eq!(special_guard_debit(&owner, 2), 1);
    owner.equipment.max_tp = 130;
    owner.equipment.tp_cost_reduction = true;
    assert_eq!(special_guard_debit(&owner, 2), 6);
    owner.equipment.damage.physical_arte_boost = true;
    assert_eq!(special_guard_debit(&owner, 2), 7);
}

#[test]
fn native_payment_uses_current_modifiers_at_admission() {
    use crate::conditions::{Condition, ConditionSet, Conditions, Layers};
    let mut battle = battle(true, Side::Party, 35);
    battle.actors[0].equipment.tp_cost_reduction = true;
    battle.actors[0].conditions = Conditions::new(Layers {
        equipment_overlay: ConditionSet::of(&[Condition::TpHalf]),
        ..Default::default()
    });
    let random = battle.random.state();
    assert_eq!(
        battle.technique_tp_cost(ActorId(0), crate::ActionKey(0)),
        Some(15)
    );
    assert_eq!(
        battle.technique_tp_cost(ActorId(0), crate::ActionKey(0)),
        Some(15)
    );
    assert_eq!(
        battle.random.state(),
        random,
        "viewing a quote does not draw payment RNG"
    );
    assert_eq!(battle.actors[0].tp, 35);
    battle.step(request()).unwrap();
    assert_eq!(battle.actors[0].tp, 20);
    battle.actors[0].equipment.tp_cost_reduction = false;
    battle.actors[0].conditions = Default::default();
    assert_eq!(
        battle.technique_tp_cost(ActorId(0), crate::ActionKey(0)),
        Some(35)
    );
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.actors[0].tp, 20);
}
