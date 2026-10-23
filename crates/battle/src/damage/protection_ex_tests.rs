//! Defensive skill interactions.
use super::*;
use crate::conditions::Condition;
use crate::{
    Activity, ChargeLevel, Control, GuardKind, HitProtection, ProtectionMode, Side, tests::actor,
};
use DamageKind::{Magic, Slash};
use HitElement::{Inherited, Neutral};

const FIRE: HitElement = HitElement::Element(Element::Fire);

fn fixed(kind: DamageKind, element: HitElement) -> HitRule {
    HitRule {
        kind,
        overlimit_pause: true,
        power: Power::Fixed(201),
        element,
        reaction: crate::ReactionRule {
            hitstun: 60,
            armor_damage: 2,
            stagger: 7,
            ..Default::default()
        },
        ..crate::tests::hit()
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
    for (kind, element, guard, roll, stable) in [
        (Slash, Neutral, None, 9, true),
        (Slash, Neutral, None, 10, false),
        (Magic, Neutral, None, 0, false),
        (Magic, FIRE, None, 9, true),
        (Slash, Neutral, Some(GuardKind::Normal), 0, false),
        (Magic, FIRE, Some(GuardKind::Normal), 9, true),
        (Magic, FIRE, Some(GuardKind::Special), 9, false),
    ] {
        let mut target = defender();
        target.equipment.damage.physical_stability = true;
        target.equipment.damage.elemental_stability = true;
        target.guard.active = guard.is_some();
        target.guard.kind = guard.unwrap_or_default();
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
        fixed(Magic, Inherited),
        100,
        [0.; 3],
        &mut |draw| match draw {
            Draw::Stability => 0,
            _ => neutral_roll(draw),
        },
        false,
    );
    assert_eq!(result.protection, HitProtection::Armored);
    // Stability suppresses hurt, but the contact still consumes hit armor.
    assert_eq!(target.reaction.armor.received, 2);
    assert_eq!(target.reaction.stagger.received, 0);
}

#[test]
fn elemental_reduction_and_stability_have_independent_probability_boundaries() {
    let rule = fixed(Magic, FIRE);
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
        let (stable, _) = conditional_stability(&target, activity, Magic, None, &mut neutral_roll);
        assert_eq!(stable, expected);
    }
    let mut target = defender();
    target.movement.locomotion = crate::Locomotion::Run;
    target.equipment.damage.run_magic_stability = true;
    assert!(
        !hit(&mut target, fixed(Slash, Neutral), neutral_roll)
            .0
            .suppresses_reaction()
    );
}

