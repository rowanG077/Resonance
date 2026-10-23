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
    use ConditionLabel::{Applied, EquipmentEffect, ExSkillEffect};
    for (luck, equipment, skill, incoming, saving, applied, label) in [
        (0, false, false, 99, 0, true, Some(Applied)),
        (0, false, true, 99, 20, true, Some(Applied)),
        (0, false, true, 99, 19, false, Some(ExSkillEffect)),
        (0, true, false, 99, 19, false, Some(EquipmentEffect)),
        (0, true, true, 99, 19, false, Some(ExSkillEffect)),
        (200, false, true, 89, 40, true, Some(Applied)),
        (200, false, true, 89, 39, false, Some(ExSkillEffect)),
        (200, false, true, 90, 0, false, None),
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
        let mut ailment_rolls = [incoming, saving].into_iter();
        let mut resistance_draws = Vec::new();
        let (hit, actual) = resolve_with_condition(
            &actor(Side::Party),
            &mut target,
            crate::Activity::Idle,
            ailment(),
            100,
            [0.; 3],
            &mut |draw| match draw {
                Draw::Ailment | Draw::AilmentResistance => {
                    resistance_draws.push(matches!(draw, Draw::AilmentResistance));
                    ailment_rolls.next().unwrap()
                }
                _ => neutral_roll(draw),
            },
            false,
            [2; 2],
        );
        assert_eq!(
            resistance_draws,
            if (equipment || skill) && incoming % 100 < 100 - luck / 20 {
                vec![false, true]
            } else {
                vec![false]
            }
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
                Draw::Ailment | Draw::AilmentResistance => 0,
                _ => neutral_roll(draw),
            },
            false,
            [2; 2],
        );
        assert_eq!(label, None);
        assert!(!target.conditions.effective().contains(Condition::Paralysis));
    }
}
