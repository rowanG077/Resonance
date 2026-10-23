use super::*;
use crate::{DamageTraits, HitResult};

fn contact(kind: DamageKind, attacking: DamageTraits, defending: DamageTraits) -> HitResult {
    let mut owner = crate::tests::actor(Side::Party);
    owner.casting_state.power_percent = 100;
    owner.equipment.stats.slash = 400;
    owner.equipment.stats.intelligence = 200;
    owner.equipment.damage = attacking;
    let mut target = crate::tests::actor(Side::Enemy);
    target.hp = 1000;
    target.equipment.max_hp = 1000;
    target.equipment.damage = defending;
    target.body.collider = Some(crate::Collider::sphere(1.));
    let mut battle = crate::tests::prepare_actors(vec![owner, target], 1)
        .finish()
        .unwrap();
    let cues = resolve_contact(
        &mut battle,
        Contact {
            radius: 2.,
            height: 2.,
            ..contact_fixture(HitRule {
                kind,
                ..crate::tests::hit()
            })
        },
    )
    .unwrap();
    let hit = hit_result(&cues);
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
