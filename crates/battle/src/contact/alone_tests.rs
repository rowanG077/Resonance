use super::*;
use crate::{ActorAvailability, DamageKind, Power};

fn battle() -> crate::Battle {
    let mut owner = crate::tests::actor(Side::Party);
    owner.equipment.stats.slash = 353;
    owner.equipment.stats.thrust = 353;
    owner.equipment.stats.accuracy = 32;
    owner.equipment.damage.alone = true;
    let mut target = crate::tests::actor(Side::Enemy);
    target.equipment.stats.defense = 23;
    target.equipment.stats.evasion = 11;
    target.hp = 20_000;
    target.equipment.max_hp = 20_000;
    target.body.collider = Some(crate::Collider::sphere(1.));
    let mut ally = crate::tests::actor(Side::Party);
    ally.position = [10_000.; 3];
    crate::tests::prepared(vec![owner, target, ally], 1)
        .finish()
        .unwrap()
}

fn hit(battle: &mut crate::Battle, kind: DamageKind, power: Power) -> crate::HitResult {
    battle.random = crate::state::Random::new(1);
    let cues = tests::resolve_contact(
        battle,
        Contact {
            radius: 2.,
            height: 2.,
            ..tests::contact_fixture(HitRule {
                kind,
                power,
                overlimit_pause: true,
                ..crate::tests::hit()
            })
        },
    )
    .unwrap();
    tests::hit_result(&cues)
}

#[test]
fn alone_rechecks_availability_each_contact_including_zero_hp_active_and_revival() {
    let mut battle = battle();
    let ordinary = hit(&mut battle, DamageKind::Slash, Power::Normal);
    assert!(ordinary.amount > 0);
    battle.actors[2].hp = 0;
    assert_eq!(
        hit(&mut battle, DamageKind::Slash, Power::Normal).amount,
        ordinary.amount
    );
    for unavailable in [
        ActorAvailability::Dead,
        ActorAvailability::Petrified,
        ActorAvailability::Absent,
    ] {
        battle.actors[2].availability = unavailable;
        assert!(hit(&mut battle, DamageKind::Slash, Power::Normal).amount > ordinary.amount);
    }
    battle.actors[2].availability = ActorAvailability::Active;
    battle.actors[2].hp = 1;
    assert_eq!(
        hit(&mut battle, DamageKind::Thrust, Power::Normal).amount,
        ordinary.amount
    );
}

#[test]
fn defender_alone_reduces_ordinary_physical_damage_but_fixed_power_bypasses_it() {
    let mut battle = battle();
    let baseline = hit(&mut battle, DamageKind::Slash, Power::Normal).amount;
    battle.actors[1].equipment.damage.alone = true;
    let defended = hit(&mut battle, DamageKind::Slash, Power::Normal).amount;
    assert!(defended < baseline);
    battle.actors[2].availability = ActorAvailability::Dead;
    assert!(hit(&mut battle, DamageKind::Slash, Power::Normal).amount > defended);
    assert_eq!(
        hit(&mut battle, DamageKind::Slash, Power::Fixed(100)).amount,
        100
    );
}

#[test]
fn alone_does_not_change_magic_damage() {
    let mut battle = battle();
    battle.actors[0].equipment.stats.intelligence = 200;
    let baseline = hit(&mut battle, DamageKind::Magic, Power::Normal);
    battle.actors[2].availability = ActorAvailability::Dead;
    battle.actors[1].equipment.damage.alone = true;
    assert_eq!(
        hit(&mut battle, DamageKind::Magic, Power::Normal).amount,
        baseline.amount
    );
}
