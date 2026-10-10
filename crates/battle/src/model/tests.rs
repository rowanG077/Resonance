use super::*;
use crate::{PreparedBattle, Side};
use resonance_content::animation::{
    Bone, Track, Transform, TransformChannels, VectorCurve, VectorInterpolation,
};

fn definition() -> Arc<ModelDefinition> {
    let skeleton = Skeleton {
        bones: vec![
            Bone {
                name: "root".into(),
                parent: None,
                bind_channels: TransformChannels(8),
                bind: Transform::default(),
            },
            Bone {
                name: "hand".into(),
                parent: Some(0),
                bind_channels: TransformChannels(8),
                bind: Transform {
                    translation: [2., 0., 3.],
                    ..Default::default()
                },
            },
        ],
    };
    let motion = Motion {
        duration_frames: 10.,
        tracks: vec![Track {
            bone: 1,
            bind_channels: TransformChannels(8),
            period_frames: 10.,
            times: vec![0., 10.],
            translation: Some(VectorCurve {
                interpolation: VectorInterpolation::Linear,
                values: vec![[2., 0., 3.], [12., 0., 3.]],
                incoming: vec![],
                outgoing: vec![],
                ease: vec![],
            }),
            scale: None,
            rotation: None,
            euler_degrees: None,
            matrices: None,
        }],
    };
    Arc::new(ModelDefinition {
        tint: [64, 64, 64, 255],
        fade_on_defeat: false,
        secondary_motion: vec![],
        reactions: Default::default(),
        idle_motions: [None; 2],
        idle_expression: [0; 4],
        blink: None,
        resource: 7,
        skeleton,
        motions: BTreeMap::from([(0, motion.clone()), (1, motion)]),
        initial: Playback {
            clip: 0,
            frame: 0.,
            rate: 0.,
            repeat: true,
        },

        shadow: None,
        weapons: vec![],

        suppress_root_translation: [false; 3],
    })
}

#[test]
fn model_playback_loops_freezes_or_finishes_at_its_directional_endpoint() -> Result<()> {
    for rate in [-0.5, 0., 0.5, 4.] {
        for repeat in [false, true] {
            let mut definition = (*definition()).clone();
            let motion = definition.motions.get_mut(&0).unwrap();
            motion.duration_frames = 30.;
            motion.tracks.clear();
            definition.initial.frame = 15.;
            definition.initial.rate = rate;
            definition.initial.repeat = repeat;
            let actor = actor();
            let mut model = Model::new(Arc::new(definition), ActorId(0), &actor)?;
            let mut previous = model.shown.frame;
            let mut wrapped = false;
            for _ in 0..100 {
                model.step(&actor, true)?;
                let frame = model.shown.frame;
                assert!((0. ..=30.).contains(&frame));
                wrapped |= (frame - previous) * rate < 0.;
                previous = frame;
            }
            assert_eq!(wrapped, repeat && rate != 0.);
            if rate == 0. {
                assert_eq!(model.shown.frame, 15.);
            } else if !repeat {
                assert_eq!(model.shown.frame, if rate > 0. { 30. } else { 0. });
            }
        }
    }
    Ok(())
}

