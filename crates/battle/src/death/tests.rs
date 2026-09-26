use super::*;
use crate::{ActionDefinition, BattleInput, ModelDefinition, Playback, ResourceBinding};
use resonance_content::animation::{Bone, Motion, Skeleton, Transform, TransformChannels};
use std::{collections::BTreeMap, sync::Arc};

fn model(resource: u32) -> Arc<ModelDefinition> {
    Arc::new(ModelDefinition {
        resource,
        skeleton: Skeleton {
            bones: vec![Bone {
                name: "root".into(),
                parent: None,
                bind_channels: TransformChannels(0),
                bind: Transform::default(),
            }],
        },
        motions: [0, 3, 7, 9]
            .map(|clip| {
                (
                    clip,
                    Motion {
                        duration_frames: 3.,
                        tracks: vec![],
                    },
                )
            })
            .into(),
        secondary_motion: vec![],
        initial: Playback {
            clip: 0,
            frame: 0.,
            rate: 0.,
            repeat: true,
        },
        hurt_motions: [None; 2],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        stun: None,
        knockdown: None,
        anchors: vec![],
        weapons: vec![],
        hurt_bones: vec![],
        approach_bones: vec![],
        target_bones: vec![],
        shadow: None,
        target_marker: None,
        suppress_root_translation: [false; 3],
    })
}

fn prepared(actors: Vec<Actor>, waits: &[bool], rest: bool) -> PreparedBattle {
    let source = include_str!("../../../../scripts/battle/actor_death.sym");
    let compiled = symphonia_script_compiler::compile(
        "death",
        &BTreeMap::from([("death".into(), source.into())]),
        &crate::native_declarations(),
    )
    .unwrap();
    let program = Arc::new(compiled.program);
    let resources = [Some(3), rest.then_some(7)].map(|clip| {
        ResourceBinding::OptionalMotion(
            (0..actors.len())
                .map(|i| {
                    clip.map(|clip| crate::MotionBinding {
                        model: i as u32,
                        clip,
                    })
                })
                .collect(),
        )
    });
    let actions = ["fall", "initial"]
        .into_iter()
        .enumerate()
        .map(|(i, name)| ActionDefinition {
            id: i as u16,
            phase: ActionPhase::Controller,
            entry: program
                .authored()
                .unwrap()
                .functions
                .iter()
                .find(|f| f.name == format!("death::{name}"))
                .unwrap()
                .entry,
            program: program.clone(),
            duration: 0,
            tp_cost: 0,
            resources: resources.to_vec(),
        })
        .collect();
    let models = (0..actors.len()).map(|i| Some(model(i as u32))).collect();
    let definitions = waits
        .iter()
        .enumerate()
        .map(|(i, &wait_for_motion)| {
            Some(DeathBinding {
                fall: 0,
                initial: 1,
                wait_for_motion,
                integrate: rest,
                darken_immediately: !rest,
                revival_motion: Some(crate::MotionBinding {
                    model: i as u32,
                    clip: 9,
                }),
            })
        })
        .collect();
    PreparedBattle::new(actors, actions, 77, models, vec![])
        .unwrap()
        .with_deaths(definitions)
        .unwrap()
}

#[test]
fn availability_is_not_hp_or_animation_completion() {
    let mut actor = crate::tests::actor(Side::Enemy);
    actor.hp = 0;
    assert!(actor.available());
    actor.availability = ActorAvailability::Dead;
    actor.hp = actor.max_hp;
    assert!(!actor.available());
    actor.availability = ActorAvailability::Petrified;
    assert!(!actor.available());
    actor.availability = ActorAvailability::Absent;
    assert!(!actor.available());
}

