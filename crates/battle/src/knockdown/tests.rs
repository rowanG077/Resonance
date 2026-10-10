use super::*;
use crate::{
    ActionId, Affinity, BattleInput, Control, Cue, DamageKind, GuardResult, GuardRule, HitElement,
    HitRule, MeleeDefinition, Power, PreparedBattle, ReactionRule, tests::actor,
};

fn hit() -> HitRule {
    HitRule {
        overlimit_pause: false,
        condition: None,
        arte: false,
        kind: DamageKind::Slash,
        power: Power::Fixed(32),
        element: HitElement::Neutral,
        prevents_defeat: false,
        guard: GuardRule::default(),
        reaction: ReactionRule {
            stagger: 5,
            hitstun: 20,
            ..Default::default()
        },
    }
}

fn damage(target: &mut Actor, rule: HitRule) -> crate::HitResult {
    crate::damage::resolve(
        &actor(Side::Party),
        target,
        rule,
        100,
        [0.; 3],
        &mut |_| 99,
        false,
    )
}

#[test]
fn protection_preserves_side_window_and_hits_down_rules() {
    for (mode, side, window, hits_down, protection, amount, hp) in [
        (
            ProtectionMode::Down,
            Side::Party,
            0,
            false,
            HitProtection::Avoided,
            0,
            50,
        ),
        (
            ProtectionMode::Down,
            Side::Enemy,
            0,
            false,
            HitProtection::Reduced,
            8,
            42,
        ),
        (
            ProtectionMode::Down,
            Side::Party,
            1,
            false,
            HitProtection::Reduced,
            4,
            46,
        ),
        (
            ProtectionMode::Down,
            Side::Enemy,
            1,
            false,
            HitProtection::Reduced,
            4,
            46,
        ),
        (
            ProtectionMode::Down,
            Side::Party,
            1,
            true,
            HitProtection::Armored,
            32,
            18,
        ),
        (
            ProtectionMode::Down,
            Side::Enemy,
            1,
            true,
            HitProtection::Armored,
            32,
            18,
        ),
        (
            ProtectionMode::Down,
            Side::Party,
            0,
            true,
            HitProtection::Avoided,
            0,
            50,
        ),
        (
            ProtectionMode::Recovery,
            Side::Party,
            1,
            true,
            HitProtection::Avoided,
            0,
            50,
        ),
        (
            ProtectionMode::Recovery,
            Side::Enemy,
            1,
            true,
            HitProtection::Reduced,
            4,
            46,
        ),
    ] {
        let mut target = actor(side);
        target.reaction.protection.mode = mode;
        target.reaction.stagger.window = window;
        target.reaction.armor.threshold = 1;
        let mut rule = hit();
        rule.reaction.hits_down = hits_down;
        rule.reaction.armor_damage = 3;
        let result = damage(&mut target, rule);
        assert_eq!(
            (result.protection, result.amount, target.hp),
            (protection, amount, hp)
        );
        assert_eq!(
            target.reaction.armor.received,
            if protection == HitProtection::Armored {
                target.reaction.armor.threshold
            } else {
                0
            }
        );
        assert_eq!(target.reaction.stagger.received, 0);
    }
    let mut target = actor(Side::Party);
    target.reaction.protection.mode = ProtectionMode::Recovery;
    target.equipment.affinities[0] = Affinity::Absorb;
    assert_eq!(damage(&mut target, hit()).hp_change, 32); // Absorption precedes avoidance.
    target.equipment.affinities[0] = Affinity::Immune;
    assert_eq!(damage(&mut target, hit()).hp_change, 0);
    target.side = Side::Enemy;
    target.hp = 1;
    target.equipment.affinities[0] = Affinity::Normal;
    let result = damage(&mut target, hit());
    assert_eq!(target.hp, 0);
    assert_eq!(result.protection, HitProtection::None); // Lethal flags replace protection.
}

#[test]
fn armor_preserves_longer_and_stronger_protection() {
    for (mode, remaining, expected_mode, expected_remaining) in [
        (ProtectionMode::Armor, 3, ProtectionMode::Armor, 10),
        (ProtectionMode::Armor, 40, ProtectionMode::Armor, 40),
        (
            ProtectionMode::Armor,
            u32::MAX,
            ProtectionMode::Armor,
            u32::MAX,
        ),
        (ProtectionMode::Recovery, 2, ProtectionMode::Recovery, 2),
        (ProtectionMode::Escape, 180, ProtectionMode::Escape, 180),
    ] {
        let mut protection = Protection { mode, remaining };
        protection.armor(10);
        assert_eq!(
            protection,
            Protection {
                mode: expected_mode,
                remaining: expected_remaining
            }
        );
    }
}

