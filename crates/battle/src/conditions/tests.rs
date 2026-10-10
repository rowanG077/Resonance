use super::*;
use crate::{
    ActorAvailability, BattleInput, Side,
    tests::{actor, prepared},
};
use Condition::*;
use resonance_content::battle_action::Condition as HitAilment;

#[test]
fn cure_erases_magnitude_and_lifetime_before_reapplication() {
    let mut conditions = Conditions::default();
    assert!(conditions.apply_stat_condition(DefenseDown, -10, 50, false));
    assert_eq!(conditions.defense_with_conditions(100), 90);
    conditions.cure(Cure::All);
    assert!(conditions.active_effects().is_empty());
    assert_eq!(conditions.remaining(DefenseDown), None);
    assert_eq!(conditions.magnitude(DefenseDown), 0);
    assert_eq!(conditions.defense_with_conditions(100), 100);
    assert!(conditions.apply_stat_condition(DefenseDown, -5, 20, false));
    assert_eq!(conditions.defense_with_conditions(100), 95);
    assert_eq!(conditions.remaining(DefenseDown), Some(20));
}

#[test]
fn stat_modifiers_keep_extreme_values_nonnegative_and_bounded() {
    let mut conditions = Conditions::default();
    conditions.apply_stat_condition(AttackUp, i16::MAX, 1, false);
    assert_eq!(conditions.attack_with_conditions(i32::MAX), i32::MAX);
    conditions.cure(Cure::All);
    conditions.apply_stat_condition(DefenseDown, i16::MAX, 1, false);
    assert_eq!(conditions.defense_with_conditions(i32::MAX), 0);
}

#[test]
fn stronger_refresh_preserves_magnitude_and_restarts_lifetime() {
    let mut owner = actor(Side::Party);
    owner
        .conditions
        .apply_stat_condition(AttackUp, 20, 3, false);
    owner.advance_conditions(true);
    owner
        .conditions
        .apply_stat_condition(AttackUp, 10, 5, false);
    assert_eq!(owner.conditions.attack_with_conditions(100), 120);
    assert_eq!(owner.conditions.remaining(AttackUp), Some(5));
    for _ in 0..4 {
        owner.advance_conditions(true);
    }
    assert_eq!(owner.conditions.remaining(AttackUp), Some(1));
    owner.advance_conditions(true);
    assert!(owner.conditions.effective().is_empty());
    assert_eq!(owner.conditions.magnitude(AttackUp), 0);
    assert_eq!(owner.conditions.attack_with_conditions(100), 100);
}

#[test]
fn opposite_stat_effects_cancel_without_applying_the_replacement() {
    for (up, down) in [
        (AttackUp, AttackDown),
        (DefenseUp, DefenseDown),
        (AccuracyUp, AccuracyDown),
        (MagicAttackUp, MagicAttackDown),
        (MagicDefenseUp, MagicDefenseDown),
    ] {
        let mut conditions = Conditions::default();
        assert!(conditions.apply_stat_condition(up, 10, 20, false));
        assert!(!conditions.apply_stat_condition(down, 10, 20, false));
        assert!(conditions.active_effects().is_empty());
        assert!(conditions.apply_stat_condition(down, 10, 20, false));
        assert_eq!(conditions.magnitude(down), -10);
    }
}

#[test]
fn stat_effects_expire_independently() {
    let mut owner = actor(Side::Party);
    owner
        .conditions
        .apply_stat_condition(AttackUp, 20, 1, false);
    owner
        .conditions
        .apply_stat_condition(DefenseUp, 10, 2, false);
    owner
        .conditions
        .apply_stat_condition(MagicAttackUp, 10, 3, false);
    owner
        .conditions
        .apply_stat_condition(MagicDefenseUp, 10, 2, false);
    owner.advance_conditions(true);
    assert_eq!(owner.conditions.attack_with_conditions(100), 100);
    assert_eq!(owner.conditions.defense_with_conditions(100), 110);
    owner.advance_conditions(true);
    assert_eq!(owner.conditions.defense_with_conditions(100), 100);
    assert_eq!(owner.conditions.remaining(MagicDefenseUp), None);
    assert_eq!(owner.conditions.remaining(MagicAttackUp), Some(1));
}

