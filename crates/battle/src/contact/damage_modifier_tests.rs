use super::*;
use crate::{DamageTraits, HitResult, PreparedBattle};

fn contact(kind: DamageKind, attacking: DamageTraits, defending: DamageTraits) -> HitResult {
    let mut owner = crate::tests::actor(Side::Party);
    owner.equipment.stats.slash = 400;
    owner.equipment.stats.intelligence = 200;
    owner.equipment.damage = attacking;
    let mut target = crate::tests::actor(Side::Enemy);
    target.hp = 1000;
    target.equipment.max_hp = 1000;
    target.equipment.damage = defending;
    target.body.collider = Some(crate::Collider::sphere(1.));
    let mut battle = PreparedBattle::new(
        vec![(owner, Default::default()), (target, Default::default())],
        Default::default(),
        1,
    )
    .unwrap()
    .finish()
    .unwrap();
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
                arte: false,
                overlimit_pause: false,
                power: Power::Normal,
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
    contacts.resolve(&mut battle, &mut cues).unwrap();
    let hit = cues
        .into_iter()
        .find_map(|cue| match cue {
            Cue::Hit { result, .. } => Some(result),
            _ => None,
        })
        .unwrap();
    assert_eq!(battle.actors[1].hp, 1000 + hit.hp_change);
    assert_eq!(hit.hp_change, -hit.amount);
    hit
}

#[test]
fn contact_applies_attacker_and_recipient_equipment_modifiers() {
    for (kind, attacking, defending, increased) in [
        (
            DamageKind::Slash,
            DamageTraits {
                physical_damage_boost: true,
                ..Default::default()
            },
            DamageTraits::default(),
            true,
        ),
        (
            DamageKind::Slash,
            DamageTraits::default(),
            DamageTraits {
                physical_damage_reduction: true,
                ..Default::default()
            },
            false,
        ),
        (
            DamageKind::Magic,
            DamageTraits {
                magic_damage_boost: true,
                ..Default::default()
            },
            DamageTraits::default(),
            true,
        ),
        (
            DamageKind::Magic,
            DamageTraits::default(),
            DamageTraits {
                damage_reduction: true,
                ..Default::default()
            },
            false,
        ),
        (
            DamageKind::Slash,
            DamageTraits::default(),
            DamageTraits {
                damage_reduction: true,
                ..Default::default()
            },
            false,
        ),
    ] {
        let baseline = contact(kind, DamageTraits::default(), DamageTraits::default());
        let modified = contact(kind, attacking, defending);
        assert_ne!(modified.amount, baseline.amount);
        assert_eq!(modified.amount > baseline.amount, increased);
    }
}
