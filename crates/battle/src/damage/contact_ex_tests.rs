use super::*;
use crate::{Activity, Control, HitProtection, Side, tests::actor};

fn rule(kind: DamageKind, power: Power) -> HitRule {
    HitRule {
        kind,
        power,
        overlimit_pause: true,
        ..crate::tests::hit()
    }
}

fn target() -> Actor {
    let mut target = actor(Side::Enemy);
    target.control = Control::Manual;
    target.hp = 1_000;
    target.equipment.max_hp = 1_000;
    target.guard.break_pressure = 10;
    target
}

#[test]
fn variable_uses_hp_quarters_before_proficiency() {
    let mut owner = actor(Side::Party);
    owner.equipment.damage.variable_attack = true;
    owner.equipment.max_hp = 2000;
    owner.equipment.stats.slash = 200;
    for (hp, expected) in [
        (2000, 200),
        (1500, 200),
        (1499, 216),
        (1000, 216),
        (999, 232),
        (500, 232),
        (499, 248),
        (1, 248),
    ] {
        owner.hp = hp;
        owner.proficiency = 0;
        assert_eq!(physical_attack(&owner, DamageKind::Slash), expected);
        owner.proficiency = 10;
        assert_eq!(
            physical_attack(&owner, DamageKind::Slash),
            expected * 110 / 100
        );
    }
}

#[test]
fn counter_guarantees_physical_criticals_against_attacking_opponents() {
    let mut owner = actor(Side::Party);
    owner.equipment.stats.slash = 200;
    owner.equipment.damage.physical_counter = true;
    let normal = resolve(
        &owner,
        &mut target(),
        rule(DamageKind::Slash, Power::Normal),
        100,
        [0.; 3],
        &mut neutral_roll,
        false,
    );
    assert!(!normal.critical);
    for (kind, power, critical) in [
        (DamageKind::Slash, Power::Normal, true),
        (DamageKind::Slash, Power::Fixed(19), false),
        (DamageKind::Magic, Power::Fixed(19), false),
    ] {
        let mut target = target();
        let result = resolve_with_condition(
            &owner,
            &mut target,
            Activity::Action,
            rule(kind, power),
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
            [2; 2],
        )
        .0;
        assert_eq!(result.critical, critical);
        if critical {
            assert!(result.amount > normal.amount);
        } else {
            assert_eq!(result.amount, 19);
        }
    }
}

#[test]
fn late_physical_bonuses_compound_without_consuming_charge() {
    let mut owner = actor(Side::Party);
    owner.equipment.stats.slash = 200;
    owner.equipment.stats.accuracy = 25;
    owner.equipment.contact.technique_balance = 100;
    owner.control_ex_state.charge = crate::ChargeLevel::Strong;
    owner.equipment.damage.elemental_physical_boost = true;
    owner.equipment.damage.weapon_species = Some(crate::Species::Human);
    let mut recipient = target();
    recipient.species = crate::Species::Human;
    let mut hit_rule = rule(DamageKind::Slash, Power::Normal);
    hit_rule.element = HitElement::Element(Element::Fire);
    // Each stage truncates: 100 -> 105 -> 147 -> 161 -> 181.
    for (reduced, expected) in [(false, 181), (true, 163)] {
        recipient.equipment.damage.damage_reduction = reduced;
        let hit = resolve(
            &owner,
            &mut recipient.clone(),
            hit_rule,
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        assert_eq!(hit.amount, expected);
        assert!(hit.boosted && !hit.critical);
        assert_eq!(owner.control_ex_state.charge, crate::ChargeLevel::Strong);
    }
    recipient.equipment.damage.damage_reduction = false;
    owner.side = Side::Enemy;
    assert_eq!(
        resolve(
            &owner,
            &mut recipient.clone(),
            hit_rule,
            100,
            [0.; 3],
            &mut neutral_roll,
            false
        )
        .amount,
        173
    );
    // Fixed physical hits skip early defenses, while the shared reduction still applies.
    recipient.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
        base: Condition::Guard.into(),
        ..Default::default()
    });
    recipient.equipment.damage.physical_damage_reduction = true;
    for (kind, power, reduced, expected, boosted) in [
        (DamageKind::Slash, 40, false, 68, true),
        (DamageKind::Slash, 40, true, 62, true),
        (DamageKind::Magic, 40, false, 40, false),
        (DamageKind::Magic, 40, true, 36, false),
        (DamageKind::Magic, 9, true, 9, false),
    ] {
        hit_rule.kind = kind;
        hit_rule.power = Power::Fixed(power);
        recipient.equipment.damage.damage_reduction = reduced;
        let hit = resolve(
            &owner,
            &mut recipient.clone(),
            hit_rule,
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        assert_eq!(hit.amount, expected);
        assert_eq!(hit.boosted, boosted);
        assert!(!hit.critical);
        assert_eq!(owner.control_ex_state.charge, crate::ChargeLevel::Strong);
    }
}

