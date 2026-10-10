//! Defensive skill interactions.
use super::*;
use crate::conditions::Condition;
use crate::{Activity, ChargeLevel, Control, HitProtection, ProtectionMode, Side, tests::actor};

fn fixed(kind: DamageKind, element: HitElement) -> HitRule {
    HitRule {
        kind,
        arte: false,
        overlimit_pause: true,
        power: Power::Fixed(201),
        element,
        prevents_defeat: false,
        guard: GuardRule::default(),
        reaction: crate::ReactionRule {
            hitstun: 60,
            armor_damage: 2,
            stagger: 7,
            ..Default::default()
        },
        condition: None,
    }
}
fn defender() -> Actor {
    let mut target = actor(Side::Party);
    target.hp = 1000;
    target.equipment.max_hp = 1000;
    target.guard.break_pressure = 10;
    target
}
fn hit(
    target: &mut Actor,
    rule: HitRule,
    mut roll: impl FnMut(Draw) -> u16,
) -> (HitResult, Option<crate::conditions::ConditionLabel>) {
    resolve_with_condition(
        &actor(Side::Enemy),
        target,
        Activity::Idle,
        rule,
        100,
        [0., 0., -1.],
        &mut roll,
        false,
        [2; 2],
    )
}

#[test]
fn stability_checks_contact_kind_element_guard_and_probability() {
    for (kind, element, guarded, roll, stable) in [
        (DamageKind::Slash, HitElement::Neutral, false, 9, true),
        (DamageKind::Slash, HitElement::Neutral, false, 10, false),
        (DamageKind::Magic, HitElement::Neutral, false, 0, false),
        (
            DamageKind::Magic,
            HitElement::Element(Element::Fire),
            false,
            9,
            true,
        ),
        (DamageKind::Slash, HitElement::Neutral, true, 0, false),
        (
            DamageKind::Magic,
            HitElement::Element(Element::Fire),
            true,
            9,
            false,
        ),
    ] {
        let mut target = defender();
        target.equipment.damage.physical_stability = true;
        target.equipment.damage.elemental_stability = true;
        target.guard.active = guarded;
        let (result, label) = hit(&mut target, fixed(kind, element), |draw| match draw {
            Draw::Stability => roll,
            _ => neutral_roll(draw),
        });
        assert_eq!(result.suppresses_reaction(), stable);
        assert_eq!(label.is_some(), stable);
    }
    let mut owner = actor(Side::Enemy);
    owner.equipment.base_element = Some(Element::Fire);
    let mut target = defender();
    target.equipment.damage.elemental_stability = true;
    target.reaction.armor.threshold = 3;
    let result = resolve(
        &owner,
        &mut target,
        fixed(DamageKind::Magic, HitElement::Inherited),
        100,
        [0.; 3],
        &mut |draw| match draw {
            Draw::Stability => 0,
            _ => neutral_roll(draw),
        },
        false,
    );
    assert_eq!(result.protection, HitProtection::Armored);
    assert_eq!(target.reaction.armor.received, 0);
    assert_eq!(target.reaction.stagger.received, 0);
}

