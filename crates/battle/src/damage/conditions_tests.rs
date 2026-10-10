use super::*;
use crate::conditions::{Condition, ConditionSet};
use crate::{
    Side,
    conditions::{Conditions, Layers},
    tests::actor,
};
use resonance_content::battle_action::Condition as HitAilment;
fn hit(kind: DamageKind) -> HitRule {
    HitRule {
        overlimit_pause: true,
        kind,
        arte: false,
        power: Power::Normal,
        element: HitElement::Neutral,
        prevents_defeat: false,
        guard: GuardRule::default(),
        reaction: Default::default(),
        condition: None,
    }
}

#[test]
fn stat_conditions_change_damage_in_the_expected_direction_without_changing_base_stats() {
    for (kind, condition, magnitude, on_owner, increases) in [
        (DamageKind::Slash, Condition::AttackUp, 10, true, true),
        (DamageKind::Slash, Condition::AttackDown, -10, true, false),
        (DamageKind::Slash, Condition::DefenseUp, 10, false, false),
        (DamageKind::Slash, Condition::DefenseDown, -10, false, true),
        (DamageKind::Slash, Condition::AccuracyUp, 10, true, true),
        (DamageKind::Slash, Condition::EvasionDown, -10, false, true),
        (DamageKind::Magic, Condition::MagicAttackUp, 10, true, true),
        (
            DamageKind::Magic,
            Condition::MagicAttackDown,
            -10,
            true,
            false,
        ),
        (
            DamageKind::Magic,
            Condition::MagicDefenseUp,
            10,
            false,
            false,
        ),
        (
            DamageKind::Magic,
            Condition::MagicDefenseDown,
            -10,
            false,
            true,
        ),
    ] {
        let mut owner = actor(Side::Party);
        owner.equipment.stats.slash = 400;
        owner.equipment.stats.intelligence = 400;
        owner.equipment.stats.accuracy = 30;
        let mut target = actor(Side::Enemy);
        target.hp = 2000;
        target.equipment.max_hp = 2000;
        target.equipment.stats.defense = 40;
        target.equipment.stats.intelligence = 80;
        target.equipment.stats.evasion = 30;
        let baseline = resolve(
            &owner,
            &mut target.clone(),
            hit(kind),
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        let stats = (owner.equipment.stats, target.equipment.stats);
        let conditions = if on_owner {
            &mut owner.conditions
        } else {
            &mut target.conditions
        };
        apply_stat_rows(conditions, &[(condition, magnitude)]);
        let result = resolve(
            &owner,
            &mut target,
            hit(kind),
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        assert_ne!(result.amount, baseline.amount, "{condition:?}");
        assert_eq!(result.amount > baseline.amount, increases, "{condition:?}");
        assert_eq!(target.hp, 2000 - result.amount);
        assert_eq!((owner.equipment.stats, target.equipment.stats), stats);
    }
}

#[test]
fn physical_buffs_and_arte_bonus_affect_physical_hits_while_protection_reduces_both_kinds() {
    for kind in [DamageKind::Slash, DamageKind::Magic] {
        let mut owner = actor(Side::Party);
        owner.equipment.stats.slash = 400;
        owner.equipment.stats.intelligence = 400;
        let mut target = actor(Side::Enemy);
        target.hp = 2000;
        target.equipment.max_hp = 2000;
        let amount = |owner: &Actor, target: &Actor, arte| {
            let mut rule = hit(kind);
            rule.arte = arte;
            resolve(
                owner,
                &mut target.clone(),
                rule,
                100,
                [0.; 3],
                &mut neutral_roll,
                false,
            )
            .amount
        };
        let baseline = amount(&owner, &target, false);
        owner.conditions = Conditions::new(Layers {
            base: Condition::Flare.into(),
            ..Default::default()
        });
        assert_eq!(
            amount(&owner, &target, false) > baseline,
            kind == DamageKind::Slash
        );
        owner.conditions = Conditions::default();
        owner.equipment.damage.physical_arte_boost = true;
        assert_eq!(amount(&owner, &target, false), baseline);
        assert_eq!(
            amount(&owner, &target, true) > baseline,
            kind == DamageKind::Slash
        );
        owner.equipment.damage.physical_arte_boost = false;
        owner.conditions = Conditions::new(Layers {
            base: crate::conditions::PROTECTION,
            ..Default::default()
        });
        assert!(amount(&owner, &target, false) < baseline);
    }
}

fn apply_stat_rows(conditions: &mut Conditions, rows: &[(Condition, i16)]) {
    for &(condition, value) in rows {
        assert!(conditions.apply_stat_condition(condition, value, 900, false));
        assert!(conditions.effective().contains(condition));
    }
}

#[test]
fn zero_chance_leaves_conditions_unchanged_but_guard_break_admits_them() {
    let owner = actor(Side::Party);
    let mut plain = actor(Side::Enemy);
    let mut condition = plain.clone();
    let no_condition = hit(DamageKind::Slash);
    let mut with_condition = no_condition;
    with_condition.condition = Some(HitCondition {
        condition: resonance_content::battle_action::Condition::Weak,
        chance: 0,
        value: 0,
    });

    let mut random_plain = Random::new(1);
    resolve(
        &owner,
        &mut plain,
        no_condition,
        100,
        [0., 0., 1.],
        &mut |_| random_plain.next_u16(),
        false,
    );
    let mut random_condition = Random::new(1);
    let (_, label) = resolve_with_condition(
        &owner,
        &mut condition,
        crate::Activity::Idle,
        with_condition,
        100,
        [0., 0., 1.],
        &mut |_| random_condition.next_u16(),
        false,
        [2; 2],
    );
    assert_eq!(condition.hp, plain.hp);
    assert_eq!(label, None);
    assert_eq!(condition.conditions.effective(), ConditionSet::EMPTY);

    let mut target = actor(Side::Enemy);
    target.guard.active = true;
    target.guard.break_pressure = 0;
    let mut rule = hit(DamageKind::Slash);
    rule.condition = Some(HitCondition {
        condition: resonance_content::battle_action::Condition::Weak,
        chance: 100,
        value: 0,
    });
    let mut random = Random::new(1);
    let (result, label) = resolve_with_condition(
        &owner,
        &mut target,
        crate::Activity::Idle,
        rule,
        100,
        [0., 0., 1.],
        &mut |_| random.next_u16(),
        false,
        [2; 2],
    );
    assert_eq!(result.guard, GuardResult::Broken);
    assert!(!target.guard.active);
    assert_eq!(target.conditions.effective(), Condition::Weak.into());
    assert_eq!(label, Some(crate::conditions::ConditionLabel::StatusDown));
}

#[test]
fn curse_persists_after_contact_and_respects_target_immunity() {
    let owner = actor(Side::Party);
    let mut target = actor(Side::Enemy);
    let mut rule = hit(DamageKind::Slash);
    rule.condition = Some(HitCondition {
        condition: resonance_content::battle_action::Condition::Curse,
        chance: 100,
        value: 0,
    });
    let (_, label) = resolve_with_condition(
        &owner,
        &mut target,
        crate::Activity::Idle,
        rule,
        100,
        [0., 0., 1.],
        &mut neutral_roll,
        false,
        [2; 2],
    );
    assert_eq!(label, Some(crate::conditions::ConditionLabel::Applied));
    assert_eq!(target.conditions.effective(), Condition::Curse.into());
    assert_eq!(target.conditions.remaining(Condition::Curse), None);
    for _ in 0..600 {
        target.advance_conditions(true);
    }
    assert_eq!(target.conditions.effective(), Condition::Curse.into());

    target.conditions = Conditions::new(Layers {
        immunity: Condition::Curse.into(),
        ..Default::default()
    });
    let (_, label) = resolve_with_condition(
        &owner,
        &mut target,
        crate::Activity::Idle,
        rule,
        100,
        [0., 0., 1.],
        &mut neutral_roll,
        false,
        [2; 2],
    );
    assert_eq!(label, None);
    assert_eq!(target.conditions.effective(), ConditionSet::EMPTY);
}

#[test]
fn paralysis_applies_after_guard_break_and_refreshes_until_expiry() {
    let owner = actor(Side::Party);
    let mut target = actor(Side::Enemy);
    target.hp = 1000;
    target.equipment.max_hp = 1000;
    target.guard.active = true;
    target.guard.break_pressure = 10;
    let mut rule = hit(DamageKind::Slash);
    rule.power = Power::Fixed(1);
    rule.condition = Some(HitCondition {
        condition: HitAilment::Paralysis,
        chance: 100,
        value: 0,
    });
    let (_, label) = resolve_with_condition(
        &owner,
        &mut target,
        crate::Activity::Idle,
        rule,
        100,
        [0.; 3],
        &mut neutral_roll,
        false,
        [2; 2],
    );
    assert_eq!(label, None);
    assert!(target.conditions.effective().is_empty());
    rule.guard.breaks = true;
    let (result, label) = resolve_with_condition(
        &owner,
        &mut target,
        crate::Activity::Idle,
        rule,
        100,
        [0.; 3],
        &mut neutral_roll,
        false,
        [2; 2],
    );
    assert_eq!(result.guard, GuardResult::Broken);
    assert_eq!(label, Some(crate::conditions::ConditionLabel::Applied));
    assert_eq!(target.conditions.remaining(Condition::Paralysis), Some(600));
    target.advance_conditions(true);
    assert_eq!(target.conditions.remaining(Condition::Paralysis), Some(599));
    resolve(
        &owner,
        &mut target,
        rule,
        100,
        [0.; 3],
        &mut neutral_roll,
        false,
    );
    assert_eq!(target.conditions.remaining(Condition::Paralysis), Some(600));
    for _ in 0..600 {
        target.advance_conditions(true);
    }
    assert_eq!(target.conditions.remaining(Condition::Paralysis), None);
}

#[test]
fn hit_conditions_apply_through_armor_but_skip_defeated_targets() {
    let owner = actor(Side::Party);
    let mut armored = actor(Side::Enemy);
    armored.reaction.protection.mode = crate::ProtectionMode::Armor;
    armored.reaction.protection.remaining = 10;
    let mut rule = hit(DamageKind::Slash);
    rule.power = Power::Fixed(1);
    rule.condition = Some(HitCondition {
        condition: resonance_content::battle_action::Condition::Weak,
        chance: 100,
        value: 0,
    });
    let (result, label) = resolve_with_condition(
        &owner,
        &mut armored,
        crate::Activity::Idle,
        rule,
        100,
        [0., 0., 1.],
        &mut neutral_roll,
        false,
        [2; 2],
    );
    assert_eq!(result.protection, crate::HitProtection::Armored);
    assert_eq!(armored.conditions.effective(), Condition::Weak.into());
    assert_eq!(label, Some(crate::conditions::ConditionLabel::StatusDown));

    let mut lethal = actor(Side::Enemy);
    lethal.hp = 1;
    lethal.equipment.max_hp = 100;
    let (result, label) = resolve_with_condition(
        &owner,
        &mut lethal,
        crate::Activity::Idle,
        rule,
        100,
        [0., 0., 1.],
        &mut neutral_roll,
        false,
        [2; 2],
    );
    assert_eq!(lethal.hp, 0);
    assert!(lethal.conditions.effective().is_empty());
    assert_eq!(label, None);
    assert_eq!(result.protection, crate::HitProtection::None);
}

#[test]
fn ailment_guards_block_their_own_group_and_explicit_immunity_blocks_both() {
    use crate::conditions::Buff;
    let owner = actor(Side::Party);
    let physical = Buff::PhysicalAilmentGuard { persistent: false };
    let magical = Buff::MagicalAilmentGuard { persistent: false };
    for (buff, condition, immune, accepted) in [
        (physical, HitAilment::Curse, false, false),
        (physical, HitAilment::Weak, false, true),
        (magical, HitAilment::Weak, false, false),
        (magical, HitAilment::Curse, false, true),
        (physical, HitAilment::Weak, true, false),
        (magical, HitAilment::Curse, true, false),
    ] {
        let mut target = actor(Side::Enemy);
        target.hp = 1000;
        target.equipment.max_hp = 1000;
        target.conditions = Conditions::new(Layers {
            immunity: if immune {
                Condition::from(condition).into()
            } else {
                ConditionSet::EMPTY
            },
            ..Default::default()
        });
        assert!(
            target
                .conditions
                .prepare_buff(buff, false)
                .commit(&mut target)
                .is_some()
        );
        let before = target.conditions.clone();
        let mut rule = hit(DamageKind::Magic);
        rule.power = Power::Fixed(100);
        rule.condition = Some(HitCondition {
            condition,
            chance: 100,
            value: 0,
        });
        let (result, label) = resolve_with_condition(
            &owner,
            &mut target,
            crate::Activity::Idle,
            rule,
            100,
            [0., 0., 1.],
            &mut neutral_roll,
            false,
            [2; 2],
        );
        assert!(result.amount > 0);
        assert_eq!(target.hp, 1000 - result.amount);
        assert_eq!(label.is_some(), accepted, "{buff:?}/{condition:?}/{immune}");
        if accepted {
            assert!(target.conditions.effective().contains(condition.into()));
        } else {
            assert_eq!(target.conditions, before);
        }
    }
}

#[test]
fn defense_halving_affects_physical_damage_but_not_magic() {
    for kind in [DamageKind::Slash, DamageKind::Thrust, DamageKind::Magic] {
        for (condition, modifier) in [(Condition::DefenseUp, 30), (Condition::DefenseDown, -10)] {
            let owner = actor(Side::Party);
            let mut target = actor(Side::Enemy);
            target.hp = 1000;
            target.equipment.max_hp = 1000;
            target.equipment.stats.defense = 101;
            target.conditions = Conditions::new(Layers {
                equipment_overlay: Condition::DefenseHalved.into(),
                ..Default::default()
            });
            apply_stat_rows(&mut target.conditions, &[(condition, modifier)]);
            let mut expected = target.clone();
            expected.conditions.reload_layers(Layers {
                equipment_overlay: ConditionSet::EMPTY,
                ..expected.conditions.layers()
            });
            if kind != DamageKind::Magic {
                expected.equipment.stats.defense = 50;
            }
            let mut actual_random = Random::new(1);
            let mut expected_random = Random::new(1);
            let actual = resolve(
                &owner,
                &mut target,
                hit(kind),
                100,
                [0., 0., 1.],
                &mut |_| actual_random.next_u16(),
                false,
            );
            let expected_result = resolve(
                &owner,
                &mut expected,
                hit(kind),
                100,
                [0., 0., 1.],
                &mut |_| expected_random.next_u16(),
                false,
            );
            assert_eq!(actual, expected_result);
            assert_eq!(target.equipment.stats.defense, 101);
        }
    }
}