#[test]
fn stored_spell_and_charge_protection_require_live_state_and_contact_element() {
    for charged in [false, true] {
        for element in [Neutral, FIRE] {
            let mut target = defender();
            target.equipment.damage.stored_spell_stability = true;
            target.stored_spell = charged.then_some(crate::ActionKey(102));
            let result = hit(&mut target, fixed(Slash, element), neutral_roll).0;
            assert_eq!(result.suppresses_reaction(), charged && element != Neutral);
            let mut target = defender();
            target.equipment.damage.charged_neutral_stability = true;
            target.control_ex_state.charge = if charged {
                ChargeLevel::Strong
            } else {
                ChargeLevel::None
            };
            let result = hit(&mut target, fixed(Magic, element), neutral_roll).0;
            assert_eq!(result.suppresses_reaction(), charged && element == Neutral);
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
        let (result, _) = hit(&mut target, fixed(Slash, FIRE), neutral_roll);
        assert_eq!(result.suppresses_reaction(), expected);
    }
}

#[test]
fn stability_keeps_down_shift_ailment_admission_and_lethal_normalization() {
    let mut target = defender();
    target.equipment.damage.stability = true;
    target.reaction.protection.mode = ProtectionMode::Down;
    target.reaction.stagger.window = 5;
    let mut rule = fixed(Slash, Neutral);
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
    let result = hit(&mut target, fixed(Slash, Neutral), neutral_roll).0;
    assert_eq!(target.hp, 0);
    assert_eq!(result.protection, HitProtection::None);
    assert!(!result.suppresses_reaction());
    assert!(result.is_unblocked_damage());
}

#[test]
fn forced_reaction_preserves_mitigation_rng_and_earlier_condition_decisions() {
    for (defense, allowed) in [
        ("guard", true),
        ("armor", true),
        ("protected", true),
        ("nullified", true),
        ("suppressed", true),
        ("overlimit", false),
        ("special", false),
        ("immune", false),
        ("absorb", false),
    ] {
        let mut states = Vec::new();
        for force in [false, true] {
            let mut target = defender();
            target.hp = 500;
            let mut rule = fixed(Slash, Neutral);
            rule.reaction.force = force;
            rule.guard.pressure = 2;
            rule.condition = Some(HitCondition {
                condition: Condition::Weak,
                chance: 100,
                value: 0,
            });
            match defense {
                "guard" | "special" => {
                    target.guard.active = true;
                    target.guard.pressure = 3;
                    if defense == "special" {
                        target.guard.kind = GuardKind::Special;
                    }
                }
                "armor" => target.reaction.armor.threshold = 10,
                "protected" => target.reaction.protection.mode = ProtectionMode::Recovery,
                "nullified" => target.equipment.damage.nullify_damage = true,
                "suppressed" => rule.reaction.suppress = true,
                "overlimit" => target.overlimit = crate::OverLimit::active(90).unwrap(),
                "immune" => target.equipment.affinities[0] = Affinity::Immune,
                "absorb" => target.equipment.affinities[0] = Affinity::Absorb,
                _ => unreachable!(),
            }
            let mut draws = Vec::new();
            let (result, _) = hit(&mut target, rule, |draw| {
                draws.push(draw as u8);
                0
            });
            assert_eq!(result.reaction_forced, force && allowed, "{defense}");
            if force && allowed {
                assert_eq!(result.guard, GuardResult::None);
                assert!(!result.suppresses_reaction());
                assert!(matches!(
                    crate::reaction::respond(
                        &actor(Side::Enemy),
                        &mut target,
                        rule.reaction,
                        result,
                        [1., 0., 0.],
                        false
                    ),
                    Some(crate::reaction::ContactReaction::Hurt { .. })
                ));
                if matches!(defense, "protected" | "nullified") {
                    assert_eq!((result.amount, result.hp_change), (201, 0));
                }
            }
            states.push((
                target.hp,
                target.guard.pressure,
                target.reaction.armor.received,
                target.reaction.stagger.received,
                target.conditions.base(),
                draws,
            ));
        }
        assert_eq!(states[0], states[1], "{defense}");
    }
}

#[test]
fn low_health_special_guard_avoids_damage_without_building_gauges() {
    for (hp, avoided) in [(100, true), (101, false)] {
        let mut target = defender();
        target.hp = hp;
        target.equipment.damage.low_hp_special_guard = true;
        target.guard.active = true;
        target.guard.kind = GuardKind::Special;
        let (result, label) = hit(&mut target, fixed(Slash, Neutral), neutral_roll);
        assert_eq!(result.hp_change, if avoided { 0 } else { -40 });
        assert_eq!(result.amount, if avoided { 0 } else { 40 });
        assert_eq!(result.protection == HitProtection::Avoided, avoided);
        assert!(target.guard.active);
        assert_eq!(
            result.guard,
            GuardResult::Blocked {
                first: false,
                special: true
            }
        );
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
        let (result, _) = hit(&mut target, fixed(Magic, Neutral), neutral_roll);
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
            fixed(Slash, Neutral),
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        assert_eq!(result.amount, expected);
    }
}

#[test]
fn rear_guard_bypasses_direction_only_and_charge_break_requires_attacker_skill() {
    let mut owner = actor(Side::Enemy);
    for (rear, charge, charge_guard, pressure, unbreakable, forced, blocked) in [
        (false, false, false, 0, false, false, false),
        (true, false, false, 0, false, false, true),
        (true, false, false, 10, false, false, false),
        (true, true, false, 0, false, false, true),
        (true, true, true, 0, false, false, false),
        (true, false, true, 0, false, false, true),
        (false, true, false, 10, true, false, true),
        (true, true, true, 0, true, true, false),
    ] {
        let mut target = defender();
        target.guard.active = true;
        target.guard.reduction = 75;
        target.equipment.damage.rear_guard = rear;
        owner.equipment.damage.charge_guard_break = charge_guard;
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
            Slash,
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