#[test]
fn equipment_grants_survive_cures_and_end_when_unequipped() {
    let mut conditions = Conditions::new(Layers {
        base: ConditionSet::of(&[Weak, Paralysis]),
        intrinsic: ConditionSet::of(&[CastingSpeed, RegenerateHp]),
        equipment_overlay: ConditionSet::of(&[Curse, Heavy]),
        ..Default::default()
    });
    let periodic = conditions.periodic_effects().to_vec();
    conditions.cure(Cure::All);
    assert!(conditions.active_effects().is_empty());
    assert!(conditions.base().is_empty());
    assert_eq!(
        conditions.effective(),
        ConditionSet::of(&[CastingSpeed, RegenerateHp, Curse, Heavy])
    );
    assert_eq!(conditions.periodic_effects(), periodic);
    assert!(!conditions.arte_queue_allowed());
    conditions.reload_layers(Layers::default());
    assert!(conditions.effective().is_empty());
    assert!(conditions.periodic_effects().is_empty());
    assert!(conditions.arte_queue_allowed());
}

#[test]
fn reload_preserves_only_effects_that_remain_active() {
    let mut conditions = Conditions::default();
    conditions.apply_stat_condition(AttackUp, 15, 17, false);
    let effects = conditions.active_effects().to_vec();
    conditions.reload_layers(Layers {
        base: AttackUp.into(),
        intrinsic: CastingSpeed.into(),
        ..Default::default()
    });
    assert_eq!(conditions.active_effects(), effects);
    conditions.reload_layers(Layers::default());
    conditions.reload_layers(Layers {
        base: AttackUp.into(),
        ..Default::default()
    });
    assert_eq!(conditions.magnitude(AttackUp), 10);
    assert_eq!(conditions.remaining(AttackUp), Some(1800));
}

#[test]
fn newly_imported_effects_use_recipient_duration_without_restarting_live_timers() {
    let mut conditions = Conditions::default().with_traits(Traits {
        extended_duration: true,
        ..Default::default()
    });
    // Immunity blocks future applications, not ailments already present at entry.
    let initial = ConditionSet::of(&[Paralysis, AttackUp]);
    conditions.initialize_profile(initial, ConditionSet::EMPTY, Paralysis.into());
    assert_eq!(conditions.remaining(Paralysis), Some(750));
    assert_eq!(conditions.remaining(AttackUp), Some(2250));
    conditions.advance(&mut None);
    let retained = conditions.active_effects().to_vec();
    conditions = conditions.with_traits(Traits::default());
    conditions.initialize_profile(initial, CastingSpeed.into(), Paralysis.into());
    conditions.reload_layers(conditions.layers());
    assert_eq!(conditions.active_effects(), retained);
    assert!(
        conditions
            .apply_hit(crate::HitCondition {
                condition: HitAilment::Paralysis,
                chance: 100,
                value: 0,
            })
            .is_none()
    );
    conditions.reload_layers(Layers {
        base: conditions.base().union(DefenseUp.into()),
        ..conditions.layers()
    });
    assert_eq!(
        conditions.remaining(DefenseUp),
        Some(DEFAULT_STAT_CONDITION_DURATION)
    );
    assert_eq!(conditions.remaining(Paralysis), Some(749));
}

#[test]
fn recipient_traits_control_immunity_duration_and_strength() {
    let mut conditions = Conditions::default().with_traits(Traits {
        extended_duration: true,
        ..Default::default()
    });
    assert!(
        conditions
            .apply_hit(crate::HitCondition {
                condition: HitAilment::Paralysis,
                chance: 100,
                value: 0
            })
            .is_some()
    );
    assert_eq!(conditions.remaining(Paralysis), Some(750));
    assert!(conditions.apply_stat_condition(DefenseDown, -10, 100, true));
    assert_eq!(conditions.magnitude(DefenseDown), -12);
    assert_eq!(conditions.remaining(DefenseDown), Some(125));
    assert!(conditions.apply_stat_condition(DefenseDown, -10, 70_000, true));
    assert_eq!(conditions.remaining(DefenseDown), Some(87_500));
    for _ in 0..87_499 {
        conditions.advance(&mut None);
    }
    assert_eq!(conditions.defense_with_conditions(100), 88);
    conditions.advance(&mut None);
    assert_eq!(conditions.defense_with_conditions(100), 100);
    assert!(conditions.apply_stat_condition(DefenseDown, -10, u32::MAX, false));
    assert_eq!(conditions.remaining(DefenseDown), Some(u32::MAX));
    conditions.advance(&mut None);
    assert_eq!(conditions.remaining(DefenseDown), Some(u32::MAX - 1));
    for conditions in [
        Conditions::new(Layers {
            immunity: DefenseDown.into(),
            ..Default::default()
        }),
        Conditions::default().with_traits(Traits {
            magical_ailment_guard: true,
            ..Default::default()
        }),
    ] {
        let mut changed = conditions.clone();
        assert!(!changed.apply_stat_condition(DefenseDown, -10, 100, false));
        assert_eq!(changed, conditions);
    }
}