#[test]
fn entry_pose_and_attachments_begin_at_final_placement_and_pause_without_drift() -> Result<()> {
    use resonance_content::secondary_motion::{Chain, Joint};
    let mut definition = (*definition()).clone();
    definition.initial.rate = 0.5;
    definition.secondary_motion.push(Chain {
        joints: (0..2)
            .map(|node| Joint {
                node,
                gravity: 1.,
                damping: 0.7,
            })
            .collect(),
        attraction: 0.03,
        preserve_rotation: false,
        rotation_locks: [false; 2],
        collision_plane: None,
    });
    definition.weapons.push(Arc::new(WeaponDefinition {
        slot: 0,
        attachment: 1,
        layers: vec![crate::WeaponLayerDefinition {
            resource: 8,
            skeleton: definition.skeleton.clone(),
            motions: definition.motions.clone(),
            playback: WeaponPlayback::Owner {
                offset: 0,
                fallback: 0,
                initial: Playback {
                    clip: 0,
                    frame: 2.,
                    rate: 0.5,
                    repeat: true,
                },
            },
            secondary_motion: vec![],
        }],
        links: vec![],
    }));
    let mut actor = actor();
    actor.position = [-300., 50., 0.];
    actor.heading = 90.;
    actor.body.scale = 2.;
    let mut enemy = crate::tests::actor(Side::Enemy);
    enemy.position = [200., 0., 100.];
    let prepared = PreparedBattle::new(
        vec![(actor, Default::default()), (enemy, Default::default())],
        Default::default(),
        1,
    )?
    .with_entry_choices(
        (0..2)
            .map(|index| crate::EntryChoice {
                actor: ActorId(index),
                strategy: crate::TargetPolicy::Nearest,
            })
            .collect(),
    )?;
    let actor = &prepared.actors[0];
    let mut model = Model::new(Arc::new(definition), ActorId(0), actor)?;
    let initial = model.shown.clone();
    let weapon = model.weapon_frames()[0].clone();
    assert_eq!(initial.world, world(actor));
    assert!(
        model.secondary[0]
            .positions()
            .iter()
            .all(|position| position.is_finite())
    );
    model.step(actor, false)?;
    assert_eq!(model.shown, initial);
    assert_eq!(model.weapon_frames()[0], weapon);
    for _ in 0..3 {
        model.step(actor, true)?;
        assert_eq!(model.shown.world, world(actor));
        assert!(
            model
                .shown
                .bones
                .iter()
                .flatten()
                .flatten()
                .all(|value| value.is_finite())
        );
        assert!(
            model.secondary[0]
                .positions()
                .iter()
                .all(|position| position.distance(glam::Vec3::from_array(actor.position)) < 50.)
        );
    }
    Ok(())
}

#[test]
fn secondary_motion_keeps_history_and_leaves_terminal_guides_at_the_sampled_pose() {
    use resonance_content::animation::transform_point;
    let position = |model: &Model, bone: usize| {
        transform_point(
            model.shown.world,
            transform_point(model.shown.bones[bone], [0.; 3]),
        )
    };
    use resonance_content::secondary_motion::{Chain, Joint};
    let mut definition = (*definition()).clone();
    definition.skeleton.bones.push(Bone {
        name: "guide".into(),
        parent: Some(1),
        bind_channels: TransformChannels(8),
        bind: Transform {
            translation: [8., 0., 0.],
            ..Default::default()
        },
    });
    definition.secondary_motion = vec![Chain {
        joints: (0..3)
            .map(|node| Joint {
                node,
                gravity: 1.,
                damping: 0.7,
            })
            .collect(),
        attraction: 0.03,
        preserve_rotation: false,
        rotation_locks: [false; 2],
        collision_plane: None,
    }];
    let mut subject = actor();
    subject.position = [0., 50., 0.];
    let mut animated = Model::new(Arc::new(definition.clone()), ActorId(0), &subject).unwrap();
    let before = animated.shown.clone();
    for _ in 0..8 {
        animated.step(&subject, true).unwrap();
    }
    assert_ne!(before.bones[1], animated.shown.bones[1]);
    let mut fresh = definition.clone();
    fresh.initial.frame = animated.shown.frame;
    let restarted = Model::new(Arc::new(fresh), ActorId(0), &subject).unwrap();
    assert_ne!(animated.shown.bones[1], restarted.shown.bones[1]);
    let mut ordinary = definition;
    ordinary.secondary_motion.clear();
    ordinary.initial.frame = animated.shown.frame;
    let sampled = Model::new(Arc::new(ordinary), ActorId(0), &subject).unwrap();
    assert_eq!(animated.shown.bones[2], sampled.shown.bones[2]);
    assert_ne!(position(&animated, 1), position(&sampled, 1));
    assert_eq!(position(&animated, 2), position(&sampled, 2));

    let mut resumed = animated.clone();
    let positions = animated.secondary[0].positions().to_vec();
    let velocity = animated.secondary[0].velocity().to_vec();
    let anchors = [
        position(&animated, 0),
        position(&animated, 1),
        position(&animated, 2),
    ];
    subject.position[0] += 10.;
    for _ in 0..60 {
        animated.step(&subject, false).unwrap();
        assert_eq!(animated.secondary[0].positions(), positions);
        assert_eq!(animated.secondary[0].velocity(), velocity);
        // Driven joints retain world positions, while the undriven terminal
        // guide follows the recomposed actor transform.
        assert!((position(&animated, 1)[0] - anchors[1][0]).abs() < 0.00001);
        assert_eq!(position(&animated, 2)[0], anchors[2][0] + 10.);
    }
    animated.step(&subject, true).unwrap();
    resumed.step(&subject, true).unwrap();
    assert_eq!(animated.shown, resumed.shown);

    let mut invalid = (*animated.definition).clone();
    invalid.secondary_motion[0].joints[1].node = 255;
    assert!(Model::new(Arc::new(invalid), ActorId(0), &subject).is_err());
}

