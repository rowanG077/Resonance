use super::*;
use crate::{
    ActionId, ActorId, Cue, ProjectileContact, ProjectileDefinition, Side, contact::Contacts,
    tests::actor,
};
use std::sync::Arc;

fn rule(kind: DamageKind) -> HitRule {
    HitRule {
        kind,
        arte: true,
        overlimit_pause: true,
        power: Power::Normal,
        element: HitElement::Neutral,
        prevents_defeat: false,
        guard: Default::default(),
        reaction: Default::default(),
        condition: None,
    }
}

fn pair() -> (Actor, Actor) {
    let mut owner = actor(Side::Party);
    owner.equipment.stats.slash = 203;
    owner.equipment.stats.thrust = 151;
    owner.equipment.stats.accuracy = 99;
    owner.equipment.stats.intelligence = 157;
    let mut target = actor(Side::Enemy);
    target.hp = 1000;
    target.equipment.max_hp = 1000;
    target.equipment.stats.defense = 37;
    target.equipment.stats.evasion = 95;
    target.equipment.stats.intelligence = 24;
    (owner, target)
}

#[test]
fn proficiency_increases_physical_and_magic_damage_without_changing_base_stats() {
    for kind in [DamageKind::Slash, DamageKind::Thrust, DamageKind::Magic] {
        let (mut owner, target) = pair();
        let stats = owner.equipment.stats;
        let mut amounts = Vec::new();
        for proficiency in 0..=5 {
            owner.proficiency = proficiency;
            let mut recipient = target.clone();
            let hit = resolve(
                &owner,
                &mut recipient,
                rule(kind),
                135,
                [0.; 3],
                &mut neutral_roll,
                false,
            );
            assert_eq!(recipient.hp, target.hp - hit.amount);
            assert_eq!(owner.equipment.stats, stats);
            amounts.push(hit.amount);
        }
        assert!(amounts.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(amounts.last().unwrap() > &amounts[0]);
    }
}

#[test]
fn emitted_projectile_reads_changed_owner_proficiency_but_retains_combo_power() {
    for (emission_bonus, contact_bonus) in [(0, 5), (5, 0)] {
        let (mut owner, mut target) = pair();
        owner.proficiency = emission_bonus;
        owner.attack_power = 135;
        let stats = owner.equipment.stats;
        target.body.collider = Some(crate::Collider::sphere(1.));
        let mut battle = crate::tests::prepared(vec![owner, target], 1)
            .finish()
            .unwrap();
        battle
            .emit(
                Arc::new(ProjectileDefinition {
                    lifetime: Some(100),
                    velocity: [0.; 3],
                    acceleration: [0.; 3],
                    offset: [0.; 3],
                    clamp_ground: false,
                    active: None,
                    birth: None,
                    motion: Default::default(),
                    effects: Default::default(),
                    contact: Some(ProjectileContact {
                        hit: rule(DamageKind::Slash),
                        cooldown: 0,
                        repeat_limit: 0,
                        radius: 10.,
                        height: 10.,
                        shape: crate::HitShape::Box,
                        offset: [0.; 3],
                        radius_growth: 0.,
                        height_growth: 0.,
                        survives_contact: true,
                        clashes: false,
                    }),
                }),
                ActionId(1),
                ActorId(0),
                ActorId(1),
                [0.; 3],
            )
            .unwrap();
        let projectile_id = *battle.projectiles.keys().next().unwrap();
        assert_eq!(battle.projectiles[&projectile_id].attack_power, 135);
        // Projectiles use current proficiency while retaining their emitted attack power.
        battle.actors[0].proficiency = contact_bonus;
        battle.actors[0].attack_power = 10;
        let mut random = battle.random;
        let expected = resolve(
            &battle.actors[0],
            &mut battle.actors[1].clone(),
            rule(DamageKind::Slash),
            135,
            [0.; 3],
            &mut |_| random.next_u16(),
            false,
        )
        .amount;
        let projectile = battle.projectiles.get_mut(&projectile_id).unwrap();
        projectile.frame.contact_active = true;
        let mut contacts = Contacts::default();
        contacts.submit(projectile).unwrap();
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues).unwrap();
        let hit = cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Hit { result, .. } => Some(result),
                _ => None,
            })
            .unwrap();
        assert_eq!(hit.amount, expected);
        assert_eq!(battle.actors[1].hp, 1000 - expected);
        assert_eq!(battle.actors[0].equipment.stats, stats);
        assert_eq!(battle.projectiles[&projectile_id].attack_power, 135);
    }
}