#[test]
fn buildup_saturates_and_only_guard_break_allows_guarded_buildup() {
    let mut target = actor(Side::Enemy);
    target.reaction.stagger.received = 253;
    damage(&mut target, hit());
    assert_eq!(target.reaction.stagger.received, u8::MAX);
    target.reaction.stagger.received = 2;
    for affinity in [Affinity::Absorb, Affinity::Immune] {
        target.equipment.affinities[0] = affinity;
        damage(&mut target, hit());
        assert_eq!(target.reaction.stagger.received, 2);
    }
    target.equipment.affinities[0] = Affinity::Normal;
    target.hp = 100;
    target.time_stop = u16::MAX;
    damage(&mut target, hit());
    assert_eq!(target.reaction.stagger.received, 2);
    target.time_stop = 0;
    target.guard.active = true;
    target.guard.break_pressure = 10;
    target.guard.reduction = 75;
    assert!(matches!(
        damage(&mut target, hit()).guard,
        GuardResult::Blocked { .. }
    ));
    assert_eq!(target.reaction.stagger.received, 2);
    let mut rule = hit();
    rule.guard.breaks = true;
    assert_eq!(damage(&mut target, rule).guard, GuardResult::Broken);
    assert_eq!(target.reaction.stagger.received, 7);
}

#[test]
fn knockdown_timer_recovers_with_missing_or_unfinished_poses() {
    {
        let mut target = actor(Side::Enemy);
        target.reaction.stagger.duration = 2;
        let mut battle =
            PreparedBattle::new(vec![(target, Default::default())], Default::default(), 1)
                .unwrap()
                .finish()
                .unwrap();
        battle.enter_knockdown(ActorId(0), &mut vec![]);
        battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::default());
        let before = battle.actors[0].clone();
        battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(battle.actors[0], before);
        let mut getting_up = false;
        for _ in 0..20 {
            battle.step(BattleInput::default()).unwrap();
            getting_up |= battle.activity(ActorId(0)) == Activity::GettingUp;
            if battle.activity(ActorId(0)) == Activity::Idle {
                break;
            }
        }
        assert!(getting_up);
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert_eq!(
            battle.actors[0].reaction.protection.mode,
            ProtectionMode::Recovery
        );
        assert!(battle.actors[0].reaction.protection.remaining > 0);
        assert!(!battle.is_diagnostic());
        for _ in 0..120 {
            battle.step(BattleInput::default()).unwrap();
        }
        assert_eq!(battle.actors[0].reaction.protection, Protection::default());
    }
}

#[test]
fn reaching_threshold_precedes_stun_even_for_knockdown_immune_profiles() {
    for immune in [false, true] {
        let mut target = actor(Side::Enemy);
        target.reaction.stagger.threshold = 5;
        target.reaction.stagger.duration = 12;
        target.reaction.profile.can_knock_down = !immune;
        target.body.collider = Some(crate::Collider::sphere(1.));
        let owner = actor(Side::Party);

        let mut battle = PreparedBattle::new(
            vec![(owner, Default::default()), (target, Default::default())],
            Default::default(),
            1,
        )
        .unwrap()
        .finish()
        .unwrap();
        let mut rule = hit();
        rule.reaction.stun_chance = 100;
        let melee = MeleeDefinition {
            hit: rule,
            trail: None,
            volume: crate::MeleeVolume {
                offset: [0.; 3],
                radius: 2.,
                half_height: 2.,
            },
        };
        let mut contacts = crate::contact::Contacts::default();
        contacts
            .melee(ActorId(0), ActionId(0), &melee, &[])
            .unwrap();
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues).unwrap();
        let target = &battle.actors[1];
        assert!(cues.iter().any(|c| matches!(c, Cue::Hit { .. })));
        assert_eq!(
            battle.activity(ActorId(1)),
            if immune {
                Activity::Hurt
            } else {
                Activity::KnockedDown
            }
        );
        assert_eq!(target.reaction.stagger.received, if immune { 5 } else { 0 });
    }
}

