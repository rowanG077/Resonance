use super::*;
use crate::conditions::{Buff, Condition, ConditionSet, Layers};
use crate::{ActorAvailability, Element};

fn layers(
    actor: &mut crate::Actor,
    base: ConditionSet,
    intrinsic: ConditionSet,
    overlay: ConditionSet,
) {
    actor.conditions = crate::conditions::Conditions::new(Layers {
        base,
        intrinsic,
        equipment_overlay: overlay,
        immunity: ConditionSet::EMPTY,
    });
}

fn assert_no_recovery_or_revival(cues: &[Cue]) {
    assert!(
        cues.iter()
            .all(|cue| !matches!(cue, Cue::Recovered { .. } | Cue::ConditionLabel { .. }))
    );
}

#[test]
fn poison_cure_prevents_further_damage_and_spends_once() {
    for item in [10, 12] {
        let mut battle = battle();
        layers(
            &mut battle.actors[1],
            Condition::PoisonSevere.into(),
            ConditionSet::EMPTY,
            ConditionSet::EMPTY,
        );
        let mut storage = Storage::new(item, 2);
        before_release(&mut battle, &mut storage, request(item, 1));
        let frame = step(&mut battle, &mut storage);
        assert!(battle.actors[1].conditions.base().is_empty());
        let cured_hp = battle.actors[1].hp;
        for _ in 0..300 {
            step(&mut battle, &mut storage);
        }
        assert_eq!(battle.actors[1].hp, cured_hp);
        assert_eq!(storage.acquisitions, 1);
        assert_eq!(storage.inventory[&item], 1);
        assert_eq!(battle.ledger.items[0], 1);
        assert_no_recovery_or_revival(&frame.cues);
        assert!(!battle.is_diagnostic());
    }
}

#[test]
fn cures_require_mutable_ailments_and_a_present_living_party_target() {
    let mut battle = battle();
    for (base, expected) in [
        (ConditionSet::EMPTY, [false; 3]),
        (Condition::PoisonSevere.into(), [true, false, true]),
        (Condition::Weak.into(), [false, true, true]),
        (Condition::Flare.into(), [false; 3]),
    ] {
        layers(
            &mut battle.actors[1],
            base,
            ConditionSet::EMPTY,
            ConditionSet::EMPTY,
        );
        for (item, eligible) in [10, 13, 12].into_iter().zip(expected) {
            assert_eq!(
                battle.item_target_eligible(item, ActorId(1)).unwrap(),
                eligible
            );
        }
    }
    // Inherent and equipped conditions cannot be cured as mutable ailments.
    layers(
        &mut battle.actors[1],
        ConditionSet::EMPTY,
        Condition::Petrified.into(),
        Condition::Weak.into(),
    );
    for item in [10, 13, 12] {
        assert!(battle.queue_item(request(item, 1)).is_err());
        assert!(battle.pending_item().is_none());
    }
    layers(
        &mut battle.actors[1],
        Condition::Petrified.into(),
        ConditionSet::EMPTY,
        ConditionSet::EMPTY,
    );
    for (availability, expected) in [
        (ActorAvailability::Active, true),
        (ActorAvailability::Petrified, true),
        (ActorAvailability::Dead, false),
        (ActorAvailability::Absent, false),
    ] {
        battle.actors[1].availability = availability;
        assert_eq!(
            battle.item_target_eligible(12, ActorId(1)).unwrap(),
            expected
        );
    }
    layers(
        &mut battle.actors[2],
        Condition::Petrified.into(),
        ConditionSet::EMPTY,
        ConditionSet::EMPTY,
    );
    assert!(!battle.item_target_eligible(12, ActorId(2)).unwrap());
    battle.actors[0].availability = ActorAvailability::Petrified;
    assert!(!battle.can_queue_item(ActorId(0)).unwrap());
}

