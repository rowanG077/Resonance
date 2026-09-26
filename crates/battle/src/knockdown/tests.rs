use super::*;
use crate::{
    ActionId, Affinity, BattleInput, Control, Cue, DamageKind, GuardResult, GuardRule, HitElement,
    HitRule, HitShape, MeleeDefinition, ModelDefinition, Playback, Power, PreparedBattle,
    ReactionRule, state::Random, tests::actor,
};
use resonance_content::animation::{Bone, Motion, Skeleton, Transform, TransformChannels};
use std::{collections::BTreeMap, sync::Arc};

fn hit() -> HitRule {
    HitRule {
        impact: None,
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
        &mut Random(1),
    )
}

#[test]
fn protection_preserves_side_window_and_hits_down_rules() {
    // Original 629F4..62A9C: protection suppresses reactions independently of
    // the damage shift. The contact still advances the armor byte.
    for (mode, side, window, hits_down, protection, amount, hp) in [
        (
            ProtectionMode::Down,
            Side::Party,
            0,
            false,
            HitProtection::Avoided,
            32,
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
            HitProtection::None,
            32,
            18,
        ),
        (
            ProtectionMode::Down,
            Side::Enemy,
            1,
            true,
            HitProtection::None,
            32,
            18,
        ),
        (
            ProtectionMode::Down,
            Side::Party,
            0,
            true,
            HitProtection::Avoided,
            32,
            50,
        ),
        (
            ProtectionMode::Recovery,
            Side::Party,
            1,
            true,
            HitProtection::Avoided,
            32,
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
        assert_eq!(target.reaction.armor.received, 3);
        assert_eq!(target.reaction.stagger.received, 0);
    }
    let mut target = actor(Side::Party);
    target.reaction.protection.mode = ProtectionMode::Recovery;
    target.affinities[0] = Affinity::Absorb;
    assert_eq!(damage(&mut target, hit()).hp_change, 32); // Absorption precedes avoidance.
    target.affinities[0] = Affinity::Immune;
    assert_eq!(damage(&mut target, hit()).hp_change, 0);
    target.side = Side::Enemy;
    target.hp = 1;
    target.affinities[0] = Affinity::Normal;
    let result = damage(&mut target, hit());
    assert_eq!(target.hp, 0);
    assert_eq!(result.protection, HitProtection::None); // Lethal flags replace protection.
}

#[test]
fn buildup_wraps_and_only_guard_break_allows_guarded_buildup() {
    let mut target = actor(Side::Enemy);
    target.reaction.stagger.received = 253;
    damage(&mut target, hit());
    assert_eq!(target.reaction.stagger.received, 2);
    for affinity in [Affinity::Absorb, Affinity::Immune] {
        target.affinities[0] = affinity;
        damage(&mut target, hit());
        assert_eq!(target.reaction.stagger.received, 2);
    }
    target.affinities[0] = Affinity::Normal;
    target.hp = 100;
    target.reaction.unflinching = true;
    damage(&mut target, hit());
    assert_eq!(target.reaction.stagger.received, 2);
    target.reaction.unflinching = false;
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

fn model(recovery_motion: Option<u16>) -> Arc<ModelDefinition> {
    Arc::new(ModelDefinition {
        secondary_motion: vec![],
        resource: 7,
        skeleton: Skeleton {
            bones: vec![Bone {
                name: "root".into(),
                parent: None,
                bind_channels: TransformChannels(8),
                bind: Transform::default(),
            }],
        },
        motions: [3, 7, 9]
            .into_iter()
            .map(|clip| {
                (
                    clip,
                    Motion {
                        duration_frames: 1.,
                        tracks: vec![],
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
        initial: Playback {
            clip: 3,
            frame: 0.,
            rate: 0.5,
            repeat: false,
        },
        hurt_motions: [Some(3), Some(3)],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        knockdown: Some(KnockdownBinding {
            down_motion: 7,
            recovery_motion,
        }),
        stun: None,
        anchors: vec![],
        approach_bones: vec![],
        target_bones: vec![],
        shadow: None,
        target_marker: None,
        weapons: vec![],
        hurt_bones: vec![],
        suppress_root_translation: [false; 3],
    })
}

fn battle(target: Actor, recovery: Option<u16>) -> Battle {
    Battle::new(Arc::new(
        PreparedBattle::new(vec![target], vec![], 1, vec![Some(model(recovery))], vec![]).unwrap(),
    ))
}

#[test]
fn down_waits_for_motion_then_getup_and_common_protection_run_independently() {
    let mut target = actor(Side::Enemy);
    target.control = Control::Enemy;
    target.reaction.stagger.duration = 2;
    target.reaction.combo_hits = 4;
    target.reaction.stagger.received = 5;
    enter(&mut target);
    let mut battle = battle(target, Some(9));
    let mut activities = vec![];
    for tick in 1..=30 {
        let frame = battle.step(BattleInput::default()).unwrap();
        let target = &frame.actors[0];
        activities.push(target.activity);
        if tick == 1 {
            // Common timers follow the callback, while presentation retains
            // the model sampled before the callback requested clip 7.
            assert_eq!(target.reaction.stagger.window, 57);
            assert_eq!(target.reaction.stagger.received, 0);
            assert_eq!(frame.models[0].clip, 3);
            assert_eq!(target.reaction.remaining, 2);
        }
        if tick == 29 {
            assert_eq!(target.activity, Activity::Idle);
            assert_eq!(target.reaction.remaining, 0);
            assert_eq!(
                target.reaction.protection,
                Protection {
                    mode: ProtectionMode::Recovery,
                    remaining: 119,
                }
            );
        }
    }
    assert!(activities[..17].iter().all(|a| *a == Activity::KnockedDown));
    assert!(activities[17..28].iter().all(|a| *a == Activity::GettingUp));
    let before = battle.actors[0].clone();
    battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.actors[0], before);
    for _ in 0..118 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.actors[0].reaction.protection, Protection::default());
}

#[test]
fn missing_getup_recovers_immediately_and_airborne_down_keeps_its_clock() {
    let mut target = actor(Side::Party);
    target.activity = Activity::KnockedDown;
    target.hit_stop = 4;
    target.reaction.protection.mode = ProtectionMode::Down;
    target.reaction.combo_hits = 120;
    let mut battle = battle(target, None);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.actors[0].activity, Activity::Idle);
    assert_eq!(frame.actors[0].reaction.remaining, 30);
    assert_eq!(frame.actors[0].reaction.stagger.window, 0); // Signed clamp 1, then common decrement.
    assert_eq!(frame.actors[0].hit_stop, 3);
    assert_eq!(frame.actors[0].reaction.protection.remaining, 119);

    let target = &mut battle.actors[0];
    target.activity = Activity::KnockedDown;
    target.position[1] = 8.;
    target.movement.vertical = -2.;
    target.reaction.remaining = 20;
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.actors[0].position[1], 6.);
    assert_eq!(frame.actors[0].reaction.remaining, 20);
    assert!(!frame.actors[0].reaction.stagger.initialized);
}

#[test]
fn reaching_threshold_precedes_stun_even_for_knockdown_immune_profiles() {
    for immune in [false, true] {
        let mut target = actor(Side::Enemy);
        target.reaction.stagger.threshold = 5;
        target.reaction.stagger.duration = 12;
        target.reaction.profile.can_knock_down = !immune;
        target.body.points.push(crate::HurtPoint {
            center: [0.; 3],
            radius: 1.,
        });
        let mut owner = actor(Side::Party);
        owner.body.anchors.push([0.; 3]);
        let mut target_model = model(Some(9));
        Arc::get_mut(&mut target_model).unwrap().hurt_bones = vec![0];
        let mut battle = Battle::new(Arc::new(
            PreparedBattle::new(
                vec![owner, target],
                vec![],
                1,
                vec![None, Some(target_model)],
                vec![],
            )
            .unwrap(),
        ));
        let mut rule = hit();
        rule.reaction.stun_chance = 100;
        let melee = MeleeDefinition {
            hit: rule,
            cooldown: 0,
            radius: 2.,
            height: 2.,
            shape: HitShape::Sphere,
            anchors: vec![0],
            trail: None,
        };
        let mut contacts = crate::contact::Contacts::default();
        contacts
            .melee(ActorId(0), ActionId(0), &battle.actors[0], None, &melee)
            .unwrap();
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues).unwrap();
        let target = &battle.actors[1];
        assert!(cues.iter().any(|c| matches!(c, Cue::Hit { .. })));
        assert_eq!(
            target.activity,
            if immune {
                Activity::Hurt
            } else {
                Activity::KnockedDown
            }
        );
        assert_eq!(target.reaction.stagger.received, if immune { 5 } else { 0 });
        let mut random = Random(1);
        for _ in 0..3 {
            random.next();
        }
        assert_eq!(battle.random_state(), random.0); // No fourth (stun) draw.
    }
}

#[test]
fn initial_down_requires_resources_even_for_an_immune_profile() {
    let mut target = actor(Side::Enemy);
    target.activity = Activity::KnockedDown;
    target.reaction.profile.can_knock_down = false;
    assert!(PreparedBattle::new(vec![target], vec![], 1, vec![], vec![]).is_err());
}

#[test]
fn stagger_interrupts_actor_tasks_but_preserves_released_work() {
    use crate::{ActionPhase, ActionRequest, Rejection};
    let mut owner = actor(Side::Party);
    owner.body.anchors.push([0.; 3]);
    let mut target = actor(Side::Enemy);
    target.reaction.stagger.threshold = 5;
    target.reaction.stagger.duration = 20;
    target.body.points.push(crate::HurtPoint {
        center: [0.; 3],
        radius: 1.,
    });
    let definitions = crate::tests::prepared(
        "pub task run() { await battle::at_age(ticks(5)); battle::heal_percent(battle::owner(), 10); }",
        vec![owner, target],
        30,
    );
    let mut actions = definitions.actions.clone();
    actions[0].phase = ActionPhase::Actor;
    actions[0].tp_cost = 0;
    let mut resident = actions[0].clone();
    resident.id = 100;
    resident.phase = ActionPhase::Resident;
    actions.push(resident);
    let mut target_model = model(Some(9));
    Arc::get_mut(&mut target_model).unwrap().hurt_bones = vec![0];
    let mut battle = Battle::new(Arc::new(
        PreparedBattle::new(
            definitions.actors.clone(),
            actions,
            1,
            vec![None, Some(target_model)],
            definitions.effects.values().cloned().collect(),
        )
        .unwrap(),
    ));
    let frame = battle
        .step(BattleInput {
            actions: [99, 100]
                .into_iter()
                .map(|action| ActionRequest {
                    actor: ActorId(1),
                    target: ActorId(1),
                    action,
                })
                .collect(),
            ..Default::default()
        })
        .unwrap();
    let ids: Vec<_> = frame
        .cues
        .iter()
        .filter_map(|cue| match cue {
            Cue::Started { action, .. } => Some(*action),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 2);
    let mut contacts = crate::contact::Contacts::default();
    contacts
        .melee(
            ActorId(0),
            ActionId(0),
            &battle.actors[0],
            None,
            &MeleeDefinition {
                hit: hit(),
                cooldown: 0,
                radius: 2.,
                height: 2.,
                shape: HitShape::Sphere,
                anchors: vec![0],
                trail: None,
            },
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
    assert!(!battle.sequences.contains_key(&ids[0]));
    assert!(battle.sequences.contains_key(&ids[1]));
    assert_eq!(battle.actors[1].hp, 18);
    let frame = battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(1),
                target: ActorId(1),
                action: 99,
            }],
            ..Default::default()
        })
        .unwrap();
    assert!(frame.cues.contains(&Cue::Rejected {
        actor: ActorId(1),
        reason: Rejection::Busy
    }));
    for _ in 0..5 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.actors[1].hp, 28); // Only the surviving resident healed.
    assert!(
        battle
            .step(BattleInput {
                interrupt: vec![ids[0]],
                ..Default::default()
            })
            .is_err()
    );
}

#[test]
fn staggering_resources_require_complete_recovery_models_before_activation() {
    let p = crate::tests::prepared(
        "pub task run() { battle::finish(); }",
        vec![actor(Side::Party)],
        1,
    );
    let mut actions = p.actions.clone();
    actions[0]
        .resources
        .push(crate::ResourceBinding::Melee(Arc::new(MeleeDefinition {
            hit: hit(),
            cooldown: 0,
            radius: 1.,
            height: 1.,
            shape: HitShape::Sphere,
            anchors: vec![0],
            trail: None,
        })));
    let prepare = |model| {
        PreparedBattle::new(
            p.actors.clone(),
            actions.clone(),
            1,
            vec![model],
            p.effects.values().cloned().collect(),
        )
    };
    assert!(
        prepare(None)
            .unwrap_err()
            .to_string()
            .contains("knockdown resources")
    );
    let mut incomplete = model(Some(9));
    Arc::get_mut(&mut incomplete).unwrap().motions.remove(&9);
    assert!(
        prepare(Some(incomplete))
            .unwrap_err()
            .to_string()
            .contains("knockdown motion")
    );
    assert!(prepare(Some(model(None))).is_ok()); // Original missing-get-up path is valid.
}
