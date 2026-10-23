use super::*;
use crate::tests::actor;

#[test]
fn casting_traits_respect_probability_boundaries_and_resumed_casts() {
    for (quick, random, rolls, expected) in [
        (true, false, vec![4], 1),
        (true, false, vec![5], 30),
        (false, true, vec![4], 1),
        (false, true, vec![5, 9], 60),
        (false, true, vec![5, 10], 30),
        (true, true, vec![4, 5, 9], 2),
    ] {
        let mut owner = actor(Side::Party);
        owner.equipment.casting.quick = quick;
        owner.equipment.casting.random = random;
        let mut rolls = rolls.into_iter();
        assert_eq!(
            initialize_random_clock(&owner, false, 30, || rolls.next().unwrap()),
            expected
        );
        assert!(rolls.next().is_none());
    }
    let mut owner = actor(Side::Party);
    owner.equipment.casting.quick = true;
    owner.equipment.casting.random = true;
    assert_eq!(
        initialize_random_clock(&owner, true, 30, || panic!("resumed cast must not reroll")),
        30
    );
    owner.side = Side::Enemy;
    assert_eq!(
        initialize_random_clock(&owner, false, 30, || panic!("enemy cast must not roll")),
        30
    );
    owner.side = Side::Party;
    owner.equipment.casting.quick = false;
    assert_eq!(
        initialize_random_clock(&owner, false, u32::MAX, || 5),
        u32::MAX
    );
    owner.equipment.luck = 999;
    assert_eq!(initialize_random_clock(&owner, false, 30, || 99), 30);
}

#[test]
fn lucky_casting_can_waive_the_final_tp_cost() {
    use crate::conditions::{Condition, ConditionSet, Conditions, Layers};
    let mut owner = actor(Side::Party);
    owner.equipment.casting.lucky_magic = true;
    owner.equipment.tp_cost_reduction = true;
    owner.conditions = Conditions::new(Layers {
        equipment_overlay: ConditionSet::of(&[Condition::TpHalf, Condition::TpThird]),
        ..Default::default()
    });
    let ordinary = spell_quote(&owner, 13, true);
    assert_eq!(commit_spell_cost(&owner, ordinary, || 4), (0, true));
    assert_eq!(commit_spell_cost(&owner, ordinary, || 5), (ordinary, false));
    owner.equipment.luck = 100;
    assert_eq!(commit_spell_cost(&owner, ordinary, || 14), (0, true));
    assert_eq!(
        commit_spell_cost(&owner, ordinary, || 15),
        (ordinary, false)
    );
    owner.side = Side::Enemy;
    assert_eq!(
        commit_spell_cost(&owner, spell_quote(&owner, 13, true), || panic!(
            "enemy cast must not roll"
        )),
        (13, false)
    );
}
