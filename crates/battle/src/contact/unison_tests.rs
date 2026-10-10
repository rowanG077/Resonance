use super::*;

fn battle(side: Side, scenario: &str, unlocked: bool, targets: usize) -> Battle {
    let mut owner = crate::tests::actor(side);
    owner.control = Control::Auto;
    let mut target = crate::tests::actor(if side == Side::Party {
        Side::Enemy
    } else {
        Side::Party
    });
    target.hp = 1000;
    target.equipment.max_hp = 2000;
    target.facing_direction = [-1., 0., 0.];
    target.heading = 180.;
    target.body.collider = Some(crate::Collider::sphere(2.));
    match scenario {
        "guard" | "broken" => {
            target.guard.active = true;
            target.guard.break_pressure = 31;
        }
        "armor" => target.reaction.armor.threshold = 10,
        "avoid" => {
            target.equipment.damage.suppress_small_hits = true;
            target.equipment.max_hp = 10_000;
        }
        "absorb" => target.equipment.affinities[0] = Affinity::Absorb,
        "immune" => target.equipment.affinities[0] = Affinity::Immune,
        "overlimit" => target.overlimit = crate::OverLimit::active(1000).unwrap(),
        "lethal" => target.hp = 1,
        "ordinary" => {}
        _ => panic!("unknown test scenario"),
    }
    let mut actors = vec![owner];
    actors.extend((0..targets).map(|_| target.clone()));
    let prepared = crate::PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        1,
    )
    .unwrap()
    .with_unison_gauge(0, unlocked, false)
    .unwrap();
    prepared.finish().unwrap()
}

fn contacts(scenario: &str, position: [f32; 3]) -> Contacts {
    let mut contacts = Contacts::default();
    contacts
        .push(Contact {
            source: ContactSource::Melee {
                actor: ActorId(0),
                action: ActionId(1),
            },
            owner: ActorId(0),
            origin: Origin::World(position),
            radius: 20.,
            height: 20.,
            shape: HitShape::Box,
            rule: HitRule {
                overlimit_pause: true,
                kind: DamageKind::Slash,
                arte: true,
                power: Power::Fixed(54),
                element: HitElement::Neutral,
                prevents_defeat: false,
                reaction: Default::default(),
                guard: GuardRule {
                    enabled: true,
                    pressure: 0,
                    breaks: scenario == "broken",
                    unbreakable: false,
                },
                condition: None,
            },
            clashes: false,
            struck: Vec::new(),
        })
        .unwrap();
    contacts
}

fn ready(cues: &[Cue]) -> usize {
    cues.iter()
        .filter(|cue| matches!(cue, Cue::UnisonReady))
        .count()
}

#[test]
fn only_unblocked_party_damage_gains_unison_when_unlocked() {
    for (side, unlocked, scenario, expected) in [
        (Side::Enemy, true, "ordinary", 0),
        (Side::Party, false, "ordinary", 0),
        (Side::Party, true, "ordinary", 16),
        (Side::Party, true, "guard", 0),
        (Side::Party, true, "broken", 16),
        (Side::Party, true, "armor", 16),
        (Side::Party, true, "avoid", 0),
        (Side::Party, true, "absorb", 0),
        (Side::Party, true, "immune", 0),
        (Side::Party, true, "overlimit", 16),
        (Side::Party, true, "lethal", 16),
    ] {
        let mut battle = battle(side, scenario, unlocked, 1);
        contacts(scenario, [0.; 3])
            .resolve(&mut battle, &mut vec![])
            .unwrap();
        assert_eq!(
            battle.unison_gauge(),
            expected,
            "{side:?}/{scenario}/{unlocked}"
        );
    }
}

#[test]
fn unison_contacts_emit_ready_only_when_crossing_the_threshold() {
    for initial in [3183, 3184, 3200] {
        let mut battle = battle(Side::Party, "lethal", true, 1);
        battle.unison_gauge = initial;
        let mut cues = Vec::new();
        contacts("lethal", [0.; 3])
            .resolve(&mut battle, &mut cues)
            .unwrap();
        assert_eq!(battle.unison_gauge(), (initial + 16).min(3200));
        assert_eq!(ready(&cues), usize::from(initial == 3184));
    }
}

#[test]
fn unison_gain_counts_each_victim_and_ignores_missed_contacts() {
    let mut battle = battle(Side::Party, "ordinary", true, 2);
    battle.unison_gauge = 3170;
    let mut cues = Vec::new();
    contacts("ordinary", [0.; 3])
        .resolve(&mut battle, &mut cues)
        .unwrap();
    assert_eq!(
        cues.iter().filter(|c| matches!(c, Cue::Hit { .. })).count(),
        2
    );
    for cue in &cues {
        if let Cue::Hit {
            owner,
            position,
            element,
            was_casting,
            ..
        } = cue
        {
            assert_eq!(*owner, ActorId(0));
            assert!(position.iter().all(|value| value.is_finite()));
            assert_eq!(*element, None);
            assert!(!was_casting);
        }
    }
    assert_eq!(battle.unison_gauge(), 3200);
    assert_eq!(ready(&cues), 1);
    let mut missed = self::battle(Side::Party, "ordinary", true, 1);
    contacts("ordinary", [1000.; 3])
        .resolve(&mut missed, &mut vec![])
        .unwrap();
    assert_eq!(missed.unison_gauge(), 0);
}
