use super::*;

fn contact(victim: Side, amount: u16, affinity: Affinity, guarded: bool) -> crate::HitResult {
    let side = if victim == Side::Party {
        Side::Enemy
    } else {
        Side::Party
    };
    let mut owner = crate::tests::actor(side);
    owner.equipment.damage.weapon_species = None;
    let mut target = crate::tests::actor(victim);
    target.hp = 40_000;
    target.equipment.max_hp = 100_000;
    target.equipment.stats.intelligence = 0;
    target.equipment.stats.level = 0;
    target.equipment.affinities[0] = affinity;
    target.guard.active = guarded;
    target.guard.reduction = 30;
    target.guard.break_pressure = 31;
    target.heading = 180.;
    target.facing_direction = [-1., 0., 0.];
    target.body.collider = Some(crate::Collider::sphere(1.));
    let mut battle = crate::tests::prepared(vec![owner, target], 1)
        .finish()
        .unwrap();
    battle.items.all_divide = true;
    let cues = resolve_contact(
        &mut battle,
        Contact {
            radius: 2.,
            height: 2.,
            ..contact_fixture(rule(amount))
        },
    )
    .unwrap();
    hit_result(&cues)
}

#[test]
fn all_divide_halves_damage_and_healing_on_both_sides() {
    for victim in [Side::Party, Side::Enemy] {
        for (affinity, change) in [
            (Affinity::Normal, -50),
            (Affinity::Absorb, 50),
            (Affinity::Immune, 0),
        ] {
            let result = contact(victim, 100, affinity, false);
            assert_eq!(result.hp_change, change);
            assert_eq!(result.amount, change.abs());
        }
    }
    let guarded = contact(Side::Enemy, 100, Affinity::Normal, true);
    assert_eq!(guarded.amount, 35);
    let minimum = contact(Side::Enemy, 1, Affinity::Normal, false);
    assert_eq!(minimum.hp_change, -1);
}