#[test]
fn stagger_interrupts_actor_tasks_but_preserves_released_work() {
    use crate::{ActionRequest, Rejection};
    let owner = actor(Side::Party);
    let mut target = actor(Side::Enemy);
    target.reaction.stagger.threshold = 5;
    target.reaction.stagger.duration = 20;
    target.body.collider = Some(crate::Collider::sphere(1.));
    let definitions = crate::tests::prepared(vec![owner, target], 30);
    let mut actions = definitions
        .resources
        .actions
        .entries
        .iter()
        .map(|row| (**row).clone())
        .collect::<Vec<_>>();
    actions[0].tp_cost = 0;

    let mut prepared = PreparedBattle::new(
        (definitions.actors.clone())
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        actions.into(),
        1,
    )
    .unwrap();
    crate::tests::assign_action(&mut prepared, 1, crate::ActionKey(0));
    let mut battle = prepared.finish().unwrap();
    let resident = battle
        .release_volley(
            std::sync::Arc::new(crate::tests::volley()),
            ActorId(1),
            ActorId(1),
            crate::SpellSlot::Primary,
            None,
            &mut vec![],
        )
        .unwrap()
        .unwrap();
    let frame = battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(1),
                target: ActorId(1),
                action: crate::ActionKey(0),
            }],
            ..Default::default()
        })
        .unwrap();
    let shots = |frame: &crate::BattleFrame| {
        frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::ProjectileStarted { .. }))
            .count()
    };
    let mut emitted = shots(&frame);
    let ids: Vec<_> = frame
        .cues
        .iter()
        .filter_map(|cue| match cue {
            Cue::Started { action, .. } => Some(*action),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 1);
    let ids = [ids[0], resident];
    let mut contacts = crate::contact::Contacts::default();
    contacts
        .melee(
            ActorId(0),
            ActionId(0),
            &MeleeDefinition {
                hit: hit(),
                trail: None,
                volume: crate::MeleeVolume {
                    offset: [0.; 3],
                    radius: 2.,
                    half_height: 2.,
                },
            },
            &[],
        )
        .unwrap();
    let mut cues = vec![];
    contacts.resolve(&mut battle, &mut cues).unwrap();
    assert_eq!(
        cues.iter()
            .filter(|c| matches!(c, Cue::Interrupted { .. }))
            .count(),
        1
    );
    assert!(cues.contains(&Cue::Interrupted { action: ids[0] }));
    assert!(battle.sequence(&ids[0]).is_none());
    assert!(battle.volleys.contains_key(&ids[1]));
    assert_eq!(battle.actors[1].hp, 18);
    let frame = battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(1),
                target: ActorId(1),
                action: crate::ActionKey(0),
            }],
            ..Default::default()
        })
        .unwrap();
    emitted += shots(&frame);
    assert!(frame.cues.contains(&Cue::Rejected {
        actor: ActorId(1),
        reason: Rejection::Busy
    }));
    for _ in 0..35 {
        emitted += shots(&battle.step(BattleInput::default()).unwrap());
    }
    assert_eq!(emitted, 3, "stagger must not interrupt the released volley");
    assert!(
        battle
            .step(BattleInput {
                interrupt: vec![ids[0]],
                ..Default::default()
            })
            .is_err()
    );
}

fn contact_battle(mut target: Actor) -> Battle {
    let mut owner = actor(Side::Party);
    owner.control = Control::Manual;
    owner.position = target.position;
    target.hp = 500;
    target.equipment.max_hp = 500;
    target.control = Control::Enemy;
    target.heading = 180.;
    target.facing_direction = [0., 0., -1.];
    target.body.collider = Some(crate::Collider::sphere(1.));
    PreparedBattle::new(
        vec![(owner, Default::default()), (target, Default::default())],
        Default::default(),
        1,
    )
    .unwrap()
    .finish()
    .unwrap()
}

fn submit_contact(battle: &mut Battle, knock_down: bool, delay: u8) -> Vec<Cue> {
    submit_contact_with_impulse(battle, knock_down, delay, 16.)
}

fn submit_contact_with_impulse(
    battle: &mut Battle,
    knock_down: bool,
    delay: u8,
    impulse: f32,
) -> Vec<Cue> {
    let mut rule = hit();
    rule.arte = true;
    rule.reaction.stagger = 1;
    rule.reaction.hitstun = 45;
    rule.reaction.recoil = crate::RecoilRule {
        impulse: [impulse, 0.],
        knock_down,
        delay,
        ..Default::default()
    };
    let melee = MeleeDefinition {
        hit: rule,
        trail: None,
        volume: crate::MeleeVolume {
            offset: [0.; 3],
            radius: 2.,
            half_height: 2.,
        },
    };
    let mut contacts = crate::contact::Contacts::default();
    contacts
        .melee(ActorId(0), ActionId(0), &melee, &[])
        .unwrap();
    let mut cues = vec![];
    contacts.resolve(battle, &mut cues).unwrap();
    assert!(cues.iter().any(|cue| matches!(cue, Cue::Hit { .. })));
    cues
}