fn actor() -> Actor {
    let mut actor = crate::tests::actor(Side::Party);
    actor.body.collider = Some(crate::Collider::sphere(1.));
    actor
}

#[test]
fn affine_root_policy_preserves_translation_observation_and_shear() -> Result<()> {
    let mut definition = (*definition()).clone();
    let motion = definition.motions.get_mut(&0).unwrap();
    let track = &mut motion.tracks[0];
    track.bone = 0;
    track.translation = None;
    track.matrices = Some(vec![
        [1., 0.25, 0., 10., 0., 1., 0., 20., 0., 0., 1., 30.];
        track.times.len()
    ]);
    definition.suppress_root_translation = [false, true, false];
    let subject = actor();
    let model = Model::new(Arc::new(definition), ActorId(0), &subject)?;
    assert_eq!(model.shown.root_translation, [10., 20., 30.]);
    assert_eq!(model.shown.bones[0][3], [10., 0., 30., 1.]);
    assert_eq!(model.shown.bones[0][1], [0.25, 1., 0., 0.]);
    Ok(())
}

#[test]
fn hurt_restarts_the_selected_clip_and_preserves_the_contact_frames_drawing_pose() {
    let mut definition = Arc::try_unwrap(definition()).unwrap();
    definition.reactions.hurt = [Some(1), Some(0)];
    let actor = actor();
    let mut model = Model::new(Arc::new(definition), ActorId(0), &actor).unwrap();
    let held = model.shown.clone();
    model.hurt(false).unwrap();
    assert_eq!(model.shown, held);
    for _ in 0..5 {
        model.step(&actor, true).unwrap();
    }
    assert_eq!(model.shown.clip, 1);
    assert!(model.shown.frame > 0.);
    model.hurt(false).unwrap(); // Repeated contacts restart the same clip.
    model.step(&actor, true).unwrap();
    assert_eq!(model.shown.frame, 0.5);
    model.hurt(true).unwrap();
    model.step(&actor, true).unwrap();
    assert_eq!(model.shown.clip, 0);
}

#[test]
fn absent_reaction_slots_preserve_playback_and_declared_missing_clips_fail() {
    let mut definition = Arc::try_unwrap(definition()).unwrap();
    definition.initial.rate = 0.5;
    definition.reactions.hurt = [None, Some(1)];
    definition.reactions.guard = [Some(1), None];
    let actor = actor();
    let mut model = Model::new(Arc::new(definition.clone()), ActorId(0), &actor).unwrap();
    model.hurt(false).unwrap();
    let mut airborne = actor.clone();
    airborne.position[1] = 50.;
    model.settle(&airborne, crate::Activity::Guarding).unwrap();
    model.step(&actor, true).unwrap();
    assert_eq!((model.shown.clip, model.shown.frame), (0, 1.));
    model.hurt(true).unwrap();
    model.step(&actor, true).unwrap();
    assert_eq!((model.shown.clip, model.shown.frame), (1, 0.5));
    definition.reactions.guard = [Some(99), None];
    assert!(Model::new(Arc::new(definition), ActorId(0), &actor).is_err());
}

