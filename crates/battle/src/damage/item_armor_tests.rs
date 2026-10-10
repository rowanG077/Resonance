use super::*;
use crate::{Control, HitProtection, ProtectionMode, Side, tests::actor};

fn rule(kind: DamageKind) -> HitRule {
    HitRule {
        overlimit_pause: true,
        kind,
        arte: false,
        power: Power::Fixed(8),
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

#[test]
fn item_armor_blocks_reaction_and_stun_while_preserving_damage() {
    let owner = actor(Side::Enemy);
    for control in [Control::Manual, Control::SemiAuto, Control::Auto] {
        let mut target = actor(Side::Party);
        target.control = control;

        target.guard.reduction = 75;
        target.guard.break_pressure = 10;
        target.reaction.stagger.received = 9;
        target.reaction.stagger.threshold = 10;
        target.reaction.protection.item();
        let before = target.reaction;
        let movement = target.movement;
        let mut random = Random::new(1);
        let rule = rule(DamageKind::Slash);
        let result = resolve(
            &owner,
            &mut target,
            rule,
            100,
            [0.; 3],
            &mut |_| random.next_u16(),
            false,
        );
        assert_eq!((result.amount, result.hp_change), (8, -8));
        assert_eq!(result.protection, HitProtection::Armored);
        assert_eq!(result.guard, GuardResult::None);
        assert!(!target.guard.active);
        assert!(
            crate::reaction::respond(
                &crate::tests::actor(Side::Enemy),
                &mut target,
                rule.reaction,
                result,
                [0.; 3],
                false
            )
            .is_none()
        );
        assert!(!crate::stun::roll(
            &owner,
            &target,
            255,
            result,
            &mut random
        ));

        assert_eq!(target.reaction, before);
        assert_eq!(target.movement, movement);
    }
}

#[test]
fn lethal_item_hit_replaces_the_protection_result_before_later_contact_processing() {
    let owner = actor(Side::Enemy);
    let mut target = actor(Side::Party);
    target.hp = 8;

    target.reaction.protection.item();
    let mut random = Random::new(1);
    let result = resolve(
        &owner,
        &mut target,
        rule(DamageKind::Slash),
        100,
        [0.; 3],
        &mut |_| random.next_u16(),
        false,
    );
    assert_eq!((target.hp, result.hp_change), (0, -8));
    assert_eq!(result.protection, HitProtection::None);
    assert_eq!(target.reaction.protection.mode, ProtectionMode::Armor);
}