#[test]
fn stale_cure_targets_cancel_before_hand_feedback_or_inventory_acquisition() {
    for item in [10, 12] {
        for dead in [false, true] {
            let mut battle = battle();
            layers(
                &mut battle.actors[1],
                ConditionSet::of(&[Condition::Petrified]),
                ConditionSet::EMPTY,
                ConditionSet::EMPTY,
            );
            let mut storage = Storage::new(item, 2);
            before_release(&mut battle, &mut storage, request(item, 1));
            if dead {
                battle.actors[1].availability = ActorAvailability::Dead;
                battle.actors[1].hp = 0;
            } else {
                layers(
                    &mut battle.actors[1],
                    ConditionSet::EMPTY,
                    ConditionSet::of(&[Condition::Petrified]),
                    ConditionSet::of(&[Condition::Petrified]),
                );
            }
            let state = battle.actors[1].conditions.clone();
            let grade = battle.ledger.grade();
            let random = battle.random_state();
            let frame = step(&mut battle, &mut storage);
            assert!(battle.pending_item().is_none());
            assert_eq!(storage.acquisitions, 0);
            assert_eq!(storage.inventory[&item], 2);
            assert_eq!(battle.actors[1].conditions.base(), state.base());
            assert_eq!(battle.ledger.grade(), grade);
            assert_eq!(battle.ledger.items[0], 0);
            assert!(!battle.ledger.party_item_effect_used);
            assert_eq!(battle.item_cooldown(), 0);
            assert_eq!(battle.random_state(), random);
            assert!(!battle.is_diagnostic());
            assert!(
                frame
                    .cues
                    .iter()
                    .all(|cue| !matches!(cue, Cue::ItemReleased { .. } | Cue::ItemNotice { .. }))
            );
            for _ in 0..3 {
                step(&mut battle, &mut storage);
            }
            assert_eq!(storage.acquisitions, 0);
        }
    }
}

#[test]
fn cures_remove_matching_effects_without_promoting_gear_or_intrinsic_conditions() {
    for (item, ailment, retained_buffs, enchanted) in [
        (
            10,
            ConditionSet::of(&[Condition::Petrified]),
            ConditionSet::of(&[
                Condition::Flare,
                Condition::PhysicalProtection,
                Condition::Quartz,
                Condition::Enchanted,
            ]),
            true,
        ),
        (
            13,
            ConditionSet::of(&[Condition::Weak]),
            ConditionSet::of(&[Condition::Quartz, Condition::Enchanted]),
            true,
        ),
        (
            12,
            ConditionSet::of(&[Condition::Petrified]),
            ConditionSet::EMPTY,
            false,
        ),
    ] {
        let mut battle = battle();
        layers(
            &mut battle.actors[1],
            ailment,
            ailment,
            ConditionSet::of(&[Condition::Heavy]),
        );
        let mut storage = Storage::new(item, 2);
        before_release(&mut battle, &mut storage, request(item, 1));
        for buff in [
            Buff::Flare,
            Buff::PhysicalAilmentGuard { persistent: true },
            Buff::Quartz(Element::Ice),
        ] {
            battle.actors[1]
                .conditions
                .prepare_buff(buff, false)
                .commit(&mut battle.actors[1]);
        }
        let before = battle.actors[1].clone();
        let cues = step(&mut battle, &mut storage).cues;
        let target = &battle.actors[1];
        assert_eq!(target.conditions.base(), retained_buffs);
        assert_eq!(
            target.conditions.effective(),
            retained_buffs
                .union(ailment)
                .union(ConditionSet::of(&[Condition::Heavy]))
        );
        assert_eq!(target.conditions.layers().intrinsic, ailment);
        assert_eq!(
            target.conditions.layers().equipment_overlay,
            ConditionSet::of(&[Condition::Heavy])
        );
        assert_eq!(
            target.elements.enchantment,
            enchanted.then_some(Element::Ice)
        );
        assert_eq!((target.hp, target.tp), (before.hp, before.tp));
        assert_eq!(target.availability, ActorAvailability::Active);
        assert!(!battle.item_target_eligible(item, ActorId(1)).unwrap());
        assert_eq!(storage.acquisitions, 1);
        assert_eq!(storage.inventory[&item], 1);
        assert_eq!(battle.ledger.items[0], 1);
        assert_eq!(battle.item_cooldown(), 120);
        assert_no_recovery_or_revival(&cues);
    }
}

#[test]
fn anti_magic_bottle_clears_weak_without_thawing_stone_or_touching_vitals() {
    let mut battle = stone_battle(1);
    layers(
        &mut battle.actors[1],
        ConditionSet::of(&[Condition::Petrified, Condition::Weak]),
        ConditionSet::EMPTY,
        ConditionSet::EMPTY,
    );
    assert_eq!(battle.actors[1].availability, ActorAvailability::Petrified);
    let hp_tp = (battle.actors[1].hp, battle.actors[1].tp);
    let activity = battle.activity(ActorId(1));
    let random = battle.random_state();
    let mut storage = Storage::new(13, 2);
    let request = Release {
        user: ActorId(0),
        target: ActorId(1),
        item: 13,
    };
    before_release(&mut battle, &mut storage, request);
    let frame = step(&mut battle, &mut storage);
    let target = &battle.actors[1];
    assert_eq!(
        target.conditions.base(),
        ConditionSet::of(&[Condition::Petrified])
    );
    assert_eq!(target.availability, ActorAvailability::Petrified);
    assert_eq!(battle.activity(ActorId(1)), activity);
    assert_eq!((target.hp, target.tp), hp_tp);
    assert_eq!(battle.random_state(), random);
    assert_eq!(storage.acquisitions, 1);
    assert_eq!(storage.inventory[&13], 1);
    assert!(battle.pending_item().is_none());
    assert_eq!(battle.item_cooldown(), 120);
    assert_no_recovery_or_revival(&frame.cues);
}