#[test]
fn initial_ko_uses_dead_pose_without_fall_rng_or_entry_reset() {
    let mut dead = crate::tests::actor(Side::Party);
    dead.hp = 0;
    dead.reaction.combo_hits = 3;
    let mut battle = Battle::new(Arc::new(prepared(
        vec![dead, crate::tests::actor(Side::Enemy)],
        &[false, false],
        true,
    )));
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.models[0].clip, 0); // model sampling preceded the callback
    assert_eq!(frame.actors[0].availability, ActorAvailability::Dead);
    assert_eq!(frame.actors[0].reaction.combo_hits, 3);
    assert_eq!(battle.random_state(), 77);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.models[0].clip, 7);
    assert_eq!(frame.models[0].blend_weight, 1. / 13.);
    // 2E3F4 uses twelve blend visits for either the profile's rest clip or
    // fallback 7. Repeated grounded requests must not restart that blend.
    for age in 2..=12 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.models[0].blend_weight, age as f32 / 13.);
        assert_eq!(frame.models[0].frame, 0.);
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.models[0].blend_weight, 1.);
    assert_eq!(frame.models[0].frame, 0.5);
    assert_eq!(battle.random_state(), 77);
}

#[test]
fn contact_death_enters_immediately_and_controller_survives_blends_and_zero_hp() {
    let mut battle = Battle::new(Arc::new(prepared(
        vec![
            crate::tests::actor(Side::Party),
            crate::tests::actor(Side::Enemy),
        ],
        &[false, false],
        true,
    )));
    battle.actors[0].hp = 0;
    battle.actors[0].reaction.combo_hits = 4;
    battle.actors[0].hit_stop = 9;
    let mut cues = vec![];
    battle.enter_death(ActorId(0), &mut cues).unwrap();
    let mut random = crate::state::Random(77);
    random.next();
    assert_eq!(battle.random_state(), random.0);
    assert_eq!(battle.actors[0].availability, ActorAvailability::Dead);
    assert_eq!(battle.actors[0].reaction.combo_hits, 0);
    assert_eq!(battle.actors[0].activity, Activity::Defeated);
    assert_eq!(battle.sequences.len(), 1);
    let first = battle.step(BattleInput::default()).unwrap();
    assert_eq!(first.models[0].clip, 3);
    assert_eq!(first.models[0].blend_weight, 1. / 9.); // lethal fall remains eight
    let next = battle.step(BattleInput::default()).unwrap();
    assert_eq!(next.models[0].clip, 7);
    assert_eq!(next.models[0].blend_weight, 1. / 13.);
    assert_eq!(battle.sequences.len(), 1);
    assert!(
        next.cues
            .iter()
            .all(|c| !matches!(c, Cue::Interrupted { .. }))
    );
}

#[test]
fn only_last_victims_descriptor_can_delay_victory() {
    let actors = vec![
        crate::tests::actor(Side::Party),
        crate::tests::actor(Side::Enemy),
        crate::tests::actor(Side::Enemy),
    ];
    let mut battle = Battle::new(Arc::new(prepared(actors, &[false, true, false], false)));
    let mut cues = vec![];
    battle.actors[1].hp = 0;
    battle.enter_death(ActorId(1), &mut cues).unwrap();
    assert!(battle.victory_death_waiting());
    assert!(!battle.victory_death_ready().unwrap());
    battle.actors[2].hp = 0;
    battle.enter_death(ActorId(2), &mut cues).unwrap();
    assert!(!battle.victory_death_waiting());
    assert!(battle.victory_death_ready().unwrap());
    assert!(!battle.models[1].as_ref().unwrap().finished());
}

#[test]
fn final_victim_wait_is_consumed_once_and_dead_alpha_fades_by_twelve() {
    let mut battle = Battle::new(Arc::new(prepared(
        vec![
            crate::tests::actor(Side::Party),
            crate::tests::actor(Side::Enemy),
        ],
        &[false, true],
        false,
    )));
    battle.actors[1].hp = 0;
    battle.enter_death(ActorId(1), &mut vec![]).unwrap();
    assert_eq!(battle.actors[1].body.tint, [0, 0, 0, 248]);
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.actors[1].body.tint[3], 236);
    assert!(!battle.victory_death_ready().unwrap());
    for _ in 0..20 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert!(battle.victory_death_ready().unwrap());
    assert!(!battle.victory_death_waiting());
    assert_eq!(battle.actors[1].body.tint[3], 0);
}

