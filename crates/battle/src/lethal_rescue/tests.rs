use super::*;
use crate::{Activity, Actor, ActorAvailability, PreparedBattle};

pub(crate) fn battle(victim: Actor, seed: u64) -> Battle {
    prepared(victim, seed).finish().unwrap()
}

pub(crate) fn prepared(mut victim: Actor, seed: u64) -> PreparedBattle {
    victim.body.collider = Some(crate::Collider::sphere(2.));
    let owner = crate::tests::actor(if victim.side == Side::Party {
        Side::Enemy
    } else {
        Side::Party
    });
    let actors = vec![owner, victim];
    PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        seed,
    )
    .unwrap()
}

#[test]
fn rescue_priority_respects_traits_side_and_luck() {
    let all = LethalRescueTraits {
        resurrect: true,
        angel_tear: true,
        equipment: [
            Some(RescueEquipment::Chance),
            Some(RescueEquipment::Consumable(5)),
            Some(RescueEquipment::Consumable(3)),
        ],
    };

    assert_eq!(
        select(
            true,
            Condition::Revive.into(),
            Side::Party,
            all,
            255,
            || panic!("guaranteed rescue must not roll")
        ),
        Some(RescueKind::AngelTear)
    );
    assert_eq!(
        select(
            false,
            Condition::Revive.into(),
            Side::Party,
            all,
            255,
            || panic!("guaranteed rescue must not roll")
        ),
        Some(RescueKind::Revive)
    );
    assert_eq!(
        select(false, ConditionSet::EMPTY, Side::Party, all, 255, || 99),
        Some(RescueKind::Doll(1))
    );
    for (side, traits, expected) in [
        (Side::Party, all, Some(RescueKind::Resurrect)),
        (
            Side::Party,
            LethalRescueTraits {
                resurrect: false,
                ..all
            },
            Some(RescueKind::Ring),
        ),
        (Side::Enemy, all, Some(RescueKind::Resurrect)),
        (
            Side::Enemy,
            LethalRescueTraits {
                resurrect: false,
                ..all
            },
            None,
        ),
        (
            Side::Party,
            LethalRescueTraits {
                equipment: [
                    Some(RescueEquipment::Consumable(5)),
                    Some(RescueEquipment::Consumable(3)),
                    None,
                ],
                ..Default::default()
            },
            Some(RescueKind::Doll(0)),
        ),
    ] {
        assert_eq!(
            select(false, ConditionSet::EMPTY, side, traits, 0, || 0),
            expected
        );
    }
    for (luck, success) in [(0, false), (15, false), (16, true), (255, true)] {
        assert_eq!(
            select(
                false,
                ConditionSet::EMPTY,
                Side::Party,
                LethalRescueTraits {
                    resurrect: true,
                    ..Default::default()
                },
                luck,
                || 2
            )
            .is_some(),
            success
        );
    }
}

#[test]
fn rescue_preserves_reaction_gauge_conditions_and_consumes_its_charge() -> Result<()> {
    let mut victim = crate::tests::actor(Side::Party);
    victim.equipment.recovery.lethal.angel_tear = true;
    victim.equipment.recovery.boost = true;
    victim.equipment.recovery.lucky = true;
    victim.overlimit = crate::OverLimit::new(543).unwrap();
    victim.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
        base: ConditionSet::from(Condition::Weak),
        ..Default::default()
    });
    let mut battle = battle(victim, 19);
    let id = ActorId(1);
    battle.actors[1].hp = 0;
    battle.enter_knockdown(id, &mut vec![]);
    battle.actors[1].reaction.protection.remaining = 240;
    let random = battle.random_state();
    let mut cues = vec![];
    assert!(battle.try_lethal_rescue(id, ConditionSet::from(Condition::Weak), &mut cues)?);
    let actor = &battle.actors[1];
    assert_eq!(
        (
            actor.hp,
            actor.overlimit.charge(),
            battle.activity(ActorId(1)),
            actor.availability
        ),
        (30, 543, Activity::KnockedDown, ActorAvailability::Active)
    );
    assert_eq!(actor.conditions.base(), ConditionSet::from(Condition::Weak));
    assert_eq!(actor.reaction.protection.remaining, 240);
    assert_eq!(
        actor.reaction.protection.mode,
        crate::ProtectionMode::Escape
    );
    assert_eq!(battle.timed_hold_remaining(), Some(45));
    assert_eq!(battle.random_state(), random);
    assert!(!battle.angel_tear_armed(id)?);

    assert!(cues.contains(&Cue::Rescued {
        actor: id,
        kind: RescueKind::AngelTear
    }));
    assert!(
        cues.iter()
            .any(|c| matches!(c, Cue::Recovered { nominal: 30, .. }))
    );
    assert!(cues.iter().any(|c| matches!(
        c,
        Cue::ConditionLabel {
            kind: crate::conditions::ConditionLabel::ExSkillEffect,
            ..
        }
    )));
    Ok(())
}