#[test]
fn elemental_reduction_and_stability_have_independent_probability_boundaries() {
    let rule = fixed(DamageKind::Magic, HitElement::Element(Element::Fire));
    for (reduction, stability, reduced, stable) in [
        (4, 10, true, false),
        (5, 9, false, true),
        (4, 9, true, true),
        (5, 10, false, false),
    ] {
        let mut target = defender();
        target.equipment.affinities[Element::Fire as usize + 1] = Affinity::Weak;
        let baseline = hit(&mut target.clone(), rule, neutral_roll).0;
        target.equipment.damage.elemental_damage_reduction = true;
        target.equipment.damage.elemental_stability = true;
        let (result, label) = hit(&mut target, rule, |draw| match draw {
            Draw::ElementalReduction => reduction,
            Draw::Stability => stability,
            _ => neutral_roll(draw),
        });
        assert_eq!(
            result.amount,
            if reduced {
                baseline.amount / 2
            } else {
                baseline.amount
            }
        );
        assert_eq!(result.suppresses_reaction(), stable);
        assert_eq!(label.is_some(), reduced || stable);
    }
    for affinity in [Affinity::Absorb, Affinity::Immune] {
        let mut target = defender();
        target.hp = 500;
        target.equipment.affinities[Element::Fire as usize + 1] = affinity;
        let baseline = hit(&mut target.clone(), rule, neutral_roll).0;
        target.equipment.damage.elemental_damage_reduction = true;
        let result = hit(&mut target, rule, |draw| match draw {
            Draw::ElementalReduction => 0,
            _ => neutral_roll(draw),
        })
        .0;
        assert_eq!(result.hp_change, baseline.hp_change);
    }
}

#[test]
fn casting_and_dash_stability_follow_gameplay_state_independently_of_portraits() {
    for (activity, running, expected) in [
        (Activity::Casting { held: false }, false, true),
        (Activity::Casting { held: true }, false, false),
        (Activity::Recovering, false, false),
        (Activity::Idle, true, true),
        (Activity::Idle, false, false),
    ] {
        let mut target = defender();
        target.movement.locomotion = if running {
            crate::Locomotion::Run
        } else {
            crate::Locomotion::Idle
        };
        target.equipment.damage.casting_stability = true;
        target.equipment.damage.run_magic_stability = true;
        let (stable, _) = conditional_stability(
            &target,
            activity,
            DamageKind::Magic,
            None,
            &mut neutral_roll,
        );
        assert_eq!(stable, expected);
    }
    let mut target = defender();
    target.movement.locomotion = crate::Locomotion::Run;
    target.equipment.damage.run_magic_stability = true;
    assert!(
        !hit(
            &mut target,
            fixed(DamageKind::Slash, HitElement::Neutral),
            neutral_roll
        )
        .0
        .suppresses_reaction()
    );
}

#[test]
fn stored_spell_and_charge_protection_require_live_state_and_contact_element() {
    for charged in [false, true] {
        for element in [HitElement::Neutral, HitElement::Element(Element::Fire)] {
            let mut target = defender();
            target.equipment.damage.stored_spell_stability = true;
            target.stored_spell = charged.then_some(crate::ActionKey(102));
            let result = hit(&mut target, fixed(DamageKind::Slash, element), neutral_roll).0;
            assert_eq!(
                result.suppresses_reaction(),
                charged && element != HitElement::Neutral
            );
            let mut target = defender();
            target.equipment.damage.charged_neutral_stability = true;
            target.control_ex_state.charge = if charged {
                ChargeLevel::Strong
            } else {
                ChargeLevel::None
            };
            let result = hit(&mut target, fixed(DamageKind::Magic, element), neutral_roll).0;
            assert_eq!(
                result.suppresses_reaction(),
                charged && element == HitElement::Neutral
            );
        }
    }
    for (charge, running, expected) in [
        (ChargeLevel::None, true, false),
        (ChargeLevel::Normal, false, false),
        (ChargeLevel::Normal, true, true),
        (ChargeLevel::Strong, true, true),
    ] {
        let mut target = defender();
        target.equipment.damage.charged_run_stability = true;
        target.control_ex_state.charge = charge;
        target.movement.locomotion = if running {
            crate::Locomotion::Run
        } else {
            crate::Locomotion::Idle
        };
        let (result, _) = hit(
            &mut target,
            fixed(DamageKind::Slash, HitElement::Element(Element::Fire)),
            neutral_roll,
        );
        assert_eq!(result.suppresses_reaction(), expected);
    }
}

