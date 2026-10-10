use super::*;
use crate::{ActorAvailability, DamageKind, GuardRule, HitElement, Power};

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
    let mut contacts = Contacts::default();
    contacts
        .push(Contact {
            source: ContactSource::Melee {
                actor: ActorId(0),
                action: ActionId(1),
            },
            owner: ActorId(0),
            origin: Origin::World([0.; 3]),
            radius: 2.,
            height: 2.,
            shape: HitShape::Box,
            rule: HitRule {
                kind,
                power,
                arte: false,
                overlimit_pause: true,
                element: HitElement::Neutral,
                prevents_defeat: false,
                guard: GuardRule::default(),
                reaction: Default::default(),
                condition: None,
            },
            clashes: false,
            struck: Vec::new(),
        })
        .unwrap();
    let mut cues = vec![];
    contacts.resolve(battle, &mut cues).unwrap();
    cues.into_iter()
        .find_map(|cue| match cue {
            Cue::Hit { result, .. } => Some(result),
            _ => None,
        })
        .unwrap()
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
fn defender_alone_reduces_physical_damage_including_fixed_power() {
    let mut battle = battle();
    let baseline = hit(&mut battle, DamageKind::Slash, Power::Normal).amount;
    battle.actors[1].equipment.damage.alone = true;
    let defended = hit(&mut battle, DamageKind::Slash, Power::Normal).amount;
    assert!(defended < baseline);
    battle.actors[2].availability = ActorAvailability::Dead;
    assert!(hit(&mut battle, DamageKind::Slash, Power::Normal).amount > defended);
    assert_eq!(
        hit(&mut battle, DamageKind::Slash, Power::Fixed(100)).amount,
        80
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