#[test]
fn revival_opens_availability_before_get_up_and_cancels_dead_controller() {
    let mut dead = crate::tests::actor(Side::Party);
    dead.hp = 0;
    let mut battle = Battle::new(Arc::new(prepared(
        vec![
            dead,
            crate::tests::actor(Side::Party),
            crate::tests::actor(Side::Enemy),
        ],
        &[false; 3],
        true,
    )));
    battle.step(BattleInput::default()).unwrap();
    battle.actors[0].hp = 1; // a vitals edit alone cannot reactivate an actor
    battle.actors[0].recovery.lucky = true;
    battle.actors[0].luck = 255;
    let before_random = battle.random_state();
    assert!(!battle.actors[0].available());
    let mut cues = vec![];
    battle.revive_percent(ActorId(0), 30, &mut cues).unwrap();
    assert!(battle.actors[0].available());
    assert_eq!(battle.actors[0].hp, 31);
    assert_eq!(battle.random_state(), before_random);
    assert_eq!(battle.actors[0].activity, Activity::GettingUp);
    assert_eq!(battle.actors[0].reaction.remaining, 10);
    assert_eq!(battle.actors[0].reaction.protection.remaining, 120);
    assert!(cues.iter().any(|c| matches!(c, Cue::Interrupted { .. })));
    assert!(!battle.models[0].as_ref().unwrap().finished());
    for _ in 0..20 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.actors[0].activity, Activity::Idle);
}

#[test]
fn preparation_rejects_wrong_controller_phase_or_foreign_revival_motion() {
    let make = || prepared(vec![crate::tests::actor(Side::Party)], &[false], true);
    let mut candidate = make();
    let binding = candidate.deaths[0].unwrap();
    candidate.actions[0].phase = ActionPhase::Actor;
    assert!(candidate.with_deaths(vec![Some(binding)]).is_err());
    let mut foreign = binding;
    foreign.revival_motion.as_mut().unwrap().model = 99;
    assert!(make().with_deaths(vec![Some(foreign)]).is_err());
    assert!(make().with_deaths(vec![]).is_err());
}

#[test]
fn ally_death_uses_one_shared_draw_and_gains_gauge_before_voice_arbitration() {
    for (seed, overlimit_active) in [1, 77, 81]
        .into_iter()
        .flat_map(|seed| [(seed, false), (seed, true)])
    {
        let mut actors = vec![crate::tests::actor(Side::Party); 4];
        actors[0].overlimit = 700;
        actors[1].overlimit = 950;
        actors[2].availability = ActorAvailability::Dead;
        actors[2].overlimit = 400;
        actors[3].overlimit = if overlimit_active { 600 } else { 0 };
        actors[3].overlimit_active = overlimit_active;
        let voices = [10, 11].map(|index| {
            Some(crate::VoiceLine {
                sound: crate::SoundBinding { resource: 9, index },
                duration: 123,
            })
        });
        let reaction = AllyDeathReaction {
            voices,
            priority: 3,
            overlimit_gain: 150,
        };
        let mut feedback = DeathFeedback {
            enemy: None,
            allies: vec![vec![None; 4]; 4],
        };
        feedback.allies[0][1..].fill(Some(reaction));
        let mut candidate = prepared(actors, &[false; 4], true)
            .with_death_feedback(feedback)
            .unwrap();
        candidate.random_seed = seed;
        let mut battle = Battle::new(Arc::new(candidate));
        battle.voices[3].blocked = !overlimit_active;
        let mut random = crate::state::Random(seed);
        let variant = usize::from(random.next() & 1);
        battle.actors[0].hp = 0;
        battle.enter_death(ActorId(0), &mut vec![]).unwrap();
        assert_eq!(battle.random_state(), random.0);
        assert_eq!(
            battle
                .actors
                .iter()
                .map(|a| a.overlimit)
                .collect::<Vec<_>>(),
            [0, 1000, 400, if overlimit_active { 600 } else { 150 }]
        );
        let selected = Some((voices[variant].unwrap().sound, 3));
        assert_eq!(battle.voices[1].pending, selected);
        assert!(battle.voices[2].pending.is_none());
        assert_eq!(
            battle.voices[3].pending,
            if overlimit_active { selected } else { None }
        );
        battle.enter_death(ActorId(0), &mut vec![]).unwrap();
        assert_eq!(battle.random_state(), random.0); // Repeated contact cannot reroll.
    }
}