#[test]
fn latch_retains_across_equipment_edits_and_doll_consumes_only_first_slot() -> Result<()> {
    let mut victim = crate::tests::actor(Side::Party);
    victim.equipment.recovery.lethal.angel_tear = true;
    let mut battle = battle(victim, 0);
    let id = ActorId(1);
    let mut replacement = crate::tests::actor(Side::Party);
    replacement.equipment.recovery.lethal.equipment = [
        Some(RescueEquipment::Consumable(5)),
        Some(RescueEquipment::Consumable(3)),
        Some(RescueEquipment::Chance),
    ];
    crate::tests::equip(&mut battle, id, replacement.clone())?;
    assert!(battle.angel_tear_armed(id)?);
    battle.actors[1].hp = 0;
    assert!(battle.try_lethal_rescue(id, ConditionSet::EMPTY, &mut vec![])?);
    crate::tests::equip(&mut battle, id, replacement.clone())?;
    assert!(!battle.angel_tear_armed(id)?);
    battle.actors[1].hp = 0;
    battle.random = crate::state::Random::new(0);
    let mut cues = vec![];
    assert!(battle.try_lethal_rescue(id, ConditionSet::EMPTY, &mut cues)?);
    assert_eq!(
        battle.actors[1].equipment.recovery.lethal.equipment,
        [
            Some(RescueEquipment::Consumed(5)),
            Some(RescueEquipment::Consumable(3)),
            Some(RescueEquipment::Chance)
        ]
    );
    assert_eq!(battle.actors[1].hp, 50);
    assert!(cues.iter().any(|c| matches!(
        c,
        Cue::Rescued {
            kind: RescueKind::Doll(0),
            ..
        }
    )));
    replacement.equipment.recovery.lethal.angel_tear = true;
    crate::tests::equip(&mut battle, id, replacement)?;
    assert!(!battle.angel_tear_armed(id)?);
    Ok(())
}

#[test]
fn revive_consumes_the_admitted_condition_without_consuming_equipment_layers() -> Result<()> {
    let mut victim = crate::tests::actor(Side::Enemy);
    let flag = Condition::Revive.into();
    victim.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
        base: flag,
        intrinsic: flag,
        equipment_overlay: flag,
        ..Default::default()
    });
    let mut battle = battle(victim, 17);
    battle.actors[1].hp = 0;
    assert!(battle.try_lethal_rescue(ActorId(1), flag, &mut vec![])?);
    let layers = battle.actors[1].conditions.layers();
    assert_eq!(
        (layers.base, layers.intrinsic, layers.equipment_overlay),
        (ConditionSet::EMPTY, ConditionSet::EMPTY, flag)
    );
    assert_eq!(battle.timed_hold_remaining(), Some(45));
    // Cached entry condition, independently of the post-hit live layer.
    battle.actors[1].conditions = Default::default();
    battle.actors[1].hp = 0;
    assert!(battle.try_lethal_rescue(ActorId(1), flag, &mut vec![])?);
    Ok(())
}

#[test]
fn rescue_hold_preserves_actor_common_timers() -> Result<()> {
    let mut victim = crate::tests::actor(Side::Party);
    victim.equipment.recovery.lethal.angel_tear = true;
    let mut battle = battle(victim, 19);
    let id = ActorId(1);
    battle.actors[1].hp = 0;
    battle.try_lethal_rescue(id, ConditionSet::EMPTY, &mut vec![])?;
    battle.request_timed_hold(3, None);
    for remaining in (40..45).rev() {
        battle.step(crate::BattleInput::default())?;

        assert_eq!(battle.actors[1].reaction.protection.remaining, 90);
        assert_eq!(battle.timed_hold_remaining(), Some(remaining));
        assert_eq!(battle.timed_hold.and_then(|hold| hold.actor), Some(id));
    }
    // A zero-duration request cannot replace or prolong the live hold.
    battle.request_timed_hold(0, None);
    assert_eq!(battle.timed_hold_remaining(), Some(40));
    for _ in 0..40 {
        battle.advance_timed_hold();
    }
    assert_eq!(battle.timed_hold_remaining(), None);
    assert_eq!(battle.timed_hold.and_then(|hold| hold.actor), None);
    assert!(!battle.is_paused());
    Ok(())
}