#[test]
fn reaction_poses_follow_native_state_and_keep_active_clocks() -> Result<()> {
    use crate::{Activity, RecoilKind};
    let mut definition = (*definition()).clone();
    for clip in 41..=47 {
        definition
            .motions
            .insert(clip, definition.motions[&1].clone());
    }
    definition.reactions = crate::ReactionMotions {
        guard: [Some(41), Some(42)],
        stunned: Some(43),
        jump: Some(44),
        backstep: Some(45),
        taunt: Some(46),
        returning: Some(44),
        stopping: Some(47),
        chant: Some(43),
        cast: Some(42),
        rising: Some(44),
        falling: Some(45),
        down: Some(46),
        get_up: Some(47),
        ..Default::default()
    };
    let mut actor = actor();
    let definition = Arc::new(definition);
    let mut model = Model::new(Arc::clone(&definition), ActorId(0), &actor)?;
    model.settle(&actor, Activity::Guarding)?;
    model.step(&actor, true)?;
    let frame = model.shown.clone();
    model.settle(&actor, Activity::Guarding)?;
    model.step(&actor, false)?;
    assert_eq!(model.shown, frame, "held guard keeps its playback");
    assert_eq!(frame.clip, 41);

    actor.position[1] = 50.;
    model.settle(&actor, Activity::Guarding)?;
    assert_eq!(model.clip, 42);
    actor.reaction.recoil.kind = RecoilKind::Launched;
    actor.movement.vertical = 3.;
    model.settle(&actor, Activity::Hurt)?;
    assert_eq!(model.clip, 44);
    actor.movement.vertical = -3.;
    model.settle(&actor, Activity::Hurt)?;
    assert_eq!(model.clip, 45);
    model.settle(&actor, Activity::KnockedDown)?;
    assert_eq!(model.clip, 45, "airborne knockdown waits for landing");

    actor.position[1] = 0.;
    for kind in [
        RecoilKind::Normal,
        RecoilKind::Down,
        RecoilKind::SettledLaunch,
    ] {
        actor.reaction.recoil.kind = kind;
        model.settle(&actor, Activity::KnockedDown)?;
        assert_eq!(model.clip, 46);
        model.settle(&actor, Activity::GettingUp)?;
        assert_eq!(model.clip, 47);
    }
    actor.time_stop = 1;
    model.settle(&actor, Activity::Stunned)?;
    assert_eq!(model.clip, 47);
    actor.time_stop = 0;
    model.settle(&actor, Activity::Stunned)?;
    let mut looped = false;
    for _ in 0..100 {
        let previous = model.shown.frame;
        model.step(&actor, true)?;
        looped |= model.shown.frame < previous;
    }
    assert_eq!(model.shown.clip, 43);
    assert!(looped, "stun playback continues through its loop");
    actor.reaction.recoil.kind = RecoilKind::Normal;
    let mut models = crate::Models::new(
        std::slice::from_ref(&actor),
        vec![Some(definition)],
        Default::default(),
        Default::default(),
    )?;
    let base =
        crate::PreparedBattle::new(vec![(actor, Default::default())], Default::default(), 1)?
            .finish()?
            .snapshot();
    for (pose, clip) in [
        (crate::CommonPose::Jump, 44),
        (crate::CommonPose::Backstep, 45),
        (crate::CommonPose::Taunt, 46),
        (crate::CommonPose::Returning, 44),
        (crate::CommonPose::Stopping, 47),
        (crate::CommonPose::Chant, 43),
        (crate::CommonPose::Cast, 42),
    ] {
        let mut frame = base.clone();
        frame.model_requests.push(crate::ModelRequest::Common {
            actor: ActorId(0),
            pose,
        });
        models.advance(&mut frame, crate::BattleClock::Running, false)?;
        assert_eq!(
            models.main_motion_observation(ActorId(0)).unwrap().clip,
            clip
        );
    }
    Ok(())
}

#[test]
fn sparse_clips_retain_omitted_channels_until_replaced() {
    let mut definition = (*definition()).clone();
    definition.motions.get_mut(&0).unwrap().tracks[0]
        .translation
        .as_mut()
        .unwrap()
        .values = vec![[12., 0., 3.]; 2];
    let mut sparse = definition.motions[&1].clone();
    sparse.tracks[0].translation = None;
    sparse.tracks[0].rotation = Some(resonance_content::animation::QuaternionCurve {
        interpolation: resonance_content::animation::QuaternionInterpolation::ShortestSlerp,
        values: vec![[0., 0., 0., 1.], [0., 0., 1., 0.]],
        incoming: vec![],
        outgoing: vec![],
        ease: vec![],
    });
    definition.motions.insert(2, sparse);
    definition.motions.insert(
        3,
        Motion {
            duration_frames: 10.,
            tracks: vec![],
        },
    );
    let definition = Arc::new(definition);
    let blend = 4u8;
    let subject = actor();
    let mut model = Model::new(Arc::clone(&definition), ActorId(0), &subject).unwrap();
    let translation = model.shown.bones[1][3];
    let rotation = model.shown.bones[1][0];
    model
        .play(MotionBinding { model: 7, clip: 2 }, 0., 0.5, false, blend)
        .unwrap();
    for _ in 0..u16::from(blend) + 6 {
        model.step(&subject, true).unwrap();
        crate::tests::assert_vector_close::<3>(
            model.shown.bones[1][3][..3].try_into().unwrap(),
            translation[..3].try_into().unwrap(),
            0.00001,
        );
    }
    assert_ne!(model.shown.bones[1][0], rotation);
    // Omitting the whole track retains its pose too.
    let retained = model.shown.bones.clone();
    model
        .play(MotionBinding { model: 7, clip: 3 }, 0., 0.5, false, blend)
        .unwrap();
    for _ in 0..u16::from(blend) + 2 {
        model.step(&subject, true).unwrap();
        assert_eq!(model.shown.bones, retained);
    }
    // A later authored translation replaces the retained channel normally.
    model
        .play(MotionBinding { model: 7, clip: 1 }, 0., 0.5, false, 0)
        .unwrap();
    model.step(&subject, true).unwrap();
    assert_eq!(model.shown.bones[1][3][0], 2.5);
}