#[test]
fn initial_ko_and_missing_or_rejected_voices_do_not_change_death_rng_rules() {
    let mut actors = vec![crate::tests::actor(Side::Party); 3];
    actors[0].hp = 0;
    let sound = crate::SoundBinding {
        resource: 9,
        index: 23,
    };
    let reaction = AllyDeathReaction {
        voices: [Some(crate::VoiceLine { sound, duration: 7 }); 2],
        priority: 1,
        overlimit_gain: 110,
    };
    let mut feedback = DeathFeedback {
        enemy: None,
        allies: vec![vec![None; 3]; 3],
    };
    feedback.allies[0][1] = Some(reaction);
    feedback.allies[1][2] = Some(reaction);
    feedback.allies[1][1] = Some(AllyDeathReaction {
        voices: [None; 2],
        ..reaction
    });
    let candidate = prepared(actors, &[false; 3], true)
        .with_death_feedback(feedback)
        .unwrap()
        .with_voices_enabled(false);
    let mut battle = Battle::new(Arc::new(candidate));
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.random_state(), 77);
    assert_eq!(battle.actors[1].overlimit, 0);
    battle.voices[2].priority = 3;
    battle.actors[1].hp = 0;
    battle.enter_death(ActorId(1), &mut vec![]).unwrap();
    let mut random = crate::state::Random(77);
    random.next();
    assert_eq!(battle.random_state(), random.0);
    assert_eq!(battle.actors[2].overlimit, 110);
    assert!(battle.voices[2].pending.is_none());
    assert_eq!(battle.actors[1].overlimit, 0); // Reaction preceded victim state/gauge clear.
}

#[test]
fn death_feedback_requires_prepared_members_and_party_only_recipients() {
    let make = || {
        prepared(
            vec![
                crate::tests::actor(Side::Party),
                crate::tests::actor(Side::Enemy),
            ],
            &[false; 2],
            true,
        )
    };
    let mut feedback = DeathFeedback {
        enemy: Some(DeathEffect {
            appearance: crate::EffectAppearance {
                resource: 9,
                member: 14,
            },
            sound: crate::SoundBinding {
                resource: 9,
                index: 73,
            },
        }),
        allies: vec![vec![None; 2]; 2],
    };
    assert!(make().with_death_feedback(feedback.clone()).is_err());
    feedback.enemy = None;
    feedback.allies[0][1] = Some(AllyDeathReaction {
        voices: [None; 2],
        priority: 1,
        overlimit_gain: 110,
    });
    assert!(make().with_death_feedback(feedback.clone()).is_err());
    feedback.allies.pop();
    assert!(make().with_death_feedback(feedback).is_err());
}

#[test]
fn result_overlimit_reset_distinguishes_active_state_from_full_gauge() {
    let mut actors = vec![crate::tests::actor(Side::Party); 2];
    actors[0].overlimit = 1000;
    actors[1].overlimit = 400;
    actors[1].overlimit_active = true;
    let mut battle = Battle::new(Arc::new(prepared(actors, &[false; 2], true)));
    battle.end_overlimit(ActorId(0)).unwrap();
    battle.end_overlimit(ActorId(1)).unwrap();
    assert_eq!(battle.actors[0].overlimit, 1000);
    assert_eq!(battle.actors[1].overlimit, 0);
    assert!(!battle.actors[1].overlimit_active);
}
