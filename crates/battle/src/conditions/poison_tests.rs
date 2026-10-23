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
    crate::tests::prepare_actors(
        vec![
            poisoned(PoisonMild.into(), ConditionSet::EMPTY),
            actor(Side::Enemy),
        ],
        17,
    )
}

#[test]
fn poison_pulses_follow_initialization_and_rearming_and_severe_takes_precedence() {
    for (base, equipment, damage) in [
        (PoisonMild.into(), ConditionSet::EMPTY, 3),
        (PoisonSevere.into(), ConditionSet::EMPTY, 6),
        (POISON, ConditionSet::EMPTY, 6),
        (PoisonMild.into(), PoisonSevere.into(), 6),
    ] {
        let mut owner = poisoned(base, equipment);
        let mut pulses = vec![];
        for update in 1..=400 {
            if owner.advance_conditions(true) {
                pulses.push(update);
            }
        }
        let persistent = !equipment.is_empty();
        assert_eq!(
            pulses,
            if persistent {
                vec![31, 61, 91, 121, 151, 181, 211, 241, 271, 301, 331, 361, 392]
            } else {
                vec![31, 61, 91, 121, 151, 181, 211, 241, 271, 301, 332, 362, 392]
            }
        );
        assert_eq!(owner.hp, 300 - 13 * damage);
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
        for _ in 0..31 {
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
        for _ in 0..30 {
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
        for _ in 0..30 {
            assert!(!owner.advance_conditions(true));
        }
        assert!(owner.advance_conditions(true));
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
    assert_ne!(owner.conditions, before);
    assert_eq!(owner.hp, 300);
}

#[test]
fn global_pauses_hold_a_due_poison_pulse_but_hit_stop_does_not() -> Result<()> {
    let mut battle = prepared().finish().unwrap();
    for _ in 0..30 {
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
    Ok(())
}
