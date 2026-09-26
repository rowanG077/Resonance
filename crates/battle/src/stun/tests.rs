use super::*;
use crate::tests::prepared;
use crate::{
    ActionId, ActionPhase, ActionRequest, BattleInput, DamageKind, GuardResult, GuardRule,
    HitElement, HitRule, HitShape, MeleeDefinition, ModelDefinition, Playback, Power,
    PreparedBattle, ReactionRule, ResourceBinding, Side,
};
use resonance_content::{
    animation::{Bone, Motion, Skeleton, Transform, TransformChannels},
    battle_effect::declaration::Declaration,
};
fn actor(side: Side) -> Actor {
    let mut actor = crate::tests::actor(side);
    actor.body.points = vec![crate::HurtPoint {
        center: [0.; 3],
        radius: 1.,
    }];
    actor
}

fn definition() -> Arc<ModelDefinition> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/stun-particle-source.json"
    ))
    .unwrap();
    let declaration: Declaration = serde_json::from_value(fixture["declaration"].clone()).unwrap();
    let uv = serde_json::from_value::<Vec<_>>(fixture["uv"].clone()).unwrap();
    let motion = Motion {
        duration_frames: 4.,
        tracks: vec![],
    };
    Arc::new(ModelDefinition {
        secondary_motion: vec![],
        resource: 7,
        skeleton: Skeleton {
            bones: vec![
                Bone {
                    name: "body".into(),
                    parent: None,
                    bind_channels: TransformChannels(8),
                    bind: Transform::default(),
                },
                Bone {
                    name: "head".into(),
                    parent: Some(0),
                    bind_channels: TransformChannels(8),
                    bind: Transform {
                        translation: [0., 0., 8.],
                        ..Default::default()
                    },
                },
            ],
        },
        motions: [3, 7, 9, 21]
            .into_iter()
            .map(|id| (id, motion.clone()))
            .collect(),
        initial: Playback {
            clip: 3,
            frame: 0.,
            rate: 0.5,
            repeat: false,
        },
        hurt_motions: [Some(3), Some(3)],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        knockdown: None,
        stun: Some(StunBinding {
            particle: Arc::new(ParticleDefinition {
                model: None,
                resource: 8,
                member: 19,
                data: declaration.particle(&uv).unwrap(),
            }),
            head: 1,
            offset: [2., 3., 4.],
            loop_motion: 21,
            down_motion: 7,
            recovery_motion: 9,
            sound: SoundBinding {
                resource: 9,
                index: 117,
            },
        }),
        anchors: vec![crate::Anchor {
            bone: 0,
            offset: [0.; 3],
        }],
        approach_bones: vec![],
        target_bones: vec![],
        shadow: None,
        target_marker: None,
        weapons: vec![],
        hurt_bones: vec![0],
        suppress_root_translation: [false; 3],
    })
}

fn battle(mut target: Actor) -> Battle {
    target.side = Side::Enemy;
    let p = Arc::try_unwrap(prepared("pub task run() { await battle::at_age(ticks(100)); battle::heal_percent(battle::owner(), 20); }", vec![actor(Side::Party), target], 120)).unwrap();
    let mut actions = p.actions;
    actions[0].phase = ActionPhase::Actor;
    Battle::new(Arc::new(
        PreparedBattle::new(
            p.actors,
            actions,
            1,
            vec![Some(definition()), Some(definition())],
            p.effects.into_values().collect(),
        )
        .unwrap(),
    ))
}

fn hit() -> HitResult {
    HitResult {
        amount: 8,
        hp_change: -8,
        critical: false,
        affinity: crate::Affinity::Normal,
        guard: GuardResult::None,
        auto_guard: false,
        armored: false,
        protection: crate::HitProtection::None,
    }
}

fn melee(chance: u8) -> MeleeDefinition {
    MeleeDefinition {
        hit: HitRule {
            impact: None,
            arte: false,
            kind: DamageKind::Slash,
            power: Power::Fixed(8),
            element: HitElement::Neutral,
            prevents_defeat: false,
            guard: GuardRule::default(),
            reaction: ReactionRule {
                stun_chance: chance,
                hitstun: 20,
                ..Default::default()
            },
        },
        cooldown: 0,
        radius: 2.,
        height: 2.,
        shape: HitShape::Sphere,
        anchors: vec![0],
        trail: None,
    }
}

fn contact(battle: &mut Battle, chance: u8) -> Vec<Cue> {
    battle.melee[0].clear();
    let mut contacts = crate::contact::Contacts::default();
    contacts
        .melee(
            ActorId(0),
            ActionId(999),
            &battle.actors[0],
            None,
            &melee(chance),
        )
        .unwrap();
    let mut cues = vec![];
    contacts.resolve(battle, &mut cues).unwrap();
    cues
}

