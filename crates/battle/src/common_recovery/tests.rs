use super::*;
use crate::PreparedBattle;
use crate::Side;
use crate::conditions::Condition;
use crate::{
    ActorAvailability, BattleInput,
    conditions::{Conditions, Layers},
    tests::actor,
};

fn battle() -> Battle {
    PreparedBattle::new(
        vec![
            (actor(Side::Party), Default::default()),
            (actor(Side::Enemy), Default::default()),
        ],
        Default::default(),
        17,
    )
    .unwrap()
    .finish()
    .unwrap()
}

#[test]
fn flat_recovery_has_an_independent_budget_and_stops_at_full_hp() -> Result<()> {
    let mut battle = battle();
    let owner = &mut battle.actors[0];
    owner.equipment.max_hp = 1000;
    owner.hp = 99;
    owner.equipment.recovery.common.low_hp = true;
    owner.equipment.recovery.common.last_hit = true;
    battle.runtime[0].recovery.last_hit_recovery = 2;
    let random = battle.random.state();
    battle.advance_actor_common(0, &mut vec![])?;
    assert_eq!(battle.actors[0].hp, 100);
    battle.advance_actor_common(0, &mut vec![])?;
    assert_eq!(battle.actors[0].hp, 101);
    assert_eq!(battle.runtime[0].recovery.last_hit_recovery, 1);
    battle.actors[0].hp = 1000;
    for _ in 0..4 {
        battle.advance_actor_common(0, &mut vec![])?;
    }
    assert_eq!(battle.actors[0].hp, 1000);
    assert_eq!(battle.runtime[0].recovery.last_hit_recovery, 0);
    assert_eq!(battle.random.state(), random);
    Ok(())
}

#[test]
fn self_cure_removes_ailments_at_its_interval_and_restarts_the_clock() -> Result<()> {
    let mut battle = battle();
    battle.actors[0].equipment.recovery.common.self_cure = true;
    battle.actors[0].conditions = Conditions::new(Layers {
        base: Condition::Paralysis.into(),
        ..Default::default()
    });
    for _ in 1..SELF_CURE_INTERVAL {
        battle.advance_actor_common(0, &mut vec![])?;
    }
    assert!(
        battle.actors[0]
            .conditions
            .base()
            .contains(Condition::Paralysis)
    );
    let mut cues = vec![];
    battle.advance_actor_common(0, &mut cues)?;
    assert!(battle.actors[0].conditions.base().is_empty());
    assert!(cues.contains(&Cue::SelfCured { actor: ActorId(0) }));
    assert_eq!(battle.runtime[0].recovery.self_cure_updates, 0);
    battle.advance_actor_common(0, &mut vec![])?;
    assert_eq!(battle.runtime[0].recovery.self_cure_updates, 1);
    Ok(())
}

#[test]
fn idle_recovery_requires_consecutive_idle_updates_and_stacks_equipped_traits() -> Result<()> {
    let mut battle = battle();
    let owner = &mut battle.actors[0];
    owner.equipment.max_hp = 1000;
    owner.hp = 950;
    owner.equipment.max_tp = 200;
    owner.tp = 190;
    owner.equipment.recovery.common = CommonRecoveryTraits {
        idle_hp_tp: true,
        idle_hp: true,
        idle_tp: true,
        ..Default::default()
    };
    let mut cues = vec![];
    for _ in 0..119 {
        battle.advance_common_ex_recovery(0, true, &mut cues)?;
    }
    assert_eq!((battle.actors[0].hp, battle.actors[0].tp), (950, 190));
    battle.actors[0].guard.active = true;
    battle.advance_common_ex_recovery(0, true, &mut cues)?;
    assert_eq!(battle.runtime[0].recovery.idle_updates, 0);
    battle.actors[0].guard.active = false;
    for _ in 0..120 {
        battle.advance_common_ex_recovery(0, true, &mut cues)?;
    }
    assert_eq!((battle.actors[0].hp, battle.actors[0].tp), (970, 194));
    assert_eq!(battle.runtime[0].recovery.idle_updates, 0);
    battle.actors[0].availability = ActorAvailability::Dead;
    battle.advance_common_ex_recovery(0, true, &mut cues)?;
    assert_eq!(battle.runtime[0].recovery.idle_updates, 0);
    Ok(())
}

#[test]
fn menus_pause_recovery_and_equipment_changes_preserve_progress() -> Result<()> {
    let mut battle = battle();
    battle.runtime[0].recovery = CommonRecoveryState {
        idle_updates: 119,
        self_cure_updates: 299,
        last_hit_updates: 1,
        last_hit_recovery: 9,
        last_hit_damage: 19,
    };
    let before = battle.runtime[0].recovery;
    battle.step(BattleInput {
        paused: true,
        ..Default::default()
    })?;
    assert_eq!(battle.runtime[0].recovery, before);
    let mut replacement = actor(Side::Party);
    replacement.equipment.recovery.common.idle_tp = true;
    crate::tests::equip(&mut battle, ActorId(0), replacement)?;
    assert_eq!(battle.runtime[0].recovery, before);
    assert!(battle.actors[0].equipment.recovery.common.idle_tp);
    Ok(())
}

#[test]
fn enemy_idle_recovery_restores_hp_and_tp() -> Result<()> {
    let mut battle = battle();
    battle.actors[1].equipment.recovery.common.idle_hp_tp = true;
    battle.actors[1].tp = 0;
    battle.runtime[1].recovery.idle_updates = 119;
    battle.advance_common_ex_recovery(1, true, &mut vec![])?;
    assert_eq!((battle.actors[1].hp, battle.actors[1].tp), (51, 1));

    Ok(())
}