#[test]
fn persistent_ailments_have_no_dummy_clocks() {
    let mut owner = actor(Side::Party);
    for condition in [HitAilment::Weak, HitAilment::Curse] {
        assert!(
            owner
                .conditions
                .apply_hit(crate::HitCondition {
                    condition,
                    chance: 100,
                    value: 0
                })
                .is_some()
        );
    }
    for _ in 0..1000 {
        owner.advance_conditions(true);
    }
    assert_eq!(
        owner.conditions.effective(),
        ConditionSet::of(&[Weak, Curse])
    );
    assert!(
        owner
            .conditions
            .active_effects()
            .iter()
            .all(|effect| effect.remaining.is_none())
    );
    assert!(owner.conditions.periodic_effects().is_empty());
}

#[test]
fn protection_has_an_explicit_persistent_or_timed_lifetime() {
    for persistent in [false, true] {
        let mut owner = actor(Side::Party);
        owner
            .conditions
            .prepare_buff(Buff::PhysicalAilmentGuard { persistent }, false)
            .commit(&mut owner);
        for _ in 0..900 {
            owner.advance_conditions(true);
        }
        assert_eq!(
            owner.conditions.effective().contains(PhysicalProtection),
            persistent
        );
        let hit = crate::HitCondition {
            condition: HitAilment::Paralysis,
            chance: 100,
            value: 0,
        };
        assert_eq!(owner.conditions.apply_hit(hit).is_none(), persistent);
        owner.conditions.cure(Cure::All);
        assert!(owner.conditions.apply_hit(hit).is_some());
    }
}

#[test]
fn quartz_replacement_and_expiry_clear_the_enchantment_together() {
    let mut owner = actor(Side::Party);
    for element in [Element::Fire, Element::Ice] {
        owner
            .conditions
            .prepare_buff(Buff::Quartz(element), false)
            .commit(&mut owner);
        assert_eq!(owner.elements.enchantment, Some(element));
    }
    for _ in 0..1199 {
        owner.advance_conditions(true);
    }
    assert_eq!(owner.elements.enchantment, Some(Element::Ice));
    owner.advance_conditions(true);
    assert_eq!(owner.elements.enchantment, None);
    assert!(
        !owner
            .conditions
            .base()
            .intersects(ConditionSet::of(&[Quartz, Enchanted]))
    );
    owner
        .conditions
        .prepare_buff(Buff::Quartz(Element::Wind), false)
        .commit(&mut owner);
    assert!(owner.conditions.cure(Cure::All));
    assert!(owner.conditions.active_effects().is_empty());
}

#[test]
fn rejected_buffs_do_not_mutate() {
    let mut owner = actor(Side::Party);
    owner.conditions = Conditions::new(Layers {
        immunity: Flare.into(),
        ..Default::default()
    });
    let before = owner.clone();
    assert_eq!(
        owner
            .conditions
            .prepare_buff(Buff::Flare, false)
            .commit(&mut owner),
        None
    );
    assert_eq!(owner, before);
}

#[test]
fn conditions_hold_for_phase_availability_and_global_pauses_not_local_hit_stop() {
    let mut owner = actor(Side::Party);
    owner
        .conditions
        .prepare_buff(Buff::Flare, false)
        .commit(&mut owner);
    owner.advance_conditions(false);
    for availability in [
        ActorAvailability::Absent,
        ActorAvailability::Dead,
        ActorAvailability::Petrified,
    ] {
        owner.availability = availability;
        owner.advance_conditions(true);
        assert_eq!(owner.conditions.remaining(Flare), Some(1200));
    }
    owner.availability = ActorAvailability::Active;
    owner.hit_stop = 10;
    let mut battle = prepared(vec![owner], 0).finish().unwrap();
    let before = battle.actors[0].conditions.clone();
    battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.actors[0].conditions, before);
    battle.advance_actor_common(0, &mut vec![]).unwrap();
    assert_eq!(battle.actors[0].conditions.remaining(Flare), Some(1199));
    assert_eq!(battle.actors[0].hit_stop, 9);
}

#[test]
fn paralysis_only_activates_for_grounded_affected_actors() {
    let mut owner = actor(Side::Party);
    owner.equipment.luck = 0;
    let mut random = crate::state::Random::new(1);
    assert!(!paralysis_controller_roll(&owner, &mut random));
    owner.conditions.apply_hit(crate::HitCondition {
        condition: HitAilment::Paralysis,
        chance: 100,
        value: 0,
    });
    owner.position[1] = 1.;
    assert!(!paralysis_controller_roll(&owner, &mut random));
    owner.position[1] = 0.;
    assert!(paralysis_controller_roll(&owner, &mut random));
}
