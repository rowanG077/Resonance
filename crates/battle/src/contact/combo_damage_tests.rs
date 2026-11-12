//! Damage that grows with the combo already on the target.
use super::*;

/// One 100-damage contact against a target already hit `hits` times.
fn contact(per_hit: u8, limit: u8, hits: i32, owner_side: Side) -> crate::HitResult {
    let victim = if owner_side == Side::Party {
        Side::Enemy
    } else {
        Side::Party
    };
    let mut owner = crate::tests::actor(owner_side);
    owner.equipment.damage.weapon_species = None;
    owner.equipment.damage.combo_damage_percent = per_hit;
    owner.equipment.damage.combo_damage_limit = limit;
    let mut target = crate::tests::actor(victim);
    target.hp = 40_000;
    target.equipment.max_hp = 100_000;
    target.equipment.stats.intelligence = 0;
    target.equipment.stats.level = 0;
    target.reaction.combo_hits = hits;
    target.body.collider = Some(crate::Collider::sphere(1.));
    let mut battle = crate::tests::prepared(vec![owner, target], 1)
        .finish()
        .unwrap();
    let cues = resolve_contact(
        &mut battle,
        Contact {
            radius: 2.,
            height: 2.,
            ..contact_fixture(rule(100))
        },
    )
    .unwrap();
    hit_result(&cues)
}

#[test]
fn each_earlier_combo_hit_adds_damage_up_to_the_limit_and_a_new_combo_starts_over() {
    for (hits, expected) in [(0, 100), (1, 101), (7, 107), (20, 120), (35, 120)] {
        assert_eq!(contact(1, 20, hits, Side::Party).amount, expected, "{hits}");
    }
    // A wearer on either side benefits; without the property nothing changes.
    assert_eq!(contact(1, 20, 5, Side::Enemy).amount, 105);
    assert_eq!(contact(0, 0, 12, Side::Party).amount, 100);
    // Steeper authored values use the same rule.
    assert_eq!(contact(5, 50, 4, Side::Party).amount, 120);
    assert_eq!(contact(5, 50, 30, Side::Party).amount, 150);
}
