use super::*;

fn contact(
    divided: bool,
    victim: Side,
    kind: DamageKind,
    amount: u16,
    affinity: Affinity,
    guarded: bool,
) -> (Battle, crate::HitResult) {
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
    battle.items.all_divide = divided;
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
                overlimit_pause: true,
                condition: None,
                arte: true,
                kind,
                power: Power::Fixed(amount),
                element: HitElement::Neutral,
                prevents_defeat: false,
                reaction: Default::default(),
                guard: GuardRule {
                    enabled: true,
                    pressure: 0,
                    breaks: false,
                    unbreakable: false,
                },
            },
            clashes: false,
            struck: Vec::new(),
        })
        .unwrap();
    let mut cues = vec![];
    contacts.resolve(&mut battle, &mut cues).unwrap();
    let result = cues
        .iter()
        .find_map(|cue| match cue {
            Cue::Hit { result, .. } => Some(*result),
            _ => None,
        })
        .expect("accepted contact");
    (battle, result)
}

#[test]
fn all_divide_halves_damage_and_healing_on_both_sides() {
    for victim in [Side::Party, Side::Enemy] {
        for (affinity, change) in [
            (Affinity::Normal, -50),
            (Affinity::Absorb, 50),
            (Affinity::Immune, 0),
        ] {
            let (_, result) = contact(true, victim, DamageKind::Slash, 100, affinity, false);
            assert_eq!(result.hp_change, change);
            assert_eq!(result.amount, change.abs());
        }
    }
    let (_, guarded) = contact(
        true,
        Side::Enemy,
        DamageKind::Slash,
        100,
        Affinity::Normal,
        true,
    );
    assert_eq!(guarded.amount, 35);
    let (_, minimum) = contact(
        true,
        Side::Enemy,
        DamageKind::Slash,
        1,
        Affinity::Normal,
        false,
    );
    assert_eq!(minimum.hp_change, -1);
}
