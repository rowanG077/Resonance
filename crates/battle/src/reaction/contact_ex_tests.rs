use super::*;
use crate::tests::actor;

#[test]
fn hitstun_modifiers_adjust_recovery_with_a_positive_floor() {
    let mut owner = actor(Side::Party);
    let mut target = actor(Side::Enemy);
    let base = hitstun(&owner, &target, 20);
    owner.equipment.contact.hard_hit = true;
    assert!(hitstun(&owner, &target, 20) > base);
    owner.equipment.contact = Default::default();
    owner.equipment.contact.air_brake = true;
    assert_eq!(hitstun(&owner, &target, 20), base);
    owner.movement.airborne_action = true;
    assert!(hitstun(&owner, &target, 20) > base);
    owner.equipment.contact = Default::default();
    target.equipment.contact.endure = true;
    assert!(hitstun(&owner, &target, 20) < base);
    target.reaction.combo_hits = i32::MAX;
    assert!(hitstun(&owner, &target, 20) > 0);
}