#[test]
fn nullification_reports_avoided_hits_and_keeps_failed_hits() {
    let owner = actor(Side::Party);
    for (avoidance, avoided) in [(4, true), (5, false)] {
        let mut target = target();
        target.equipment.damage.nullify_damage = true;
        let (hit, label) = resolve_with_condition(
            &owner,
            &mut target,
            Activity::Idle,
            rule(DamageKind::Slash, Power::Fixed(8)),
            100,
            [0.; 3],
            &mut |draw| match draw {
                Draw::Avoidance => avoidance,
                _ => neutral_roll(draw),
            },
            false,
            [2; 2],
        );
        assert_eq!(hit.protection == HitProtection::Avoided, avoided);
        assert_eq!(hit.hp_change, if avoided { 0 } else { -8 });
        assert_eq!(
            label,
            avoided.then_some(crate::conditions::ConditionLabel::ExSkillEffect)
        );
    }
}

#[test]
fn nullification_preserves_defenses_and_blocks_ailments() {
    let owner = actor(Side::Party);
    let mut target = target();
    target.equipment.damage.nullify_damage = true;
    target.reaction.armor.threshold = 10;
    target.reaction.protection.mode = crate::ProtectionMode::Down;
    target.reaction.stagger.window = 1;
    target.guard.active = true;
    let mut hit_rule = rule(DamageKind::Slash, Power::Fixed(64));
    hit_rule.reaction.armor_damage = 3;
    hit_rule.condition = Some(HitCondition {
        condition: resonance_content::battle_action::Condition::Curse,
        chance: 100,
        value: 0,
    });
    let (hit, label) = resolve_with_condition(
        &owner,
        &mut target,
        Activity::Idle,
        hit_rule,
        100,
        [0.; 3],
        &mut |draw| match draw {
            Draw::Avoidance => 0,
            _ => neutral_roll(draw),
        },
        false,
        [2; 2],
    );
    assert_eq!((hit.amount, hit.hp_change), (0, 0));
    assert_eq!(target.reaction.armor.received, 0);
    assert!(
        hit.protection == HitProtection::Avoided
            && hit.suppresses_reaction()
            && target.guard.active
    );
    assert_eq!(hit.guard, GuardResult::None);
    assert!(!hit.is_unblocked_damage());
    assert!(target.conditions.effective().is_empty());
    assert_eq!(
        label,
        Some(crate::conditions::ConditionLabel::ExSkillEffect)
    );
}

#[test]
fn suppression_stops_small_hits_before_guard_break_or_stagger() {
    let owner = actor(Side::Party);
    for (raw, divide, avoided) in [
        (9, false, true),
        (10, false, false),
        (19, true, true),
        (20, true, false),
    ] {
        let mut target = target();
        target.equipment.damage.suppress_small_hits = true;
        target.guard.active = true;
        let mut rule = rule(DamageKind::Slash, Power::Fixed(raw));
        rule.guard.breaks = true;
        rule.reaction.stagger = 3;
        let (hit, label) = resolve_with_condition(
            &owner,
            &mut target,
            Activity::Idle,
            rule,
            100,
            [0.; 3],
            &mut neutral_roll,
            divide,
            [2; 2],
        );
        assert_eq!(hit.protection == HitProtection::Avoided, avoided);
        assert_eq!(hit.hp_change, if avoided { 0 } else { -10 });
        assert_eq!(
            hit.guard,
            if avoided {
                GuardResult::None
            } else {
                GuardResult::Broken
            }
        );
        assert_eq!(target.guard.active, avoided);
        assert_eq!(
            target.reaction.stagger.received,
            if avoided { 0 } else { 3 }
        );
        assert_eq!(hit.is_unblocked_damage(), !avoided);
        assert_eq!(label, None);
    }
}
