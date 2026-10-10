use super::*;
use crate::{
    Side,
    conditions::{ConditionLabel, Conditions, Layers},
    tests::actor,
};

fn ailment() -> HitRule {
    HitRule {
        condition: Some(HitCondition {
            condition: resonance_content::battle_action::Condition::Paralysis,
            chance: 100,
            value: 0,
        }),
        ..super::tests::rule(DamageKind::Slash, Power::Fixed(10))
    }
}

#[test]
fn ailment_chance_combines_luck_and_one_resistance_source() {
    for (luck, equipment, skill, roll, applied, label) in [
        (0, false, false, 99, true, Some(ConditionLabel::Applied)),
        (0, false, true, 49, true, Some(ConditionLabel::Applied)),
        (
            0,
            false,
            true,
            50,
            false,
            Some(ConditionLabel::ExSkillEffect),
        ),
        (
            0,
            true,
            false,
            50,
            false,
            Some(ConditionLabel::EquipmentEffect),
        ),
        (
            0,
            true,
            true,
            50,
            false,
            Some(ConditionLabel::ExSkillEffect),
        ),
        (200, false, true, 44, true, Some(ConditionLabel::Applied)),
        (
            200,
            false,
            true,
            45,
            false,
            Some(ConditionLabel::ExSkillEffect),
        ),
        (200, false, true, 90, false, None),
    ] {
        let mut target = actor(Side::Enemy);
        target.equipment.luck = luck;
        target.equipment.damage.ailment_resistance = skill;
        if equipment {
            target.conditions = Conditions::new(Layers {
                intrinsic: Condition::AilmentResistance.into(),
                ..Default::default()
            });
        }
        let (hit, actual) = resolve_with_condition(
            &actor(Side::Party),
            &mut target,
            crate::Activity::Idle,
            ailment(),
            100,
            [0.; 3],
            &mut |draw| match draw {
                Draw::Ailment => roll,
                _ => neutral_roll(draw),
            },
            false,
            [2; 2],
        );
        assert_eq!(actual, label);
        assert_eq!(
            target.conditions.effective().contains(Condition::Paralysis),
            applied
        );
        assert_eq!(hit.hp_change, -10);
    }
}

#[test]
fn impossible_ailments_do_not_apply_or_report_resistance() {
    for immune in [false, true] {
        let mut target = actor(Side::Enemy);
        target.equipment.damage.ailment_resistance = true;
        let mut rule = ailment();
        if immune {
            target.conditions = Conditions::new(Layers {
                immunity: Condition::Paralysis.into(),
                ..Default::default()
            });
        } else {
            rule.condition.as_mut().unwrap().chance = 0;
        }
        let (_, label) = resolve_with_condition(
            &actor(Side::Party),
            &mut target,
            crate::Activity::Idle,
            rule,
            100,
            [0.; 3],
            &mut |draw| match draw {
                Draw::Ailment => 0,
                _ => neutral_roll(draw),
            },
            false,
            [2; 2],
        );
        assert_eq!(label, None);
        assert!(!target.conditions.effective().contains(Condition::Paralysis));
    }
}
