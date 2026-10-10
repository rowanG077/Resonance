use super::*;
use crate::{Control, HitProtection, ProtectionMode, Side, tests::actor};

fn fixed(kind: DamageKind, amount: u16) -> HitRule {
    HitRule {
        kind,
        arte: false,
        overlimit_pause: true,
        power: Power::Fixed(amount),
        element: HitElement::Neutral,
        prevents_defeat: false,
        guard: GuardRule::default(),
        reaction: crate::ReactionRule {
            hitstun: 60,
            stagger: 200,
            stun_chance: 255,
            ..Default::default()
        },
        condition: None,
    }
}

fn active() -> Actor {
    let mut target = actor(Side::Enemy);
    target.hp = 1000;
    target.equipment.max_hp = 1000;
    target.overlimit = crate::OverLimit::active(1000).unwrap();
    target
}

#[test]
fn guard_and_overlimit_use_the_strongest_reduction() {
    for (kind, guard, down, breaks, reduction, amount) in [
        (
            DamageKind::Slash,
            Some(GuardKind::Normal),
            false,
            false,
            75,
            25,
        ),
        (
            DamageKind::Slash,
            Some(GuardKind::Normal),
            false,
            false,
            255,
            1,
        ),
        (
            DamageKind::Magic,
            Some(GuardKind::Normal),
            false,
            false,
            75,
            50,
        ),
        (
            DamageKind::Magic,
            Some(GuardKind::Special),
            false,
            false,
            75,
            20,
        ),
        (
            DamageKind::Slash,
            Some(GuardKind::Normal),
            false,
            true,
            75,
            50,
        ),
        (DamageKind::Slash, None, true, false, 75, 25),
    ] {
        let mut target = active();
        target.guard.active = guard.is_some();
        target.guard.kind = guard.unwrap_or_default();
        target.guard.reduction = reduction;
        target.guard.break_pressure = 10;
        target.reaction.armor.threshold = 3;
        if down {
            target.reaction.protection.mode = ProtectionMode::Down;
        }
        let mut rule = fixed(kind, 100);
        rule.guard.breaks = breaks;
        let hit = resolve(
            &actor(Side::Party),
            &mut target,
            rule,
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        assert_eq!(hit.hp_change, -amount);
        assert_eq!(target.guard.active, guard.is_some() && !breaks);
        assert_eq!(hit.guard == GuardResult::Broken, breaks);
        assert!(hit.suppresses_reaction());
        assert_eq!(target.reaction.armor.received, 0);
    }
}

#[test]
fn active_contact_suppresses_automatic_guard_stagger_reaction_and_stun() {
    let owner = actor(Side::Party);
    let mut target = active();

    target.control = Control::Auto;
    target.guard.auto_chance = 100;
    target.reaction.stagger.received = 3;
    let rule = fixed(DamageKind::Slash, 100);
    let mut random = Random::new(1);
    let hit = resolve(
        &owner,
        &mut target,
        rule,
        100,
        [0., 0., -1.],
        &mut |_| random.next_u16(),
        false,
    );
    assert_eq!((hit.amount, target.hp), (50, 950));
    assert_eq!(hit.guard, GuardResult::None);
    assert_eq!(target.reaction.stagger.received, 3);
    assert!(
        crate::reaction::respond(
            &crate::tests::actor(Side::Enemy),
            &mut target,
            rule.reaction,
            hit,
            [0., 0., -1.],
            false
        )
        .is_none()
    );
    assert!(!crate::stun::roll(
        &owner,
        &target,
        rule.reaction.stun_chance,
        hit,
        &mut random
    ));

    assert!(target.time_stop == 0);
}

#[test]
fn active_lethal_damage_keeps_lifecycle_for_the_contact_death_owner() {
    let mut target = active();
    target.hp = 10;
    let hit = resolve(
        &actor(Side::Party),
        &mut target,
        fixed(DamageKind::Slash, 100),
        100,
        [0., 0., -1.],
        &mut neutral_roll,
        false,
    );
    assert_eq!((hit.amount, hit.hp_change, target.hp), (50, -10, 0));
    assert_eq!(
        (hit.guard, hit.protection),
        (GuardResult::None, HitProtection::None)
    );
    assert!(target.overlimit.is_active());
    assert_eq!(target.overlimit.remaining(), 1000);
}

#[test]
fn large_absorption_heals_to_capacity_without_reaction_or_stun() {
    let owner = actor(Side::Party);
    let mut target = active();
    target.hp = 500;
    target.equipment.affinities[0] = Affinity::Absorb;

    let rule = fixed(DamageKind::Slash, u16::MAX);
    let mut random = Random::new(1);
    let hit = resolve(
        &owner,
        &mut target,
        rule,
        100,
        [0.; 3],
        &mut |_| random.next_u16(),
        false,
    );
    assert!(hit.amount > 30_000);
    assert_eq!((hit.hp_change, target.hp), (500, 1000));
    assert_eq!(hit.affinity, Affinity::Absorb);
    assert!(target.overlimit.is_active());
    assert!(hit.suppresses_reaction());
    let before = target.reaction;
    assert!(
        crate::reaction::respond(&owner, &mut target, rule.reaction, hit, [0.; 3], false).is_none()
    );
    assert_eq!(target.reaction, before);

    assert!(!crate::stun::roll(&owner, &target, 255, hit, &mut random));
}