#[test]
fn chance_clamps_before_ex_bonus_and_rolls_before_immunity_even_at_zero() {
    let mut owner = actor(Side::Party);
    let mut target = actor(Side::Enemy);
    for chance in [0, 100] {
        target.reaction.stun.immune = true;
        let mut random = crate::state::Random(1);
        assert!(!roll(&owner, &target, chance, hit(), &mut random));
        let mut expected = crate::state::Random(1);
        expected.next();
        assert_eq!(random.0, expected.0);
    }
    target.reaction.stun.immune = false;
    target.reaction.stun.resistance = 255;
    owner.reaction.stun.ex_bonus = true;
    // Select a real generator output under 5: the EX bonus is added after clamping.
    let seed = (0..1000)
        .find(|&seed| crate::state::Random(seed).next() % 100 < 5)
        .unwrap();
    assert!(roll(
        &owner,
        &target,
        0,
        hit(),
        &mut crate::state::Random(seed)
    ));
    owner.reaction.stun.chance_bonus = 100;
    target.reaction.stun.resistance = 0;
    assert!(roll(
        &owner,
        &target,
        0,
        hit(),
        &mut crate::state::Random(1)
    ));
    for case in 0..5 {
        let mut target = target.clone();
        let mut result = hit();
        match case {
            0 => target.hp = 0,
            1 => target.reaction.unflinching = true,
            2 => result.armored = true,
            3 => result.affinity = crate::Affinity::Absorb,
            _ => {
                result.guard = GuardResult::Blocked {
                    first: true,
                    special: false,
                }
            }
        }
        let mut random = crate::state::Random(1);
        assert!(!roll(&owner, &target, 100, result, &mut random));
        assert_eq!(random.0, 1);
    }
    let mut broken = hit();
    broken.guard = GuardResult::Broken;
    assert!(roll(
        &owner,
        &target,
        100,
        broken,
        &mut crate::state::Random(1)
    ));
}

#[test]
fn real_contact_cancels_actor_work_and_keeps_a_retained_star_until_recovery() {
    let mut battle = battle(actor(Side::Enemy));
    let start = battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(1),
                target: ActorId(1),
                action: 99,
            }],
            ..Default::default()
        })
        .unwrap();
    let action = start.actions[0].0;
    let cues = contact(&mut battle, 100);
    assert!(cues.contains(&Cue::Interrupted { action }));
    assert_eq!(battle.actors[1].activity, Activity::Stunned);
    assert_eq!(battle.actors[1].reaction.remaining, 180);
    assert_eq!(battle.actors[1].reaction.armor.threshold, 0);
    assert!(battle.particle_frames().is_empty()); // Born after the particle pass.
    let star = battle.actors[1].reaction.stun.particle.unwrap();
    assert!(battle.particle(star.0, action).is_err()); // Actor ownership cannot be borrowed by a task.
    let mut sounds = vec![];
    for visit in 1..=180 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.actors[1].hp, 42); // Cancelled recovery task never resumes.
        if frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Sound { .. }))
        {
            sounds.push(visit);
        }
        if visit == 1 {
            assert!(frame.cues.contains(&Cue::ParticleStarted {
                particle: star,
                action: None
            }));
            assert_eq!(frame.particles[0].origin, [0., 8., 0.]);
            assert_eq!(frame.particles[0].state.offset, [2., 3., 4.]);
            assert_eq!(frame.particles[0].state.geometry_count, 4);
        }
        if visit < 180 {
            let expected = if visit <= 90 {
                4
            } else if visit <= 135 {
                2
            } else {
                1
            };
            assert_eq!(frame.particles[0].state.geometry_count, expected);
        }
        if visit == 180 {
            assert_eq!(frame.actors[1].activity, Activity::Idle);
            assert!(frame.particles.is_empty());
            assert!(
                frame
                    .cues
                    .contains(&Cue::ParticleExpired { particle: star })
            );
        }
    }
    assert_eq!(sounds, [20, 60, 100, 140]);
    assert!(battle.actors[1].reaction.stun.particle.is_none());
}