#[test]
fn stability_keeps_down_shift_ailment_admission_and_lethal_normalization() {
    let mut target = defender();
    target.equipment.damage.stability = true;
    target.reaction.protection.mode = ProtectionMode::Down;
    target.reaction.stagger.window = 5;
    let mut rule = fixed(DamageKind::Slash, HitElement::Neutral);
    rule.condition = Some(HitCondition {
        condition: resonance_content::battle_action::Condition::Weak,
        chance: 100,
        value: 0,
    });
    let (result, label) = hit(&mut target, rule, neutral_roll);
    assert_eq!(result.amount, 25);
    assert_eq!(result.protection, HitProtection::Reduced);
    assert_eq!(target.conditions.base(), Condition::Weak.into());
    assert_eq!(label, Some(crate::conditions::ConditionLabel::StatusDown));
    let mut target = defender();
    target.hp = 1;
    target.equipment.damage.stability = true;
    let result = hit(
        &mut target,
        fixed(DamageKind::Slash, HitElement::Neutral),
        neutral_roll,
    )
    .0;
    assert_eq!(target.hp, 0);
    assert_eq!(result.protection, HitProtection::None);
    assert!(!result.suppresses_reaction());
    assert!(result.is_unblocked_damage());
}

#[test]
fn low_health_special_guard_avoids_damage_without_building_gauges() {
    for (hp, avoided) in [(100, true), (101, false)] {
        let mut target = defender();
        target.hp = hp;
        target.equipment.damage.low_hp_special_guard = true;
        target.guard.active = true;
        target.guard.kind = GuardKind::Special;
        let (result, label) = hit(
            &mut target,
            fixed(DamageKind::Slash, HitElement::Neutral),
            neutral_roll,
        );
        assert_eq!(result.hp_change, if avoided { 0 } else { -40 });
        assert_eq!(result.amount, if avoided { 0 } else { 40 });
        assert_eq!(result.protection == HitProtection::Avoided, avoided);
        assert!(target.guard.active);
        assert!(!result.is_unblocked_damage());
        assert_eq!(label, None);
    }
}

#[test]
fn special_guard_survival_requires_an_active_stance() {
    for (kind, active, expected_hp) in [
        (GuardKind::Normal, true, 0),
        (GuardKind::Special, true, 1),
        (GuardKind::Special, false, 0),
    ] {
        let mut target = defender();
        target.hp = 1;
        target.guard.kind = kind;
        target.guard.active = active;
        target.equipment.damage.special_guard_survival = true;
        let (result, _) = hit(
            &mut target,
            fixed(DamageKind::Magic, HitElement::Neutral),
            neutral_roll,
        );
        assert_eq!(target.hp, expected_hp);
        if kind == GuardKind::Special && active {
            assert_eq!(
                result.guard,
                GuardResult::Blocked {
                    first: false,
                    special: true
                }
            );
            assert!(!result.is_unblocked_damage());
        }
    }
}

