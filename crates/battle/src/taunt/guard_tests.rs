use super::*;
use crate::{
    ActorAvailability, Control, DamageKind, GuardResult, GuardRule, HitElement, HitProtection,
    HitRule, Power, Protection, ProtectionMode, ReactionRule, RecoilRule,
};

fn guarded() -> Battle {
    let mut prepared = prepared(0, true);
    prepared.actors[0].equipment.taunt_guard = true;
    prepared.finish().unwrap()
}

fn hit_rule(kind: DamageKind) -> HitRule {
    HitRule {
        kind,
        arte: false,
        overlimit_pause: true,
        power: Power::Fixed(8),
        element: HitElement::Neutral,
        prevents_defeat: false,
        guard: GuardRule::default(),
        reaction: ReactionRule {
            hitstun: 60,
            stagger: 200,
            stun_chance: 255,
            recoil: RecoilRule {
                impulse: [8., 0.],
                ..Default::default()
            },
            ..Default::default()
        },
        condition: None,
    }
}

#[test]
fn taunt_and_armor_pause_together_and_complete_during_hit_stop() {
    let mut battle = guarded();
    battle.step(input(true, true)).unwrap();
    let remaining = TAUNT_DURATION_TICKS - 1;
    battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Taunting);
    assert_eq!(battle.actors[0].reaction.protection.remaining, remaining);
    battle.actors[0].hit_stop = (TAUNT_DURATION_TICKS + 1) as u8;
    for _ in 1..remaining {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.activity(ActorId(0)), Activity::Taunting);
    assert_eq!(
        battle.actors[0].reaction.protection.mode,
        ProtectionMode::Armor
    );
    assert_eq!(battle.unison_gauge(), 0);
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    assert_eq!(battle.actors[0].reaction.protection, Protection::default());
    assert_eq!(battle.unison_gauge(), TAUNT_GAUGE_GAIN);
}

#[test]
fn taunt_guard_contacts_keep_full_damage_without_recoil_stagger_stun_or_interruption() {
    for kind in [DamageKind::Slash, DamageKind::Magic] {
        let mut battle = guarded();
        battle.actors[0].control = Control::Manual;
        battle.actors[1].tp = 0;
        battle.step(input(true, true)).unwrap();
        let rule = hit_rule(kind);
        let (hit, _) = contact(&mut battle, rule);
        assert_eq!((hit.amount, hit.hp_change), (8, -8));
        assert_eq!(hit.protection, HitProtection::Armored);
        assert_eq!(hit.guard, GuardResult::None);
        assert_eq!(hit.protection, crate::HitProtection::Armored);
        assert_eq!(battle.activity(ActorId(0)), Activity::Taunting);
        assert!(battle.ledger.party_was_hit);
        assert_eq!(
            battle.actors[1].tp, 1,
            "armor keeps ordinary contact TP gain"
        );
        assert_eq!(
            battle.unison_gauge(),
            0,
            "enemy contact is not party gauge gain"
        );
        for _ in 1..TAUNT_DURATION_TICKS {
            battle.step(BattleInput::default()).unwrap();
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert_eq!(
            battle.unison_gauge(),
            48,
            "the actual Taunt still completes"
        );
    }
}

#[test]
fn taunt_guard_does_not_prevent_lethal_contact_or_clear_a_longer_timer_on_completion() {
    let mut lethal = guarded();
    lethal.actors[0].control = Control::Manual;
    lethal.actors[0].hp = 1;
    lethal.step(input(true, true)).unwrap();
    let mut rule = hit_rule(DamageKind::Slash);
    rule.reaction.stun_chance = 0;
    rule.reaction.stagger = 0;
    let (hit, _) = contact(&mut lethal, rule);
    assert_eq!(hit.hp_change, -1);
    assert_eq!(
        hit.protection,
        HitProtection::None,
        "lethal damage ends armor protection"
    );
    assert!(!hit.suppresses_reaction());
    assert_eq!(lethal.activity(ActorId(0)), Activity::Defeated);
    assert_eq!(lethal.actors[0].availability, ActorAvailability::Dead);
    assert_eq!(lethal.actors[0].reaction.protection, Protection::default());
    assert_eq!(lethal.unison_gauge(), 0);

    let mut longer = guarded();
    longer.actors[0].reaction.protection = Protection {
        mode: ProtectionMode::Armor,
        remaining: TAUNT_DURATION_TICKS + 30,
    };
    longer.step(input(true, true)).unwrap();
    for _ in 1..TAUNT_DURATION_TICKS {
        longer.step(BattleInput::default()).unwrap();
    }
    assert_eq!(longer.activity(ActorId(0)), Activity::Idle);
    assert_eq!(longer.unison_gauge(), 48);
    assert_eq!(
        longer.actors[0].reaction.protection,
        Protection {
            mode: ProtectionMode::Armor,
            remaining: 30,
        }
    );
}

#[test]
fn taunt_guard_is_not_granted_without_actual_taunt_admission() {
    for (taunt_enabled, gauge, availability) in [
        (false, 0, ActorAvailability::Active),
        (true, 3200, ActorAvailability::Active),
        (true, 0, ActorAvailability::Dead),
        (true, 0, ActorAvailability::Petrified),
        (true, 0, ActorAvailability::Absent),
    ] {
        let mut battle = guarded();
        battle.actors[0].equipment.taunt_enabled = taunt_enabled;
        battle.unison_gauge = gauge;
        battle.actors[0].availability = availability;
        battle.actors[0].hp = if availability == ActorAvailability::Dead {
            0
        } else {
            50
        };
        battle.step(input(true, true)).unwrap();
        assert_ne!(battle.activity(ActorId(0)), Activity::Taunting);
        assert_eq!(battle.actors[0].reaction.protection, Protection::default());
    }
}
