use crate::conditions::{Buff, Conditions, Cure, Layers};
use crate::conditions::{Condition, ConditionSet};
use crate::{Actor, ActorId, Battle, BattleInput, PreparedBattle, Side};
use anyhow::Result;

fn stone() -> Actor {
    let mut actor = crate::tests::actor(Side::Party);
    actor.availability = crate::ActorAvailability::Petrified;
    actor.conditions = Conditions::new(Layers {
        base: ConditionSet::of(&[Condition::Petrified]),
        ..Default::default()
    });
    actor
}

fn battle() -> Battle {
    let mut other = crate::tests::actor(Side::Party);
    other.position = [100., 0., 0.];
    let mut enemy = crate::tests::actor(Side::Enemy);
    enemy.position = [400., 0., 0.];
    PreparedBattle::new(
        vec![
            (stone(), Default::default()),
            (other, Default::default()),
            (enemy, Default::default()),
        ],
        Default::default(),
        19,
    )
    .unwrap()
    .finish()
    .unwrap()
}

#[test]
fn petrification_holds_conditions_while_body_settles() -> Result<()> {
    let mut battle = battle();
    let actor = &mut battle.actors[0];
    actor.position = [10., 4., 0.];
    actor.heading = 37.;
    actor.reaction.direction = [1., 0., 0.];
    actor.movement.direction = [0., 0., 1.];
    actor.movement.forward = 2.;
    actor.movement.vertical = -1.;
    actor.movement.gravity = -0.25;
    actor.movement.braking = 0.75;
    actor.hit_stop = 2;
    actor.reaction.protection.item();
    actor
        .conditions
        .prepare_buff(Buff::Flare, false)
        .commit(actor);
    let conditions = actor.conditions.clone();
    let seed = battle.random.state();
    battle.step(BattleInput::default())?;
    let actor = &battle.actors[0];
    assert_eq!(actor.position, [12., 3., 0.]);
    assert_eq!(actor.movement.forward, 2.);
    assert_eq!(actor.movement.vertical, -1.25);
    assert_eq!(actor.heading, 37.);
    assert_eq!(actor.hit_stop, 1);
    assert_eq!(actor.reaction.protection.remaining, 59);

    assert_eq!(actor.conditions, conditions);
    assert_eq!(battle.random.state(), seed);

    battle.actors[0].position[1] = 0.;
    battle.actors[0].movement.vertical = -1.;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[0].position[1], 0.);
    assert_eq!(battle.actors[0].movement.forward, 1.25);
    assert_eq!(battle.actors[0].movement.vertical, 0.);
    Ok(())
}

#[test]
fn cure_keeps_intrinsic_stone_separate_from_mutable_conditions() -> Result<()> {
    let mut battle = battle();
    battle.actors[0].conditions = Conditions::new(Layers {
        base: ConditionSet::of(&[Condition::Petrified]),
        intrinsic: ConditionSet::of(&[Condition::Petrified]),
        ..Default::default()
    });
    battle.apply_cure(ActorId(0), Cure::All, &mut vec![]);
    assert!(battle.actors[0].available());
    assert!(battle.actors[0].conditions.base().is_empty());
    assert_eq!(
        battle.actors[0].conditions.effective(),
        ConditionSet::of(&[Condition::Petrified])
    );
    battle.step(BattleInput::default())?;

    Ok(())
}

#[test]
fn petrified_actor_publishes_current_body_after_physics() -> Result<()> {
    let mut owner = stone();
    owner.position = [10., 0., 0.];
    owner.reaction.direction = [1., 0., 0.];
    owner.movement.forward = 2.;
    owner.body.collider = Some(crate::Collider::sphere(10.));

    let mut enemy = crate::tests::actor(Side::Enemy);
    enemy.position = [400., 0., 0.];
    enemy.body.collider = Some(crate::Collider::sphere(10.));
    let prepared = PreparedBattle::new(
        vec![(owner, Default::default()), (enemy, Default::default())],
        Default::default(),
        19,
    )?;
    let mut battle = prepared.finish().unwrap();
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[0].target_center()[0], 12.);
    assert_eq!(battle.actors[0].position[0], 12.);
    crate::tests::assert_close(
        crate::control::body_gap(&battle.actors[0], &battle.actors[1]),
        368.,
        0.001,
    );
    Ok(())
}

#[test]
fn self_cure_restores_availability_and_idle_recovery_during_updates() -> Result<()> {
    let mut battle = battle();

    let actor = &mut battle.actors[0];
    actor.equipment.recovery.common.self_cure = true;
    actor.equipment.recovery.common.idle_hp_tp = true;
    actor.hp = actor.equipment.max_hp - 2;
    actor.tp = actor.equipment.max_tp - 1;
    battle.runtime[0].recovery.self_cure_updates = 299;
    battle.runtime[0].recovery.idle_updates = 119;
    let frame = battle.step(BattleInput::default())?;
    let actor = &frame.actors[0];
    assert!(actor.available());
    assert!(!actor.conditions.base().contains(Condition::Petrified));
    assert!(
        frame
            .cues
            .iter()
            .any(|cue| matches!(cue, crate::Cue::SelfCured { .. }))
    );
    assert!(
        frame
            .cues
            .iter()
            .any(|cue| matches!(cue, crate::Cue::Recovered { .. }))
    );
    battle.step(BattleInput::default())?;

    Ok(())
}