#[test]
fn guard_modifiers_compose_with_bounded_reduction_and_overlimit() {
    let mut owner = actor(Side::Enemy);
    owner.equipment.damage.guard_damage_boost = true;
    for (special, reduction, expected) in [(false, 75, 70), (false, 40, 100), (true, 75, 30)] {
        let mut target = defender();
        target.overlimit = crate::OverLimit::active(1000).unwrap();
        target.guard.active = true;
        target.guard.reduction = reduction;
        target.guard.kind = if special {
            GuardKind::Special
        } else {
            GuardKind::Normal
        };
        target.equipment.damage.special_guard_reduction = true;
        target.control_ex_state.guard_ready = true;
        let result = resolve(
            &owner,
            &mut target,
            fixed(DamageKind::Slash, HitElement::Neutral),
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        assert_eq!(result.amount, expected);
    }
}

#[test]
fn rear_guard_bypasses_direction_only_and_charge_break_immunity_requires_equipped_trait() {
    let mut owner = actor(Side::Enemy);
    for (rear, charge, charge_guard, pressure, unbreakable, forced, blocked) in [
        (false, false, false, 0, false, false, false),
        (true, false, false, 0, false, false, true),
        (true, false, false, 10, false, false, false),
        (true, true, false, 0, false, false, false),
        (true, true, true, 0, false, false, true),
        (false, true, false, 10, true, false, true),
        (true, true, true, 0, true, true, false),
    ] {
        let mut target = defender();
        target.guard.active = true;
        target.guard.reduction = 75;
        target.equipment.damage.rear_guard = rear;
        target.equipment.damage.single_charge_guard = charge_guard;
        owner.control_ex_state.charge = if charge {
            ChargeLevel::Normal
        } else {
            ChargeLevel::None
        };
        let rule = GuardRule {
            pressure,
            unbreakable,
            breaks: forced,
            ..Default::default()
        };
        let mut amount = 201;
        let result = guard::resolve_from(
            &owner,
            &mut target,
            DamageKind::Slash,
            rule,
            Affinity::Normal,
            [0., 0., 1.],
            &mut amount,
        );
        assert_eq!(matches!(result, GuardResult::Blocked { .. }), blocked);
        assert_eq!(amount, if blocked { 50 } else { 201 });
    }
}

#[test]
fn aerial_guard_requires_airborne_guard_intent_and_uses_random_policy_for_auto() {
    use crate::action_selection::{GroundMotion, InputIntent};
    for (height, guarding, attack, jump, face_target, expected) in [
        (0.1, true, false, false, false, false),
        (0.10001, true, false, false, false, true),
        (10., true, true, false, false, false),
        (10., true, false, true, false, false),
        (10., false, false, false, false, false),
        (10., true, false, false, true, true),
    ] {
        let mut target = defender();
        target.equipment.control_ex.aerial_guard = true;
        target.position[1] = height;
        target.input = InputIntent {
            motion: if guarding {
                GroundMotion::Guard
            } else {
                GroundMotion::Idle
            },
            action: attack.then_some(crate::action_selection::ActionCandidate {
                action: crate::ActionKey(0),
                range: [0., 100.],
            }),
            jump,
            face_target,
        };
        assert_eq!(
            guard::attempt(&mut target, Activity::Jumping, Affinity::Normal, || 0),
            expected
        );
    }
    for control in [Control::SemiAuto, Control::Auto] {
        for enabled in [false, true] {
            let mut target = defender();
            target.control = control;
            target.equipment.control_ex.aerial_guard = enabled;
            target.guard.auto_chance = 58;
            assert_eq!(
                guard::attempt(&mut target, Activity::Jumping, Affinity::Normal, || 0),
                enabled
            );
        }
    }
}

#[test]
fn equipment_refresh_replaces_protection_traits_and_preserves_live_guard() {
    let mut target = defender();
    target.guard.kind = GuardKind::Special;
    target.guard.active = true;
    target.guard.pressure = 7;
    target.equipment.damage.rear_guard = true;
    target.equipment.damage.physical_stability = true;
    target.species = 6;
    let prepared =
        crate::PreparedBattle::new(vec![(target, Default::default())], Default::default(), 17)
            .unwrap();
    let id = prepared.actor_ids().next().unwrap();
    let mut battle = prepared.finish().unwrap();
    let mut replacement = defender();
    replacement.equipment.damage.special_guard_reduction = true;
    replacement.equipment.damage.special_guard_survival = true;
    replacement.equipment.damage.casting_stability = true;
    crate::tests::equip(&mut battle, id, replacement).unwrap();
    let target = &battle.actors()[0];
    assert_eq!(
        (
            target.guard.kind,
            target.guard.active,
            target.guard.pressure
        ),
        (GuardKind::Special, true, 7)
    );
    assert_eq!(target.species, 6);
    assert!(!target.equipment.damage.rear_guard && !target.equipment.damage.physical_stability);
    assert!(
        target.equipment.damage.special_guard_reduction
            && target.equipment.damage.special_guard_survival
            && target.equipment.damage.casting_stability
    );
    assert_eq!(battle.random_state(), 17);
}