#[test]
fn struggle_pulse_spans_airborne_visits_and_hit_stop_but_menu_holds_everything() {
    let mut target = actor(Side::Enemy);
    target.control = Control::SemiAuto;
    target.reaction.stun.shortened = true;
    let mut battle = battle(target);
    battle.enter_stun(ActorId(1)).unwrap();
    assert_eq!(battle.actors[1].reaction.remaining, 90);
    battle.actors[1].hit_stop = 5;
    battle.actors[1].movement.forward = 2.;
    battle.actors[1].movement.braking = 0.5;
    battle.actors[1].reaction.direction = [1., 0., 0.];
    let input = || BattleInput {
        stun_struggle: vec![ActorId(1)],
        ..Default::default()
    };
    let first = battle.step(input()).unwrap();
    assert_eq!(first.actors[1].reaction.remaining, 88);
    assert_eq!(first.actors[1].position[0], 2.);
    assert_eq!(first.particles[0].state.angular_velocity[2], -7.5);
    let paused = battle
        .step(BattleInput {
            menu_open: true,
            ..input()
        })
        .unwrap();
    assert_eq!(first.actors, paused.actors);
    assert_eq!(first.particles, paused.particles);
    battle.actors[1].position[1] = 5.;
    let airborne = battle.step(input()).unwrap();
    assert_eq!(airborne.actors[1].reaction.remaining, 87); // Last pulse, no grounded decrement.
    let airborne = battle.step(input()).unwrap();
    assert_eq!(airborne.actors[1].reaction.remaining, 87);
    assert_eq!(airborne.particles[0].state.angular_velocity[2], -2.5);
    assert_eq!(airborne.actors[1].hit_stop, 2);
    let before = battle.actors.clone();
    assert!(
        battle
            .step(BattleInput {
                stun_struggle: vec![ActorId(12)],
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(battle.actors, before);
}

#[test]
fn new_hurt_and_death_remove_stars_without_removing_released_particles() {
    let mut battle = battle(actor(Side::Enemy));
    battle.enter_stun(ActorId(1)).unwrap();
    let old = battle.actors[1].reaction.stun.particle.unwrap();
    let definition = definition().stun.as_ref().unwrap().particle.clone();
    let independent = battle
        .spawn_particle(
            definition,
            Some(ActionId(7)),
            ActorId(1),
            ActorId(1),
            [0.; 3],
            0.,
        )
        .unwrap()
        .unwrap();
    let cues = contact(&mut battle, 0);
    assert_eq!(battle.actors[1].activity, Activity::Hurt);
    assert!(cues.contains(&Cue::ParticleExpired { particle: old }));
    assert!(battle.particles.contains_key(&independent));
    battle.enter_stun(ActorId(1)).unwrap();
    let old = battle.actors[1].reaction.stun.particle.unwrap();
    battle.actors[1].hp = 1;
    let cues = contact(&mut battle, 100);
    assert_eq!(battle.actors[1].hp, 0);
    assert!(cues.contains(&Cue::ParticleExpired { particle: old }));
    assert!(battle.particles.contains_key(&independent));
}

#[test]
fn expiry_waits_for_the_recovery_clip_and_only_zero_removes_the_star() {
    let mut battle = battle(actor(Side::Enemy));
    battle.enter_stun(ActorId(1)).unwrap();
    battle.models[1]
        .as_mut()
        .unwrap()
        .play(
            crate::MotionBinding { model: 7, clip: 7 },
            0.,
            0.5,
            false,
            0,
        )
        .unwrap();
    battle.actors[1].reaction.remaining = 1;
    let frame = battle.step(BattleInput::default()).unwrap();
    assert!(frame.particles.is_empty());
    assert_eq!(frame.actors[1].activity, Activity::Stunned);
    assert!(!battle.models[1].as_ref().unwrap().finished());
    let mut visits = 0;
    while battle.actors[1].activity == Activity::Stunned {
        battle.step(BattleInput::default()).unwrap();
        visits += 1;
        assert!(visits < 20);
    }
    assert_eq!(visits, 13); // Four blend visits, then nine half-frame visits.
}

#[test]
fn preparation_rejects_incomplete_stun_bindings_before_activation() {
    let p = Arc::try_unwrap(prepared(
        "pub task run() {}",
        vec![actor(Side::Party), actor(Side::Enemy)],
        0,
    ))
    .unwrap();
    let mut actions = p.actions;
    actions[0].resources = vec![ResourceBinding::Melee(Arc::new(melee(20)))];
    assert!(PreparedBattle::new(p.actors.clone(), actions.clone(), 1, vec![], vec![]).is_err());
    for missing in 0..3 {
        let mut model = (*definition()).clone();
        match missing {
            0 => model.stun.as_mut().unwrap().head = 2,
            1 => model.stun.as_mut().unwrap().loop_motion = 999,
            _ => model.stun.as_mut().unwrap().offset[0] = f32::NAN,
        }
        assert!(
            PreparedBattle::new(
                p.actors.clone(),
                actions.clone(),
                1,
                vec![Some(Arc::new(model)), Some(definition())],
                vec![]
            )
            .is_err()
        );
    }
    assert!(
        PreparedBattle::new(
            p.actors,
            actions,
            1,
            vec![Some(definition()), Some(definition())],
            vec![]
        )
        .is_ok()
    );
}

#[test]
fn contact_rolls_match_original_rng_and_effective_chance() {
    for source in [
        include_str!("../../tests/fixtures/opening-stun-rolls.json"),
        include_str!("../../tests/fixtures/opening-stun-entry.json"),
    ] {
        let fixture: serde_json::Value = serde_json::from_str(source).unwrap();
        for row in fixture["observations"].as_array().unwrap() {
            let mut owner = actor(Side::Party);
            let mut target = actor(Side::Enemy);
            owner.reaction.stun.chance_bonus = row["bonus"].as_u64().unwrap() as u8;
            owner.reaction.stun.ex_bonus = row["ex_bonus"].as_bool().unwrap();
            target.reaction.stun.resistance = row["resistance"].as_u64().unwrap() as u8;
            target.reaction.stun.immune = row["immune"].as_bool().unwrap();
            let seed = row["random_before"].as_u64().unwrap() as u32;
            let mut random = crate::state::Random(seed);
            let selected = roll(
                &owner,
                &target,
                row["chance"].as_u64().unwrap() as u8,
                hit(),
                &mut random,
            );
            assert_eq!(selected, row["after"]["activity"] == 15);
            if selected {
                target.reaction.stun.shortened = row["shortened"].as_bool().unwrap();
                target.reaction.armor.threshold = row["before"]["armor"].as_u64().unwrap() as u8;
                let mut battle = battle(target);
                battle.enter_stun(ActorId(1)).unwrap();
                let actor = &battle.actors[1];
                assert_eq!(actor.activity, Activity::Stunned);
                assert_eq!(
                    i64::from(actor.reaction.remaining),
                    row["after"]["remaining"].as_i64().unwrap()
                );
                assert_eq!(
                    u64::from(actor.reaction.armor.threshold),
                    row["after"]["armor"].as_u64().unwrap()
                );
                assert_eq!(
                    u64::from(actor.reaction.stun.pulse),
                    row["after"]["pulse"].as_u64().unwrap()
                );
                assert_eq!(
                    battle.particles.len() as u64,
                    row["after"]["retained_count"].as_u64().unwrap()
                );
            }
            assert_eq!(u64::from(random.0), row["random_after"].as_u64().unwrap());
            assert_eq!(
                u64::from(crate::state::Random(seed).next() % 100),
                row["roll"].as_u64().unwrap()
            );
        }
    }
}

#[test]
fn only_human_controls_accept_struggle_and_attachment_follows_the_sampled_head() {
    for control in [
        Control::Manual,
        Control::SemiAuto,
        Control::Auto,
        Control::Enemy,
    ] {
        let mut target = actor(Side::Enemy);
        target.control = control;
        target.body.scale = 2.;
        let mut battle = battle(target);
        battle.enter_stun(ActorId(1)).unwrap();
        battle.actors[1].position = [20., 0., 40.];
        battle.actors[1].heading = 90.;
        let frame = battle
            .step(BattleInput {
                stun_struggle: vec![ActorId(1)],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            frame.actors[1].reaction.remaining,
            if matches!(control, Control::Manual | Control::SemiAuto) {
                178
            } else {
                179
            }
        );
        assert_eq!(frame.particles[0].origin, [20., 16., 40.]);
        for (actual, expected) in frame.particles[0]
            .state
            .offset
            .into_iter()
            .zip([8., 6., -4.])
        {
            assert!((actual - expected).abs() < 0.00001);
        }
    }
}

#[test]
fn particle_pool_exhaustion_does_not_cancel_stun_or_consume_random() {
    let mut battle = battle(actor(Side::Enemy));
    let data = definition().stun.as_ref().unwrap().particle.clone();
    for _ in 0..414 {
        assert!(
            battle
                .spawn_particle(
                    data.clone(),
                    Some(ActionId(7)),
                    ActorId(0),
                    ActorId(0),
                    [0.; 3],
                    0.
                )
                .unwrap()
                .is_some()
        );
    }
    let random = battle.random_state();
    battle.enter_stun(ActorId(1)).unwrap();
    assert_eq!(battle.actors[1].activity, Activity::Stunned);
    assert!(battle.actors[1].reaction.stun.particle.is_none());
    assert_eq!(battle.random_state(), random);
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.actors[1].reaction.remaining, 179);
}