#[test]
fn stone_cures_restore_control_and_spend_once() {
    for (item, user, target) in [(10, 0, 1), (12, 1, 0)] {
        let candidate = stone_prepared(target);
        let mut battle = candidate.finish().unwrap();
        let request = Release {
            user: ActorId(user),
            target: ActorId(target as u8),
            item,
        };
        let mut storage = Storage::new(item, 2);
        before_release(&mut battle, &mut storage, request);
        let vitals = (battle.actors[target].hp, battle.actors[target].tp);
        let frame = step(&mut battle, &mut storage);
        let recipient = &battle.actors[target];
        assert!(recipient.available());
        assert!(recipient.conditions.base().is_empty());
        assert_eq!((recipient.hp, recipient.tp), vitals);
        assert_no_recovery_or_revival(&frame.cues);
        for _ in 0..12 {
            step(&mut battle, &mut storage);
        }
        assert!(battle.actor_command_ready(request.target));
        assert_eq!(storage.acquisitions, 1);
        assert_eq!(storage.inventory[&item], 1);
        assert_eq!(battle.ledger.items[usize::from(user)], 1);
        assert!(!battle.is_diagnostic());
    }
}

#[test]
fn cure_faults_preserve_conditions_and_inventory_without_blocking_later_actors() {
    for item in [10, 12] {
        for paranoid in [false, true] {
            let mut battle = stone_battle(1);
            let diagnostics = Diagnostics::new(paranoid);
            battle.set_diagnostics(diagnostics.clone());
            let mut storage = Storage::new(item, 1);

            before_release(&mut battle, &mut storage, request(item, 1));
            storage.inventory.clear();
            battle.begin_hurt(ActorId(2), 1, &mut vec![]);
            let target = battle.actors[1].clone();
            let inventory = storage.inventory.clone();
            let result = battle.update(BattleInput::default(), &mut storage);
            assert_eq!(result.is_err(), paranoid, "{item}");
            assert!(battle.pending_item().is_none());
            assert_eq!(storage.acquisitions, 1);
            assert_eq!(storage.inventory, inventory);
            assert!(!storage.gel);
            assert!(!storage.scanned && !storage.location);
            assert_eq!(battle.actors[1].conditions, target.conditions);
            assert_eq!(battle.actors[1].availability, ActorAvailability::Petrified);
            assert_eq!(
                (battle.actors[1].hp, battle.actors[1].tp),
                (target.hp, target.tp)
            );
            assert_eq!(battle.ledger.items[0], 0);
            assert_eq!(battle.ledger.grade(), 0);
            assert!(!battle.ledger.party_item_effect_used);

            assert_eq!(battle.item_cooldown(), 0);
            if let Ok(cues) = result {
                assert!(battle.is_diagnostic());
                assert_eq!(battle.activity(ActorId(2)), Activity::Idle);
                assert_eq!(diagnostics.entries().len(), 1);
                assert!(
                    cues.iter().all(|cue| !matches!(
                        cue,
                        Cue::ItemReleased { .. } | Cue::ItemNotice { .. }
                    ))
                );
                for _ in 0..3 {
                    step(&mut battle, &mut storage);
                }
                assert_eq!(diagnostics.entries().len(), 1);
                assert_eq!(storage.acquisitions, 1);
                assert_eq!(storage.inventory, inventory);
            }
        }
    }
}

fn stone_prepared(target: usize) -> PreparedBattle {
    let mut actors = vec![actor(), actor(), crate::tests::actor(Side::Enemy)];
    actors[target].availability = crate::ActorAvailability::Petrified;
    layers(
        &mut actors[target],
        ConditionSet::of(&[Condition::Petrified]),
        ConditionSet::EMPTY,
        ConditionSet::EMPTY,
    );
    prepared(actors)
}

fn stone_battle(target: usize) -> Battle {
    stone_prepared(target).finish().unwrap()
}
