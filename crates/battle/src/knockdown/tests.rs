use super::*;
use crate::{
    ActionId, Affinity, BattleInput, Control, Cue, GuardResult, HitRule, MeleeDefinition, Power,
    PreparedBattle, ReactionRule, tests::actor,
};

fn hit() -> HitRule {
    HitRule {
        power: Power::Fixed(32),
        reaction: ReactionRule {
            stagger: 5,
            hitstun: 20,
            ..Default::default()
        },
        ..crate::tests::hit()
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

fn contact(battle: &mut Battle, rule: HitRule) -> Vec<Cue> {
    let mut contacts = crate::contact::Contacts::default();
    contacts
        .melee(
            ActorId(0),
            ActionId(0),
            &MeleeDefinition {
                hit: rule,
                trail: None,
                volume: crate::MeleeVolume::Fixed {
                    offset: [0.; 3],
                    radius: 2.,
                    half_height: 2.,
                },
            },
            &[],
        )
        .unwrap();
    let mut cues = vec![];
    contacts.resolve(battle, &mut cues, &[]).unwrap();
    assert!(cues.iter().any(|cue| matches!(cue, Cue::Hit { .. })));
    cues
}

fn set_target_model(battle: &mut Battle, model: crate::ModelDefinition) {
    battle.set_models(
        crate::Models::new(
            battle.actors(),
            vec![None, Some(std::sync::Arc::new(model))],
            Default::default(),
            Default::default(),
        )
        .unwrap(),
    );
}

fn motion(battle: &Battle) -> crate::MainMotionObservation {
    battle
        .models()
        .unwrap()
        .main_motion_observation(ActorId(1))
        .unwrap()
}

fn assert_paused(battle: &mut Battle, id: ActorId) {
    let before = battle.actors[id.index()].clone();
    let motion = battle
        .models()
        .and_then(|models| models.main_motion_observation(id));
    battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.actors[id.index()], before);
    assert_eq!(
        battle
            .models()
            .and_then(|models| models.main_motion_observation(id)),
        motion
    );
}

fn assert_down_remaining(battle: &Battle, remaining: u32) {
    assert!(matches!(
        battle.runtime[1].task(),
        ActorTask::Knockdown(Recovery::Down { remaining: actual }) if *actual == remaining
    ));
}

#[test]
fn protection_preserves_side_window_and_hits_down_rules() {
    use HitProtection::{Armored, Avoided, Reduced};
    use ProtectionMode::{Down, Recovery};
    use Side::{Enemy, Party};
    for (mode, side, window, hits_down, protection, amount) in [
        (Down, Party, 0, false, Avoided, 0),
        (Down, Enemy, 0, false, Reduced, 8),
        (Down, Party, 1, false, Reduced, 4),
        (Down, Enemy, 1, false, Reduced, 4),
        (Down, Party, 1, true, Armored, 32),
        (Down, Enemy, 1, true, Armored, 32),
        (Down, Party, 0, true, Avoided, 0),
        (Recovery, Party, 1, true, Avoided, 0),
        (Recovery, Enemy, 1, true, Reduced, 4),
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
            (protection, amount, 50 - amount)
        );
        assert_eq!(
            target.reaction.armor.received,
            if protection == HitProtection::Avoided {
                0
            } else {
                target.reaction.armor.threshold
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
    for (mode, remaining, expected_remaining) in [
        (ProtectionMode::Armor, 3, 10),
        (ProtectionMode::Armor, 40, 40),
        (ProtectionMode::Armor, u32::MAX, u32::MAX),
        (ProtectionMode::Recovery, 2, 2),
        (ProtectionMode::Escape, 180, 180),
    ] {
        let mut protection = Protection { mode, remaining };
        protection.armor(10);
        assert_eq!(
            protection,
            Protection {
                mode,
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
fn knockdown_without_models_recovers_and_preserves_pause() {
    let mut target = actor(Side::Enemy);
    target.reaction.stagger.duration = 2;
    let mut battle = crate::tests::prepare_actors(vec![target], 1)
        .finish()
        .unwrap();
    battle.enter_knockdown(ActorId(0), &mut vec![]);
    battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::default());
    assert_paused(&mut battle, ActorId(0));
    for _ in 0..2 {
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.activity(ActorId(0)), Activity::KnockedDown);
    }
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
    crate::tests::step_idle(&mut battle, 120).unwrap();
    assert_eq!(battle.actors[0].reaction.protection, Protection::default());
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

        let mut battle = crate::tests::prepare_actors(vec![owner, target], 1)
            .finish()
            .unwrap();
        let mut model = crate::tests::model_definition(
            7,
            [(0, 20.), (3, 3.), (7, 2.), (8, 2.), (9, 2.), (10, 2.)],
        );
        model.reactions.hurt = [Some(3); 2];
        model.reactions.stagger_down = Some(7);
        model.reactions.stagger_get_up = Some(9);
        model.reactions.down = Some(8);
        model.reactions.get_up = Some(10);
        model.idle_motions[0] = Some(0);
        set_target_model(&mut battle, model);
        let mut rule = hit();
        rule.reaction.stun_chance = 100;
        let cues = contact(&mut battle, rule);
        let target = &battle.actors[1];
        assert_eq!(
            battle.activity(ActorId(1)),
            if immune {
                Activity::Hurt
            } else {
                Activity::KnockedDown
            }
        );
        assert_eq!(target.reaction.stagger.received, if immune { 5 } else { 0 });
        let mut frame = battle.publish(cues);
        battle
            .advance_models(&mut frame, crate::BattleClock::Running, true)
            .unwrap();
        assert_eq!(frame.models[0].clip, 3, "contact keeps the hurt pose");
        if !immune {
            let mut got_up = false;
            for _ in 0..80 {
                let mut frame = battle.step(BattleInput::default()).unwrap();
                battle
                    .advance_models(&mut frame, crate::BattleClock::Running, true)
                    .unwrap();
                match frame.actors[1].activity {
                    Activity::KnockedDown => assert_eq!(frame.models[0].clip, 7),
                    Activity::GettingUp => {
                        assert_eq!(frame.models[0].clip, 9);
                        got_up = true;
                    }
                    Activity::Idle => break,
                    activity => panic!("unexpected stagger recovery activity: {activity:?}"),
                }
            }
            assert!(got_up, "stagger uses its own get-up pose before recovery");
            assert_eq!(battle.activity(ActorId(1)), Activity::Idle);
        }
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
        .step(BattleInput::from(ActionRequest {
            actor: ActorId(1),
            target: ActorId(1),
            action: crate::ActionKey(0),
        }))
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
    let cues = contact(&mut battle, hit());
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
        .step(BattleInput::from(ActionRequest {
            actor: ActorId(1),
            target: ActorId(1),
            action: crate::ActionKey(0),
        }))
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
    crate::tests::prepare_actors(vec![owner, target], 1)
        .finish()
        .unwrap()
}

fn submit_contact(battle: &mut Battle, knock_down: bool) {
    let mut rule = hit();
    rule.arte = true;
    rule.reaction.stagger = 1;
    rule.reaction.hitstun = 45;
    rule.reaction.recoil = crate::RecoilRule {
        impulse: [16., 0.],
        knock_down,
        ..Default::default()
    };
    contact(battle, rule);
}

#[test]
fn contact_down_waits_then_gets_up_and_recovers() {
    let mut target = actor(Side::Enemy);
    target.reaction.stagger.duration = 2;
    let mut battle = contact_battle(target);
    submit_contact(&mut battle, true);
    assert_eq!(battle.activity(ActorId(1)), Activity::Hurt);
    assert_eq!(
        battle.actors[1].reaction.recoil.kind,
        crate::RecoilKind::Down
    );

    assert_eq!(battle.actors[1].movement.forward, 16.);
    assert_eq!(battle.actors[1].reaction.stagger.received, 1);
    battle.step(BattleInput::default()).unwrap();
    // The landing visit consumes the first tick of the down duration.
    assert_eq!(battle.activity(ActorId(1)), Activity::KnockedDown);

    assert_eq!(battle.actors[1].reaction.stagger.received, 1);
    assert_eq!(battle.actors[1].reaction.stagger.window, 59);

    assert_paused(&mut battle, ActorId(1));

    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.activity(ActorId(1)), Activity::KnockedDown);
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.activity(ActorId(1)), Activity::GettingUp);

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

    let target = ActorId(1);
    let mut definition = crate::tests::model_definition(7, [(0, 20.), (4, 2.), (8, 2.), (10, 2.)]);
    definition.initial = crate::Playback {
        clip: 4,
        frame: 1.,
        rate: 0.5,
        repeat: false,
    };
    definition.idle_motions = [Some(0), None];
    definition.reactions.down = Some(8);
    definition.reactions.get_up = Some(10);
    set_target_model(&mut battle, definition);
    battle.set_task(
        target.index(),
        ActorTask::Knockdown(Recovery::Down { remaining: 3 }),
    );

    battle.step(BattleInput::default()).unwrap();
    assert_down_remaining(&battle, 3);
    let hurt = motion(&battle);
    assert_eq!((hurt.clip, hurt.frame, hurt.finished), (4, 2., false));
    assert_paused(&mut battle, target);

    battle.step(BattleInput::default()).unwrap();
    assert_eq!(motion(&battle).clip, 8);
    // Replacing the completed hurt pose consumes its final ready tick.
    assert_down_remaining(&battle, 2);
    let mut finished = false;
    for _ in 0..32 {
        battle.step(BattleInput::default()).unwrap();
        finished = motion(&battle).finished;
        if finished {
            break;
        }
        assert_down_remaining(&battle, 2);
    }
    assert!(finished);
    assert_down_remaining(&battle, 1);
    battle.step(BattleInput::default()).unwrap();
    assert_down_remaining(&battle, 0);
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.activity(target), Activity::GettingUp);
    assert_eq!(motion(&battle).clip, 10);
    for _ in 0..32 {
        battle.step(BattleInput::default()).unwrap();
        if battle.activity(target) == Activity::Idle {
            break;
        }
    }
    assert_eq!(battle.activity(target), Activity::Idle);
    assert!(battle.actor_command_ready(target));
    let completed = motion(&battle);
    assert_eq!(
        (completed.clip, completed.frame, completed.finished),
        (10, 2., true)
    );
    assert_paused(&mut battle, target);
    battle.step(BattleInput::default()).unwrap();
    let idle = motion(&battle);
    assert_eq!((idle.clip, idle.frame, idle.repeat), (0, 0., true));
    for _ in 0..8 {
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(motion(&battle), idle);
    }
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(motion(&battle).frame, 0.5);
}

#[test]
fn contact_down_waits_for_landing_even_with_air_recovery() {
    let mut target = actor(Side::Enemy);
    target.position[1] = 30.;
    target.reaction.recover_in_air = true;
    target.reaction.stagger.duration = 12;
    let mut battle = contact_battle(target);
    submit_contact(&mut battle, true);
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
        submit_contact(&mut battle, true);
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
    submit_contact(&mut battle, true);
    // A new inclusive hit window clears the previous contact cache.
    submit_contact(&mut battle, false);
    assert_eq!(
        battle.actors[1].reaction.recoil.kind,
        crate::RecoilKind::Normal
    );
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.activity(ActorId(1)), Activity::Hurt);
}

#[test]
fn pratfall_counts_ground_impacts_once_and_not_airborne_knockdowns() {
    let mut target = actor(Side::Party);
    target.position[1] = 10.;
    let mut battle = contact_battle(target);
    battle.actors[1].reaction.recoil.kind = crate::RecoilKind::Launched;
    let id = ActorId(1);
    let mut cues = Vec::new();
    battle.apply_knockdown_impact(id, &mut cues);
    assert_eq!(battle.ledger.achievements[1].knockdowns, 0);
    battle.actors[1].position[1] = 0.;
    battle.apply_knockdown_impact(id, &mut cues);
    assert_eq!(battle.ledger.achievements[1].knockdowns, 1);
    battle.apply_knockdown_impact(id, &mut cues);
    assert_eq!(battle.ledger.achievements[1].knockdowns, 1);
}