#[test]
fn contact_down_waits_then_gets_up_and_recovers() {
    let mut target = actor(Side::Enemy);
    target.reaction.stagger.duration = 2;
    let mut battle = contact_battle(target);
    submit_contact(&mut battle, true, 0);
    assert_eq!(battle.activity(ActorId(1)), Activity::Hurt);
    assert_eq!(
        battle.actors[1].reaction.recoil.kind,
        crate::RecoilKind::Down
    );

    assert_eq!(battle.actors[1].movement.forward, 16.);
    assert_eq!(battle.actors[1].reaction.stagger.received, 1);
    battle.step(BattleInput::default()).unwrap();
    // Landing starts the full authored down duration; Hurt no longer advances it.
    assert_eq!(battle.activity(ActorId(1)), Activity::KnockedDown);

    assert_eq!(battle.actors[1].reaction.stagger.received, 1);
    assert_eq!(battle.actors[1].reaction.stagger.window, 59);

    let actor_before = battle.actors[1].clone();
    battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.actors[1], actor_before);

    let mut got_up = false;
    for _ in 0..90 {
        battle.step(BattleInput::default()).unwrap();
        got_up |= battle.activity(ActorId(1)) == Activity::GettingUp;
        if battle.activity(ActorId(1)) == Activity::Idle {
            break;
        }
    }
    assert!(got_up);
    assert_eq!(battle.activity(ActorId(1)), Activity::Idle);
    assert_eq!(
        battle.actors[1].reaction.protection.mode,
        ProtectionMode::Recovery
    );
    assert_eq!(
        battle.actors[1].reaction.recoil.kind,
        crate::RecoilKind::Down
    );
}

#[test]
fn contact_down_waits_for_landing_even_with_air_recovery() {
    let mut target = actor(Side::Enemy);
    target.position[1] = 30.;
    target.reaction.recover_in_air = true;
    target.reaction.stagger.duration = 12;
    let mut battle = contact_battle(target);
    submit_contact(&mut battle, true, 0);
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.activity(ActorId(1)), Activity::Hurt);
    assert!(battle.actors[1].position[1] > 0.1);
    for _ in 0..30 {
        battle.step(BattleInput::default()).unwrap();
        if battle.activity(ActorId(1)) != Activity::Hurt {
            break;
        }
    }
    assert_eq!(battle.activity(ActorId(1)), Activity::KnockedDown);
}

#[test]
fn contact_down_retains_existing_guard_profile_and_protection_gates() {
    for (guarded, immune, clear_pending, armored) in [
        (true, false, false, false),
        (false, true, false, false),
        (false, false, true, false),
        (false, false, false, true),
    ] {
        let mut target = actor(Side::Enemy);
        target.guard.active = guarded;
        target.guard.break_pressure = 100;
        target.reaction.profile.can_knock_down = !immune;
        target.reaction.profile.clear_pending = clear_pending;
        if armored {
            target.reaction.protection.mode = ProtectionMode::Armor;
        }
        let mut battle = contact_battle(target);
        submit_contact(&mut battle, true, 0);
        assert_eq!(
            battle.actors[1].reaction.recoil.kind,
            crate::RecoilKind::Normal
        );
        assert_ne!(battle.activity(ActorId(1)), Activity::KnockedDown);
        if guarded {
            assert_eq!(battle.activity(ActorId(1)), Activity::Guarding);
        }
        if armored {
            assert_eq!(battle.activity(ActorId(1)), Activity::Idle);
        }
    }
    let mut target = actor(Side::Enemy);
    target.reaction.stagger.duration = 12;
    let mut battle = contact_battle(target);
    submit_contact(&mut battle, true, 0);
    // A new inclusive hit window clears the previous contact cache.
    submit_contact(&mut battle, false, 0);
    assert_eq!(
        battle.actors[1].reaction.recoil.kind,
        crate::RecoilKind::Normal
    );
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.activity(ActorId(1)), Activity::Hurt);
}