#[test]
fn replacement_during_blend_starts_from_visible_pose_and_reaches_each_endpoint() {
    let actor = actor();
    let mut model = Model::new(definition(), ActorId(0), &actor).unwrap();
    let binding = MotionBinding { model: 7, clip: 1 };
    model.play(binding, 8., 0.5, false, 4).unwrap();
    model.step(&actor, true).unwrap();
    for frame in [4., 1.] {
        let visible = model.shown.bones[1][3][0];
        model.play(binding, frame, 0., false, 4).unwrap();
        model.step(&actor, false).unwrap();
        assert!((model.shown.bones[1][3][0] - visible).abs() < 0.00001);
        let endpoint = 2. + frame;
        let mut previous = visible;
        for _ in 0..5 {
            model.step(&actor, true).unwrap();
            let current = model.shown.bones[1][3][0];
            assert!((current - endpoint).abs() <= (previous - endpoint).abs() + 0.00001);
            previous = current;
        }
        assert!((previous - endpoint).abs() < 0.00001);
    }
}

#[test]
fn invalid_playback_is_rejected_before_replacing_a_visible_pose() -> Result<()> {
    let actor = actor();
    let mut model = Model::new(definition(), ActorId(0), &actor)?;
    for (frame, rate, duration) in [
        (f32::NAN, 0.5, 10.),
        (0., 1e-14, 10.),
        (0., f32::INFINITY, 10.),
        (0., 5., 10.),
        (0., -5., 10.),
        (0., 0.5, 4000.),
    ] {
        let mut invalid = (*definition()).clone();
        invalid.initial.frame = frame;
        invalid.initial.rate = rate;
        invalid.motions.get_mut(&0).unwrap().duration_frames = duration;
        assert!(Model::new(Arc::new(invalid), ActorId(0), &actor).is_err());
        if duration == 10. {
            let before = model.shown.clone();
            assert!(
                model
                    .play(MotionBinding { model: 7, clip: 1 }, frame, rate, false, 4)
                    .is_err()
            );
            assert_eq!(model.shown, before);
        }
    }
    Ok(())
}

