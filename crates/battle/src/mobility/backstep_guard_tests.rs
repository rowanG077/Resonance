use super::*;
use crate::conditions::{Condition, ConditionSet};
use crate::{
    ActionId, ActorAvailability, DamageKind, GuardRule, HitElement, HitProtection, HitResult,
    HitRule, MeleeDefinition, Power, Protection, ProtectionMode, ReactionRule, contact::Contacts,
};

fn guarding(enabled: bool, control: Control) -> Battle {
    let mut battle = battle();
    battle.actors[0].equipment.backstep_guard = enabled;
    battle.actors[0].control = control;
    battle.step(input([0, 0], true, 0)).unwrap();
    assert!(!battle.is_diagnostic());
    assert_eq!(battle.activity(ActorId(0)), Activity::Guarding);
    battle
}

fn begin(battle: &mut Battle) -> crate::BattleFrame {
    let frame = battle.step(input([-70, 0], true, -1)).unwrap();
    assert!(!battle.is_diagnostic());
    assert_eq!(battle.activity(ActorId(0)), Activity::Evading);
    frame
}

fn contact(battle: &mut Battle, stun_chance: u8) -> HitResult {
    battle.actors[1].position = battle.actors[0].position;
    battle.actors[0].body.collider = Some(crate::Collider::sphere(2.));

    let mut contacts = Contacts::default();
    contacts
        .melee(
            ActorId(1),
            ActionId(1),
            &MeleeDefinition {
                hit: HitRule {
                    kind: DamageKind::Slash,
                    arte: false,
                    overlimit_pause: true,
                    power: Power::Fixed(9),
                    element: HitElement::Neutral,
                    prevents_defeat: false,
                    guard: GuardRule::default(),
                    reaction: ReactionRule {
                        hitstun: 60,
                        stagger: if stun_chance == 0 { 0 } else { 200 },
                        stun_chance,
                        ..Default::default()
                    },
                    condition: None,
                },
                trail: None,
                volume: crate::MeleeVolume {
                    offset: [0.; 3],
                    radius: 20.,
                    half_height: 20.,
                },
            },
            &[],
        )
        .unwrap();
    let mut cues = Vec::new();
    contacts.resolve(battle, &mut cues).unwrap();
    assert!(!battle.is_diagnostic());
    cues.iter()
        .find_map(|cue| match cue {
            crate::Cue::Hit { actor, result, .. } if *actor == ActorId(0) => Some(*result),
            _ => None,
        })
        .expect("the submitted contact must reach the backstepping actor")
}

#[test]
fn backstep_starts_moving_with_optional_armor() {
    for enabled in [false, true] {
        let mut battle = guarding(enabled, Control::Manual);

        let position = battle.actors[0].position;
        begin(&mut battle);
        let owner = &battle.actors[0];
        assert!(owner.position[0] < position[0]);
        assert!(owner.position[1] > position[1]);
        assert!(!owner.guard.active && !owner.movement.turning_disabled);
        assert_eq!(
            owner.reaction.protection.mode,
            if enabled {
                ProtectionMode::Armor
            } else {
                ProtectionMode::None
            }
        );
    }
}

#[test]
fn backstep_guard_requires_the_actual_guard_away_edge_and_excludes_jump() {
    for (stick, edge, held, hit_stop, blocked) in [
        ([-70, 0], 0, true, 0, false),
        ([70, 0], 1, true, 0, false),
        ([-29, 0], -1, true, 0, false),
        ([-70, 0], -1, false, 0, false),
        ([-70, 0], -1, true, 2, false),
        ([-70, 0], -1, true, 0, true),
    ] {
        let mut battle = guarding(true, Control::Manual);
        battle.actors[0].hit_stop = hit_stop;
        if blocked {
            battle.actors[0].conditions =
                crate::conditions::Conditions::new(crate::conditions::Layers {
                    intrinsic: ConditionSet::of(&[Condition::Heavy]),
                    ..Default::default()
                });
        }
        battle.step(input(stick, held, edge)).unwrap();
        assert!(!battle.is_diagnostic());
        assert_eq!(
            battle.activity(ActorId(0)),
            if held || hit_stop > 0 {
                Activity::Guarding
            } else {
                Activity::Idle
            }
        );
        assert_eq!(battle.actors[0].reaction.protection, Protection::default());
    }
    let mut idle = battle();
    idle.actors[0].equipment.backstep_guard = true;
    idle.step(input([-70, 0], true, -1)).unwrap();
    assert!(!idle.is_diagnostic());
    assert_eq!(idle.activity(ActorId(0)), Activity::Guarding);
    assert_eq!(idle.actors[0].reaction.protection, Protection::default());

    let mut jump = guarding(true, Control::Manual);
    for _ in 0..6 {
        jump.step(input([0, 70], true, 0)).unwrap();
        assert!(!jump.is_diagnostic());
    }
    assert_eq!(jump.activity(ActorId(0)), Activity::Jumping);
    assert_eq!(jump.actors[0].reaction.protection, Protection::default());

    let mut automatic = guarding(true, Control::Manual);
    automatic.actors[0].control = Control::Auto;
    automatic.step(input([-70, 0], true, -1)).unwrap();
    assert!(!automatic.is_diagnostic());
    assert_eq!(automatic.activity(ActorId(0)), Activity::Idle);
    assert_eq!(
        automatic.actors[0].reaction.protection,
        Protection::default()
    );
}

