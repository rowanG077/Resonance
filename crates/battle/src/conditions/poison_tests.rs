use super::*;
use crate::{ActorAvailability, BattleInput, PreparedBattle, Side, tests::actor};
use anyhow::Result;

fn poisoned(base: ConditionSet, equipment: ConditionSet) -> Actor {
    let mut owner = actor(Side::Party);
    owner.equipment.max_hp = 328;
    owner.hp = 300;
    owner.conditions = Conditions::new(Layers {
        base,
        equipment_overlay: equipment,
        ..Default::default()
    });
    owner
}

fn prepared() -> PreparedBattle {
    PreparedBattle::new(
        vec![
            (
                poisoned(PoisonMild.into(), ConditionSet::EMPTY),
                Default::default(),
            ),
            (actor(Side::Enemy), Default::default()),
        ],
        Default::default(),
        17,
    )
    .unwrap()
}

#[test]
fn poison_ticks_every_thirty_updates_and_severe_poison_takes_precedence() {
    for (base, equipment, damage) in [
        (PoisonMild.into(), ConditionSet::EMPTY, 3),
        (PoisonSevere.into(), ConditionSet::EMPTY, 6),
        (POISON, ConditionSet::EMPTY, 6),
        (PoisonMild.into(), PoisonSevere.into(), 6),
    ] {
        let mut owner = poisoned(base, equipment);
        let mut pulses = vec![];
        for update in 1..=90 {
            if owner.advance_conditions(true) {
                pulses.push(update);
            }
        }
        assert_eq!(pulses, [30, 60, 90]);
        assert_eq!(owner.hp, 300 - 3 * damage);
    }
}

#[test]
fn poison_is_nonlethal_and_uses_full_hp_without_recovery_or_contact_modifiers() {
    for (maximum, hp, mask, expected) in [
        (328, 300, PoisonMild.into(), 297),
        (328, 300, PoisonSevere.into(), 294),
        (328, 164, ConditionSet::of(&[PoisonMild, Weak]), 161),
        (99, 1, PoisonSevere.into(), 1),
        (i32::MAX, 100_000, PoisonMild.into(), 1),
    ] {
        let mut owner = poisoned(mask, ConditionSet::EMPTY);
        owner.equipment.max_hp = maximum;
        owner.hp = hp;
        owner.equipment.recovery.boost = true;
        owner.equipment.recovery.lucky = true;
        owner.guard.active = true;
        let tp = owner.tp;
        for _ in 0..30 {
            owner.advance_conditions(true);
        }
        assert_eq!(owner.hp, expected);
        assert_eq!(owner.tp, tp);
        assert!(owner.available());
    }
}

#[test]
fn cure_removes_poison_and_its_clock_without_removing_equipment_poison() {
    for equipment in [ConditionSet::EMPTY, PoisonMild.into()] {
        let mut owner = poisoned(PoisonMild.into(), equipment);
        for _ in 0..29 {
            owner.advance_conditions(true);
        }
        owner.conditions.cure(Cure::Physical);
        assert_eq!(owner.conditions.base(), ConditionSet::EMPTY);
        assert_eq!(owner.advance_conditions(true), !equipment.is_empty());
        assert_eq!(owner.hp, if equipment.is_empty() { 300 } else { 297 });
        owner.conditions.reload_layers(Layers::default());
        assert!(owner.conditions.periodic_effects().is_empty());
        owner.conditions.reload_layers(Layers {
            base: PoisonMild.into(),
            ..Default::default()
        });
        assert_eq!(owner.conditions.periodic_effects()[0].remaining, 30);
    }
}

#[test]
fn phase_and_unavailable_actors_hold_poison() {
    let mut owner = poisoned(PoisonMild.into(), ConditionSet::EMPTY);
    let before = owner.conditions.clone();
    owner.advance_conditions(false);
    for availability in [
        ActorAvailability::Dead,
        ActorAvailability::Absent,
        ActorAvailability::Petrified,
    ] {
        owner.availability = availability;
        assert!(!owner.advance_conditions(true));
        assert_eq!(owner.conditions, before);
    }
    owner.availability = ActorAvailability::Active;
    owner.hit_stop = 10;
    owner.advance_conditions(true);
    assert_eq!(owner.conditions.periodic_effects()[0].remaining, 29);
}

#[test]
fn global_pauses_hold_a_due_poison_pulse_but_hit_stop_does_not() -> Result<()> {
    let mut battle = prepared().finish().unwrap();
    for _ in 0..29 {
        battle.advance_condition_callbacks(0, true, &mut vec![]);
    }
    let before = battle.actors[0].conditions.clone();
    battle.step(BattleInput {
        paused: true,
        ..Default::default()
    })?;
    assert_eq!(battle.actors[0].conditions, before);
    battle.actors[0].hit_stop = 9;
    let frame = battle.step(BattleInput::default())?;
    assert_eq!(frame.cues.iter().filter(|cue| matches!(cue, crate::Cue::PoisonPulse { actor } if *actor == crate::ActorId(0))).count(), 1);
    assert!(frame.models.is_empty());
    assert_eq!(battle.actors[0].hp, 297);
    assert_eq!(
        battle.actors[0].conditions.periodic_effects()[0].remaining,
        30
    );

    Ok(())
}