#[test]
fn resting_pose_uses_health_and_keeps_actions_and_active_idle_clocks() -> Result<()> {
    use crate::Activity;
    let mut definition = (*definition()).clone();
    for clip in [41, 42] {
        definition
            .motions
            .insert(clip, definition.motions[&0].clone());
    }
    definition.initial.clip = 41;
    definition.idle_motions = [Some(41), Some(42)];
    definition.idle_expression = [1, 2, 3, 4];
    let mut subject = actor();
    subject.hp = 24;
    let mut model = Model::new(Arc::new(definition), ActorId(0), &subject)?;
    assert_eq!(model.shown.texture_layers, [1, 2, 3, 4]);
    model.shown.texture_layers = [9; 4];
    model.settle(&subject, Activity::Idle)?;
    assert_eq!(model.shown.texture_layers, [1, 2, 3, 4]);
    model.step(&subject, true)?;
    assert_eq!(model.shown.clip, 42);
    let shown = model.shown.clone();
    model.settle(&subject, Activity::Idle)?;
    model.step(&subject, false)?;
    assert_eq!(
        model.shown, shown,
        "rest selection does not restart playback"
    );
    subject.hp = 25;
    model.settle(&subject, Activity::Idle)?;
    model.step(&subject, false)?;
    assert_eq!(model.shown.clip, 41);
    model.play(MotionBinding { model: 7, clip: 1 }, 0., 1., false, 0)?;
    model.settle(&subject, Activity::Action)?;
    model.step(&subject, false)?;
    assert_eq!(model.shown.clip, 1);
    model.settle(&subject, Activity::Recovering)?;
    assert_eq!(model.clip, 1, "unfinished recovery keeps its pose");
    for _ in 0..40 {
        model.step(&subject, true)?;
    }
    model.settle(&subject, Activity::Recovering)?;
    model.step(&subject, false)?;
    assert_eq!(model.shown.clip, 41);
    model.play(MotionBinding { model: 7, clip: 1 }, 0., 0.5, true, 0)?;
    subject.movement.forward = 5.;
    model.settle(&subject, Activity::Idle)?;
    model.step(&subject, false)?;
    assert_eq!(
        model.shown.clip, 1,
        "AI repositioning keeps its movement pose"
    );
    subject.movement.forward = 0.;
    model.settle(&subject, Activity::Recovering)?;
    assert_eq!(model.clip, 1, "looping recovery keeps its pose");
    model.settle(&subject, Activity::Idle)?;
    assert_eq!(model.clip, 41, "native completion permits rest");
    subject.side = Side::Enemy;
    subject.hp = 1;
    model.settle(&subject, Activity::Idle)?;
    assert_eq!(model.clip, 41);
    let end = model.definition.motions[&41].duration_frames;
    model.play(MotionBinding { model: 7, clip: 41 }, end, 0.5, false, 0)?;
    model.settle(&subject, Activity::Idle)?;
    model.step(&subject, true)?;
    assert!(
        model.shown.frame < end,
        "an already bound rest clip must loop"
    );
    Ok(())
}

#[test]
fn blink_overlay_preserves_expressions_and_pauses_with_the_model() -> Result<()> {
    use crate::ActorAvailability;
    use resonance_content::battle_profile::Blink;

    let baseline = [7, 3, 9, 11];
    let mut body = (*definition()).clone();
    body.blink = Some(Blink {
        channel: 1,
        frames: [4, 5],
        excluded_expressions: vec![8],
    });
    let mut actor = actor();
    let mut model = Model::new(Arc::new(body), ActorId(0), &actor)?;
    model.shown.texture_layers = baseline;
    for _ in 0..BLINK_PERIOD_TICKS {
        model.step(&actor, true)?;
        let frame = model.render_frame(&actor);
        if frame.texture_layers != baseline {
            break;
        }
    }
    assert_eq!(model.render_frame(&actor).texture_layers, [7, 4, 9, 11]);
    model.step(&actor, false)?;
    assert_eq!(model.render_frame(&actor).texture_layers, [7, 4, 9, 11]);

    model.shown.texture_layers[1] = 8;
    assert_eq!(model.render_frame(&actor).texture_layers, [7, 8, 9, 11]);
    model.shown.texture_layers[1] = 6;
    actor.availability = ActorAvailability::Petrified;
    model.step(&actor, false)?;
    assert_eq!(model.render_frame(&actor).texture_layers, [7, 6, 9, 11]);
    actor.availability = ActorAvailability::Active;
    assert_eq!(model.render_frame(&actor).texture_layers, [7, 4, 9, 11]);
    for expression in [4, 5, 5, 4, 4, 6] {
        model.step(&actor, true)?;
        assert_eq!(
            model.render_frame(&actor).texture_layers,
            [7, expression, 9, 11]
        );
    }
    assert_eq!(model.shown.texture_layers, [7, 6, 9, 11]);
    Ok(())
}

#[test]
fn shadows_follow_native_bodies_independently_of_animation() -> Result<()> {
    let mut body = (*definition()).clone();
    body.shadow = Some(ShadowDefinition {
        scale: 6.,
        color: [16, 16, 16, 176],
    });
    let mut actor = actor();
    actor.body.collider = Some(crate::Collider::standing(30., 150.));
    let mut model = Model::new(Arc::new(body), ActorId(0), &actor)?;
    let initial = model.shown.shadow.unwrap();
    actor.position = [100., 400., 0.];
    model.sample_shadow(&actor);
    let shadow = model.shown.shadow.unwrap();
    assert_eq!(shadow.position[0], actor.position[0]);
    assert!(shadow.radius < initial.radius);
    Ok(())
}