#[test]
fn backstep_guard_common_clock_holds_in_menus_and_expires_during_local_hit_stop() {
    let mut battle = guarding(true, Control::Manual);
    begin(&mut battle);
    let position = battle.actors[0].position;
    battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert!(!battle.is_diagnostic());
    assert_eq!(battle.actors[0].reaction.protection.remaining, 29);
    assert_eq!(battle.actors[0].position, position);
    battle.actors[0].hit_stop = 100;
    for visit in 1..=29 {
        battle.step(BattleInput::default()).unwrap();
        assert!(!battle.is_diagnostic());
        assert_eq!(battle.actors[0].hit_stop, 100 - visit);
        assert_eq!(
            battle.actors[0].reaction.protection.remaining,
            29 - u32::from(visit)
        );
        assert_eq!(
            battle.actors[0].reaction.protection.mode,
            if visit == 29 {
                ProtectionMode::None
            } else {
                ProtectionMode::Armor
            }
        );
    }
    let hit = contact(&mut battle, 0); // Disable stun/stagger for ordinary hurt.
    assert_eq!(hit.protection, HitProtection::None);
    assert_eq!(hit.hp_change, -9);
    assert_eq!(battle.activity(ActorId(0)), Activity::Hurt);
}

#[test]
fn backstep_armor_allows_damage_but_prevents_interruption() {
    for armored in [false, true] {
        let mut battle = guarding(armored, Control::Manual);
        battle.actors[1].tp = 0;
        begin(&mut battle);
        let hp = battle.actors[0].hp;
        let hit = contact(&mut battle, if armored { 255 } else { 0 });
        assert_eq!(battle.actors[0].hp, hp - 9);
        assert_eq!(hit.suppresses_reaction(), armored);
        assert_eq!(
            battle.activity(ActorId(0)),
            if armored {
                Activity::Evading
            } else {
                Activity::Hurt
            }
        );
        assert_eq!(battle.actors[1].tp, 1);
        for _ in 0..120 {
            battle.step(BattleInput::default()).unwrap();
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    }
}

#[test]
fn backstep_guard_lethal_contact_normalizes_result_and_clears_the_live_timer() {
    let mut battle = guarding(true, Control::Manual);
    battle.actors[0].hp = 1;
    begin(&mut battle);
    let hit = contact(&mut battle, 0);
    assert_eq!(hit.hp_change, -1);
    assert_eq!(hit.protection, HitProtection::None);
    assert!(!hit.suppresses_reaction());
    assert_eq!(battle.activity(ActorId(0)), Activity::Defeated);
    assert_eq!(battle.actors[0].availability, ActorAvailability::Dead);
    assert_eq!(battle.actors[0].reaction.protection, Protection::default());
}

#[test]
fn backstep_guard_equipment_and_mode_refresh_preserve_existing_entry_and_timer() {
    let mut battle = guarding(true, Control::Manual);
    begin(&mut battle);
    let before = battle.actors[0].reaction;
    let mut replacement = battle.actors[0].clone();
    replacement.equipment.backstep_guard = false;
    crate::tests::equip(&mut battle, ActorId(0), replacement).unwrap();
    assert!(!battle.actors[0].equipment.backstep_guard);
    assert_eq!(battle.actors[0].reaction, before);
    // Active movement continues after a control-mode edit.
    battle.actors[0].control = Control::Auto;
    battle.step(BattleInput::default()).unwrap();
    assert!(!battle.is_diagnostic());
    assert_eq!(battle.activity(ActorId(0)), Activity::Evading);
    assert_eq!(battle.actors[0].reaction.protection.remaining, 28);
    let mut replacement = battle.actors[0].clone();
    replacement.equipment.backstep_guard = true;
    crate::tests::equip(&mut battle, ActorId(0), replacement).unwrap();
    assert!(battle.actors[0].equipment.backstep_guard);
    assert_eq!(battle.actors[0].reaction.protection.remaining, 28);
}
