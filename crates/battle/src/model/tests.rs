use super::*;
use crate::WeaponSlot;
use crate::{
    PreparedBattle, ResourceId, Side,
    tests::{models, prepare_actors},
};
use resonance_content::animation::{
    Bone, Track, Transform, TransformChannels, VectorCurve, VectorInterpolation,
};

fn definition() -> Arc<ModelDefinition> {
    let mut definition = crate::tests::model_definition(7, [(0, 10.), (1, 10.)]);
    definition.initial.rate = 0.;
    definition.skeleton.bones.push(Bone {
        name: "hand".into(),
        parent: Some(0),
        bind_channels: TransformChannels(8),
        bind: Transform {
            translation: [2., 0., 3.],
            ..Default::default()
        },
    });
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
    definition.motions = BTreeMap::from([(0, motion.clone()), (1, motion)]);
    Arc::new(definition)
}

#[test]
fn partial_pose_keeps_advancing_during_body_blend_and_blends_out_from_visible_pose() -> Result<()> {
    let mut definition = (*definition()).clone();
    let motion = definition.motions.get_mut(&1).unwrap();
    let mut root = motion.tracks[0].clone();
    root.bone = 0;
    root.translation.as_mut().unwrap().values = vec![[0.; 3], [20., 0., 0.]];
    motion.tracks.push(root);
    let actor = actor();
    let mut model = Model::new(Arc::new(definition), ActorId(0), &actor)?;
    let binding = |clip| MotionBinding {
        model: ResourceId(7),
        clip,
    };
    let outgoing = model.composed_local()[1].value();
    let mut blending = model.clone();
    blending.play_part(
        binding(0),
        Pose {
            blend: 4,
            rate: 2.,
            ..Default::default()
        },
        vec![1],
    )?;
    blending.step(&actor, true)?;
    assert_eq!(blending.parts[0].animation.clock.frame, 0.);
    blending.step(&actor, true)?;
    assert_eq!(blending.parts[0].animation.clock.frame, 2.);
    assert!(blending.parts[0].animation.clock.blending());
    model.play_part(
        binding(0),
        Pose {
            frame: 2.,
            blend: 0,
            ..Default::default()
        },
        vec![1],
    )?;
    model.step(&actor, true)?;
    assert_eq!(model.composed_local()[1].value(), outgoing);
    let parts = std::mem::take(&mut model.parts);
    model.play(binding(1), 6., 0.5, false, 4)?;
    model.parts = parts;
    model.step(&actor, true)?;
    model.step(&actor, true)?;
    assert_eq!(model.animation.clock.frame, 6.);
    assert_eq!(model.parts[0].animation.clock.frame, 3.);
    assert_eq!(model.composed_local()[0].translation()[0], 2.4);
    assert_eq!(model.composed_local()[1].translation()[0], 5.);
    let visible = model.shown.clone();
    model.step(&actor, false)?;
    assert_eq!(model.shown, visible);
    model.play(binding(0), 0., 0.5, true, 4)?;
    assert!(model.parts.is_empty());
    model.step(&actor, true)?;
    assert_eq!(model.shown.bones, visible.bones);
    model.step(&actor, true)?;
    assert_eq!(model.animation.local[1].translation()[0], 4.4);
    Ok(())
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
    let actor = actor();
    let mut model = Model::new(definition(), ActorId(0), &actor)?;
    model.play(
        MotionBinding {
            model: ResourceId(7),
            clip: 0,
        },
        2.4,
        0.8,
        false,
        0,
    )?;
    model.set_end(35.2)?;
    for _ in 0..50 {
        model.step(&actor, true)?;
    }
    assert_eq!(model.shown.frame, 35.2);
    assert!(model.animation.clock.finished);
    assert!(model.set_end(f32::INFINITY).is_err());
    assert!(model.set_end(10_000.).is_err());
    Ok(())
}

#[test]
fn trimmed_body_animation_keeps_components_in_sync_without_trimming_carried_loops() -> Result<()> {
    let mut definition = (*definition()).clone();
    definition.initial.rate = 0.5;
    for (slot, playback) in [
        WeaponPlayback::Body {
            offset: 0,
            initial: definition.initial,
        },
        WeaponPlayback::Local(definition.initial),
    ]
    .into_iter()
    .enumerate()
    {
        definition.weapons.push(Arc::new(WeaponDefinition::new(
            WeaponSlot(slot as u8),
            0,
            vec![WeaponLayerDefinition {
                resource: ResourceId(8 + slot as u32),
                skeleton: definition.skeleton.clone(),
                motions: definition.motions.clone(),
                playback,
                secondary_motion: vec![],
                texture_animation: None,
            }],
            vec![],
        )?));
    }
    let definition = Arc::new(definition);
    let actor = actor();
    for repeat in [false, true] {
        let mut model = Model::new(Arc::clone(&definition), ActorId(0), &actor)?;
        model.play(
            MotionBinding {
                model: ResourceId(7),
                clip: 0,
            },
            0.,
            0.5,
            repeat,
            0,
        )?;
        model.set_end(3.)?;
        assert!(model.set_end(f32::NAN).is_err());
        let mut carried_passed_end = false;
        for _ in 0..20 {
            model.step(&actor, true)?;
            let frames = model.weapon_frames();
            assert_eq!(model.shown.frame, frames[0].frame);
            assert!(model.shown.frame <= 3.);
            carried_passed_end |= frames[1].frame > 3.;
        }
        assert!(carried_passed_end);
        if !repeat {
            assert_eq!(model.shown.frame, 3.);
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
        slot: Some(WeaponSlot(0)),
        attachment: Some(1),
        effect_interval: None,
        initially_visible: true,
        layers: vec![crate::WeaponLayerDefinition {
            texture_animation: None,
            resource: ResourceId(8),
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
    let prepared = prepare_actors(vec![actor, enemy], 1).with_entry_choices(
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
fn body_gap_uses_posed_spheres_at_current_placement_without_advancing() -> Result<()> {
    let mut definition = (*definition()).clone();
    definition.body_spheres = vec![
        BodySphere {
            bone: 0,
            radius: 1.,
        },
        BodySphere {
            bone: 1,
            radius: 2.,
        },
    ];
    let mut first = actor();
    first.position = [0.; 3];
    let mut second = first.clone();
    second.position = [100., 40., 0.];
    let mut models = models(
        &[first.clone(), second.clone()],
        vec![Some(Arc::new(definition)); 2],
    )?;
    let gap = |first: &Actor, second: &Actor| {
        models
            .body_gap(ActorId(0), first, ActorId(1), second)
            .unwrap()
    };
    assert_eq!(gap(&first, &second), 95.);
    let shown = models.models[0].as_ref().unwrap().shown.clone();
    first.position[0] = 80.;
    first.heading = 180.;
    crate::tests::assert_close(gap(&first, &second), 18., 0.0001);
    first.heading = 0.;
    first.body.scale = 2.;
    assert_eq!(gap(&first, &second), 13.);
    second.position[0] = 80.;
    assert_eq!(gap(&first, &second), 0.01);
    assert_eq!(models.models[0].as_ref().unwrap().shown, shown);
    first.position = [0.; 3];
    first.body.scale = 1.;
    second.position[0] = 100.;
    models.models[0].as_mut().unwrap().animation.clock.rate = 1.;
    models.prepare_approach_geometry(&[first.clone(), second.clone()])?;
    assert_eq!(
        models.approach_gap(ActorId(0), &first, ActorId(1), &second),
        Some(94.)
    );
    assert_eq!(
        models.body_gap(ActorId(0), &first, ActorId(1), &second),
        Some(95.)
    );
    assert_eq!(models.models[0].as_ref().unwrap().shown, shown);
    assert_eq!(models.models[0].as_ref().unwrap().animation.clock.frame, 0.);
    models.finish_approach_queries();
    assert_eq!(
        models.approach_gap(ActorId(0), &first, ActorId(1), &second),
        Some(95.)
    );
    first.hit_stop = 1;
    models.prepare_approach_geometry(&[first.clone(), second.clone()])?;
    assert_eq!(
        models.approach_gap(ActorId(0), &first, ActorId(1), &second),
        Some(95.)
    );
    models.finish_approach_queries();
    Ok(())
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
    definition.motions.get_mut(&1).unwrap().tracks[0]
        .translation
        .as_mut()
        .unwrap()
        .values = vec![[12., 0., 3.], [22., 0., 3.]];
    let mut actor = actor();
    let mut model = Model::new(Arc::new(definition), ActorId(0), &actor).unwrap();
    let held = model.shown.clone();
    model.hurt(false).unwrap();
    assert_eq!(model.shown, held);
    model.step(&actor, true).unwrap();
    model.step(&actor, true).unwrap();
    assert_eq!(model.shown.bones[1][3][0], 4.);
    let interrupted = model.shown.clone();
    model.hurt(false).unwrap();
    assert_eq!(model.shown, interrupted);
    model.step(&actor, false).unwrap();
    assert_eq!(model.shown.bones, interrupted.bones);
    for _ in 0..6 {
        model.step(&actor, true).unwrap();
    }
    assert_eq!(model.shown.clip, 1);
    assert!(model.shown.frame > 0.);
    model.hurt(false).unwrap(); // Repeated contacts restart the same clip.
    model.step(&actor, true).unwrap();
    assert_eq!(model.shown.frame, 0.);
    model.hurt(true).unwrap();
    model.step(&actor, true).unwrap();
    assert_eq!(model.shown.clip, 0);
    actor.position = [100., 20., 30.];
    actor.reaction.direction = [0., 0., 1.];
    for (remaining, offset) in (0..=10)
        .rev()
        .zip([0., 36., 0., 28., 0., 20., 0., 12., 0., 4., 0.])
    {
        actor.reaction.hit_shake_ticks = remaining;
        model.step(&actor, false).unwrap();
        assert_eq!(model.shown.world[3], [100., 20., 30. + offset, 1.]);
        assert_eq!(actor.position, [100., 20., 30.]);
    }
    actor.reaction.direction = [0.; 3];
    actor.reaction.hit_shake_ticks = 9;
    model.step(&actor, false).unwrap();
    assert_eq!(model.shown.world[3], [136., 20., 30., 1.]);
}

#[test]
fn absent_reaction_slots_preserve_playback_and_declared_missing_clips_fail() {
    let mut definition = Arc::try_unwrap(definition()).unwrap();
    definition.initial.rate = 0.5;
    definition.reactions.hurt = [None, Some(1)];
    definition.motions.get_mut(&1).unwrap().tracks[0]
        .translation
        .as_mut()
        .unwrap()
        .values = vec![[20., 0., 3.]; 2];
    definition.reactions.guard = [Some(1), None];
    let actor = actor();
    let mut model = Model::new(Arc::new(definition.clone()), ActorId(0), &actor).unwrap();
    model.hurt(false).unwrap();
    let mut airborne = actor.clone();
    airborne.position[1] = 50.;
    model
        .settle(&airborne, crate::Activity::Guarding, &mut false)
        .unwrap();
    model.step(&actor, true).unwrap();
    assert_eq!((model.shown.clip, model.shown.frame), (0, 1.));
    let mut transformed = model.clone();
    transformed.set_motion_aliases([(0, Some(1))].into());
    transformed.step(&actor, true).unwrap();
    assert_eq!((transformed.shown.clip, transformed.shown.frame), (0, 1.5));
    assert_eq!(transformed.shown.bones[1][3][0], 20.);
    transformed
        .play_part(
            MotionBinding {
                model: ResourceId(7),
                clip: 0,
            },
            Pose {
                blend: 0,
                ..Default::default()
            },
            vec![1],
        )
        .unwrap();
    transformed.set_motion_aliases([(0, None), (1, None)].into());
    transformed
        .play(
            MotionBinding {
                model: ResourceId(7),
                clip: 0,
            },
            0.,
            0.5,
            false,
            0,
        )
        .unwrap();
    transformed.hurt(true).unwrap();
    transformed
        .settle(&actor, crate::Activity::Guarding, &mut false)
        .unwrap();
    transformed.step(&actor, true).unwrap();
    assert_eq!((transformed.shown.clip, transformed.shown.frame), (0, 2.));
    assert_eq!(transformed.shown.bones[1][3][0], 20.);
    model.hurt(true).unwrap();
    model.step(&actor, true).unwrap();
    assert_eq!((model.shown.clip, model.shown.frame), (1, 0.));
    definition.reactions.guard = [Some(99), None];
    assert!(Model::new(Arc::new(definition), ActorId(0), &actor).is_err());
}

#[test]
fn reaction_poses_follow_native_state_and_keep_active_clocks() -> Result<()> {
    use crate::{Activity, RecoilKind};
    let mut definition = (*definition()).clone();
    definition.idle_motions = [Some(0); 2];
    for clip in 41..=47 {
        definition
            .motions
            .insert(clip, definition.motions[&1].clone());
    }
    definition.reactions = crate::ReactionMotions {
        hurt: [Some(41), Some(42)],
        guard: [Some(41), Some(42)],
        stunned: Some(43),
        jump: Some(44),
        backstep: Some(45),
        taunt: Some(46),
        returning: Some(44),
        stopping: Some(47),
        chant: Some(43),
        chant_rate: 1.,
        chant_loop_start: 2.,
        cast: Some(42),
        rising: Some(44),
        rising_blend: 4,
        falling: Some(45),
        falling_blend: 16,
        down: Some(46),
        get_up: Some(47),
        ..Default::default()
    };
    let mut actor = actor();
    let definition = Arc::new(definition);
    let mut model = Model::new(Arc::clone(&definition), ActorId(0), &actor)?;
    model.settle(&actor, Activity::Guarding, &mut false)?;
    model.step(&actor, true)?;
    let frame = model.shown.clone();
    model.settle(&actor, Activity::Guarding, &mut false)?;
    model.step(&actor, false)?;
    assert_eq!(model.shown, frame, "held guard keeps its playback");
    assert_eq!(frame.clip, 41);

    actor.position[1] = 50.;
    model.settle(&actor, Activity::Guarding, &mut false)?;
    assert_eq!(model.clip, 42);
    let hurt_battle = |vertical, falling_blend| -> Result<crate::Battle> {
        let mut owner = actor.clone();
        owner.position[1] = 1000.;
        let mut definition = (*definition).clone();
        definition.reactions.falling_blend = falling_blend;
        let mut battle = prepare_actors(vec![owner], 1).finish()?;
        battle.set_models(crate::Models::new(
            battle.actors(),
            vec![Some(Arc::new(definition))],
            Default::default(),
            Default::default(),
        )?);
        battle.begin_hurt(ActorId(0), 100, &mut Vec::new());
        battle.actors[0].reaction.recoil.kind = RecoilKind::Launched;
        battle.actors[0].movement.vertical = vertical;
        battle.actors[0].movement.gravity = 0.;
        Ok(battle)
    };
    let step = |battle: &mut crate::Battle, paused| -> Result<crate::BattleFrame> {
        let mut frame = battle.step(crate::BattleInput {
            paused,
            ..Default::default()
        })?;
        battle.advance_models(&mut frame, crate::BattleClock::Running, true)?;
        Ok(frame)
    };
    for (kind, vertical, clip, blend) in [
        (RecoilKind::Launched, 3., 44, 4),
        (RecoilKind::Launched, -3., 45, 16),
        (RecoilKind::Launched, -3., 45, 12),
        (RecoilKind::Down, 3., 42, 8),
        (RecoilKind::Down, 0., 42, 8),
    ] {
        let mut battle = hurt_battle(vertical, blend)?;
        battle.actors[0].reaction.recoil.kind = kind;
        for age in 0..=blend {
            let frame = step(&mut battle, false)?;
            let pose = &frame.models[0];
            assert_eq!((pose.clip, pose.frame), (clip, 0.));
            assert_eq!(pose.blend_weight, f32::from(age) / f32::from(blend + 1));
            let held = step(&mut battle, true)?;
            assert_eq!(held.models, frame.models, "paused reaction keeps its blend");
        }
        let frame = step(&mut battle, false)?;
        assert_eq!(
            (frame.models[0].frame, frame.models[0].blend_weight),
            (0.5, 1.)
        );
    }
    let mut battle = hurt_battle(0., 16)?;
    battle.model_requests.push(crate::ModelRequest::Hurt {
        actor: ActorId(0),
        alternate: false,
    });
    let mut contact = battle.publish(Vec::new());
    battle.advance_models(&mut contact, crate::BattleClock::Running, true)?;
    assert_eq!(contact.models[0].clip, 41, "contact owns its hurt pose");
    battle.actors[0].movement.gravity = crate::movement::GRAVITY;
    assert_eq!(
        step(&mut battle, false)?.models[0].clip,
        41,
        "zero velocity keeps hurt"
    );
    assert_eq!(
        step(&mut battle, false)?.models[0].clip,
        45,
        "falling follows negative velocity"
    );
    for (finished, height, expected) in [(false, 0., 42), (true, 0., 46), (true, 10., 42)] {
        let mut battle = hurt_battle(0., 16)?;
        battle.actors[0].position[1] = height;
        battle.actors[0].reaction.recoil.kind = RecoilKind::Down;
        battle.actors[0].reaction.recoil.delay = 2;
        battle.actors[0].reaction.recoil.pending = [6., 0.];
        battle.actors[0].reaction.direction = [1., 0., 0.];
        let start_x = battle.actors[0].position[0];
        let model = battle.models.as_mut().unwrap().models[0].as_mut().unwrap();
        let motion = MotionBinding {
            model: definition.resource,
            clip: 41,
        };
        let start = if finished {
            model.duration(motion)?
        } else {
            0.
        };
        model.play(motion, start, 0.5, false, 0)?;
        model.step(&battle.actors[0], true)?;
        model.step(&battle.actors[0], true)?;
        assert_eq!(step(&mut battle, false)?.models[0].clip, 41);
        let released = step(&mut battle, false)?;
        assert_eq!(
            (released.models[0].clip, released.models[0].frame),
            (expected, 0.),
            "grounded knockdown keeps completed outgoing pose readiness"
        );
        assert_eq!(battle.actors[0].position[0], start_x + 6.);
    }
    model.settle(&actor, Activity::KnockedDown, &mut false)?;
    assert_eq!(model.clip, 42, "airborne knockdown waits for landing");

    actor.position[1] = 0.;
    for (kind, blend) in [(RecoilKind::Down, 12), (RecoilKind::SettledLaunch, 4)] {
        actor.reaction.recoil.kind = kind;
        model.hurt(false)?;
        model.set_end(3.)?;
        for frame in [0., 0., 0., 0., 0., 0.5, 1., 1.5, 2., 2.5, 3.] {
            model.settle(&actor, Activity::KnockedDown, &mut true)?;
            model.step(&actor, true)?;
            assert_eq!((model.clip, model.shown.frame), (41, frame));
        }
        model.settle(&actor, Activity::KnockedDown, &mut false)?;
        assert_eq!(model.clip, 41, "paused final sample retains the hit pose");
        model.settle(&actor, Activity::KnockedDown, &mut true)?;
        assert_eq!(model.clip, 46);
        for _ in 0..=blend {
            model.step(&actor, true)?;
            assert_eq!(model.shown.frame, 0.);
            model.settle(&actor, Activity::KnockedDown, &mut false)?;
        }
        model.step(&actor, true)?;
        assert_eq!(model.shown.frame, 0.5);
        model.settle(&actor, Activity::GettingUp, &mut false)?;
        assert_eq!(model.clip, 47);
        for frame in [0., 0., 0., 0., 0., 0.5] {
            model.step(&actor, true)?;
            assert_eq!(model.shown.frame, frame);
        }
    }
    let mut fallback = (*definition).clone();
    fallback.reactions.down = None;
    fallback.reactions.stagger_down = Some(46);
    actor.reaction.recoil.kind = RecoilKind::LandingLaunch;
    let mut fallback_models = Models::new(
        &[actor.clone()],
        vec![Some(Arc::new(fallback))],
        Default::default(),
        Default::default(),
    )?;
    let fallback_model = fallback_models.models[0].as_mut().unwrap();
    fallback_model.settle(&actor, Activity::KnockedDown, &mut false)?;
    for _ in 0..5 {
        fallback_model.step(&actor, true)?;
    }
    assert_eq!(
        fallback_model.clip, 46,
        "missing down pose uses the visual fallback"
    );
    assert!(
        !fallback_models.down_pose_ready(ActorId(0)),
        "fallback does not admit landing damage"
    );

    for get_up in [None, Some(47)] {
        let mut recovery_definition = (*definition).clone();
        recovery_definition.reactions.get_up = get_up;
        let mut recovery_actor = actor.clone();
        recovery_actor.reaction.recoil.kind = RecoilKind::Normal;
        let mut recovery = prepare_actors(vec![recovery_actor], 1).finish()?;
        recovery.set_models(crate::Models::new(
            recovery.actors(),
            vec![Some(Arc::new(recovery_definition))],
            Default::default(),
            Default::default(),
        )?);
        recovery.actors[0].reaction.recoil.kind = RecoilKind::SettledLaunch;
        recovery.enter_get_up(ActorId(0));
        crate::tests::step_idle(&mut recovery, crate::knockdown::GET_UP_DURATION)?;
        if get_up.is_some() {
            assert_eq!(recovery.activity(ActorId(0)), Activity::GettingUp);
            let before = recovery.snapshot();
            recovery.step(crate::BattleInput {
                paused: true,
                ..Default::default()
            })?;
            assert_eq!(recovery.snapshot().models, before.models);
            assert_eq!(recovery.activity(ActorId(0)), Activity::GettingUp);
            for _ in 0..60 {
                recovery.step(crate::BattleInput::default())?;
                let motion = recovery
                    .models
                    .as_ref()
                    .unwrap()
                    .main_motion_observation(ActorId(0))
                    .unwrap();
                assert_eq!(motion.clip, 47);
                if motion.finished {
                    assert_eq!(motion.frame, motion.end);
                    assert_eq!(recovery.activity(ActorId(0)), Activity::Idle);
                    break;
                }
                assert_eq!(recovery.activity(ActorId(0)), Activity::GettingUp);
            }
            assert_eq!(recovery.activity(ActorId(0)), Activity::Idle);
            recovery.step(crate::BattleInput::default())?;
            let idle = recovery
                .models()
                .unwrap()
                .main_motion_observation(ActorId(0))
                .unwrap();
            assert_eq!((idle.clip, idle.frame, idle.finished), (0, 0., false));
        }
        assert_eq!(recovery.activity(ActorId(0)), Activity::Idle);
    }
    actor.reaction.recoil.kind = RecoilKind::Normal;
    let mut common_definition = (*definition).clone();
    common_definition
        .weapons
        .push(Arc::new(WeaponDefinition::body(
            None,
            vec![WeaponLayerDefinition {
                resource: ResourceId(8),
                skeleton: definition.skeleton.clone(),
                motions: definition
                    .motions
                    .iter()
                    .map(|(&clip, motion)| (clip + 60, motion.clone()))
                    .collect(),
                playback: WeaponPlayback::Body {
                    offset: 60,
                    initial: Playback {
                        clip: 60,
                        ..definition.initial
                    },
                },
                secondary_motion: vec![],
                texture_animation: None,
            }],
        )?));
    let mut models = crate::Models::new(
        std::slice::from_ref(&actor),
        vec![Some(Arc::new(common_definition))],
        Default::default(),
        Default::default(),
    )?;
    let base = prepare_actors(vec![actor], 1).finish()?.snapshot();
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
        if pose == crate::CommonPose::Chant {
            let observed = models.main_motion_observation(ActorId(0)).unwrap();
            assert_eq!((observed.rate, observed.loop_start), (1., 2.));
            let model = models.models[0].as_mut().unwrap();
            for _ in 0..8 {
                model.step(&base.actors[0], true)?;
                assert_eq!(model.shown.frame, 0.);
                assert_eq!(model.weapon_frames()[0].frame, 0.);
            }
            for expected in (1..=10).map(|frame| frame as f32).chain([3.]) {
                model.step(&base.actors[0], true)?;
                assert_eq!(model.shown.frame, expected);
                assert_eq!(model.weapon_frames()[0].frame, expected);
            }
        }
    }
    let mut frame = base.clone();
    frame.actors[0].activity = Activity::Recovering;
    frame.actors[0].movement.vertical = 0.;
    for update in 1..=100 {
        frame.update = update;
        models.advance(&mut frame, crate::BattleClock::Running, true)?;
    }
    let pose = models.main_motion_observation(ActorId(0)).unwrap();
    assert_eq!(pose.clip, 42);
    assert_eq!(pose.frame, pose.end, "sustained cast retains its endpoint");
    frame.actors[0].activity = Activity::Hurt;
    frame.model_requests.push(crate::ModelRequest::Hurt {
        actor: ActorId(0),
        alternate: false,
    });
    models.advance(&mut frame, crate::BattleClock::Running, true)?;
    assert_eq!(models.main_motion_observation(ActorId(0)).unwrap().clip, 41);
    frame.actors[0].activity = Activity::Idle;
    models.advance(&mut frame, crate::BattleClock::Running, true)?;
    assert_eq!(
        models.main_motion_observation(ActorId(0)).unwrap().clip,
        41,
        "idle availability does not cut off the new hurt pose"
    );
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
    let subject = actor();
    for (blend, translations) in [
        (0, [12.; 10]),
        (4, [12., 10., 8., 6., 4., 4., 4., 4., 4., 4.]),
    ] {
        let mut model = Model::new(Arc::clone(&definition), ActorId(0), &subject).unwrap();
        let rotation = model.shown.bones[1][0];
        model
            .play(
                MotionBinding {
                    model: ResourceId(7),
                    clip: 2,
                },
                0.,
                0.5,
                false,
                blend,
            )
            .unwrap();
        // Blending approaches the bind translation; subsequent sparse samples
        // retain its last mixed value. Without a blend, it never changes.
        for x in translations {
            model.step(&subject, true).unwrap();
            crate::tests::assert_vector_close::<3>(
                model.shown.bones[1][3][..3].try_into().unwrap(),
                [x, 0., 3.],
                0.00001,
            );
        }
        assert_ne!(model.shown.bones[1][0], rotation);
        // A whole omitted track retains its pose even during a new blend.
        let retained = model.shown.bones.clone();
        model
            .play(
                MotionBinding {
                    model: ResourceId(7),
                    clip: 3,
                },
                0.,
                0.5,
                false,
                blend,
            )
            .unwrap();
        for _ in 0..u16::from(blend) + 2 {
            model.step(&subject, true).unwrap();
            assert_eq!(model.shown.bones, retained);
        }
        // A later authored translation replaces the retained channel normally.
        model
            .play(
                MotionBinding {
                    model: ResourceId(7),
                    clip: 1,
                },
                0.,
                0.5,
                false,
                0,
            )
            .unwrap();
        model.step(&subject, true).unwrap();
        assert_eq!(model.shown.bones, retained);
        model.step(&subject, true).unwrap();
        assert_eq!(model.shown.bones[1][3][0], 2.5);
    }
}

#[test]
fn selected_clip_holds_its_start_frame_through_blending() -> Result<()> {
    let actor = actor();
    // Replacement starts retain the outgoing sample. Held blend samples then
    // approach the next pose; pausing preserves both clock and visible pose.
    for (blend, weights) in [
        (0, &[1.][..]),
        (1, &[1.][..]),
        (2, &[0., 1. / 3., 2. / 3.][..]),
        (4, &[0., 0.2, 0.4, 0.6, 0.8][..]),
    ] {
        let mut model = Model::new(definition(), ActorId(0), &actor)?;
        let outgoing = model.shown.bones.clone();
        model.play(
            MotionBinding {
                model: ResourceId(7),
                clip: 1,
            },
            3.,
            0.5,
            false,
            blend,
        )?;
        for &weight in weights {
            model.step(&actor, true)?;
            assert_eq!(model.shown.frame, 3.);
            assert_eq!(model.shown.blend_weight, weight);
            if blend == 0 {
                assert_eq!(model.shown.bones, outgoing);
            } else {
                assert!((model.shown.bones[1][3][0] - (2. + 3. * weight)).abs() < 0.00001);
            }
            let held = model.shown.clone();
            model.step(&actor, false)?;
            assert_eq!(model.shown, held);
        }
        model.step(&actor, true)?;
        assert_eq!(model.shown.frame, 3.5);
        assert_eq!(model.shown.blend_weight, 1.);
        assert_eq!(model.shown.bones[1][3][0], 5.5);
    }
    let mut model = Model::new(definition(), ActorId(0), &actor)?;
    model.play(
        MotionBinding {
            model: ResourceId(7),
            clip: 1,
        },
        3.5,
        0.5,
        false,
        0,
    )?;
    model.step(&actor, true)?;
    model.set_rate(0.)?;
    model.step(&actor, true)?;
    assert_eq!(
        (
            model.shown.clip,
            model.shown.frame,
            model.shown.blend_weight
        ),
        (1, 3.5, 1.)
    );
    model.set_rate(0.25)?;
    model.step(&actor, true)?;
    assert_eq!(model.shown.frame, 3.75);
    assert!(model.set_rate(f32::NAN).is_err());
    Ok(())
}

#[test]
fn replacement_during_blend_retains_its_baseline_and_reaches_each_endpoint() -> Result<()> {
    let actor = actor();
    let binding = MotionBinding {
        model: ResourceId(7),
        clip: 1,
    };
    for elapsed in [1, 4] {
        let mut model = Model::new(definition(), ActorId(0), &actor)?;
        model.play(binding, 8., 0.5, false, 4)?;
        for _ in 0..=elapsed {
            model.step(&actor, true)?;
        }
        assert!(model.shown.bones[1][3][0] > 2.);
        let mut baseline = 2.;
        for frame in [4., 1.] {
            let held = model.shown.clone();
            model.play(binding, frame, 0., false, 4)?;
            assert_eq!(model.shown, held);
            model.step(&actor, false)?;
            assert_eq!(model.shown.bones, held.bones);
            let endpoint = 2. + frame;
            let mut previous = baseline;
            for update in 1..=5 {
                model.step(&actor, true)?;
                let current = model.shown.bones[1][3][0];
                let expected = baseline + (endpoint - baseline) * update as f32 / 5.;
                assert!((current - expected).abs() < 0.00001);
                assert!((current - endpoint).abs() <= (previous - endpoint).abs() + 0.00001);
                previous = current;
            }
            assert!((previous - endpoint).abs() < 0.00001);
            baseline = endpoint;
        }
    }
    // Replacement requests finish the outgoing pose once when its clock runs.
    // Held, hit-stopped, resuming, and already published updates cannot add a tick.
    for (clock, paused, resuming, already_advanced, expected_x) in [
        (crate::BattleClock::Running, false, false, false, 5.5),
        (crate::BattleClock::Held, false, false, false, 5.),
        (crate::BattleClock::Running, true, false, false, 5.),
        (crate::BattleClock::Running, false, true, false, 5.),
        (crate::BattleClock::Running, false, false, true, 5.5),
    ] {
        for blend in [0, 1, 2] {
            let mut battle = crate::tests::prepared(vec![actor.clone()], 1).finish()?;
            let mut definition = (*definition()).clone();
            definition.initial = Playback {
                clip: 1,
                frame: 3.,
                rate: 0.,
                repeat: false,
            };
            let mut models = models(battle.actors(), [Some(Arc::new(definition))])?;
            models.models[0].as_mut().unwrap().set_rate(0.5)?;
            let mut frame = battle.publish(Vec::new());
            if resuming {
                frame.paused_actors.push(ActorId(0));
            }
            models.advance(
                &mut frame,
                if resuming {
                    crate::BattleClock::Running
                } else {
                    crate::BattleClock::Held
                },
                false,
            )?;
            frame.update += 1;
            frame.paused_actors.clear();
            if paused {
                frame.paused_actors.push(ActorId(0));
            }
            if already_advanced {
                models.advance(&mut frame, clock, false)?;
            }
            for start in [8., 4.] {
                frame.model_requests.push(ModelRequest::Play {
                    actor: ActorId(0),
                    motion: MotionBinding {
                        model: ResourceId(7),
                        clip: 0,
                    },
                    pose: Pose {
                        frame: start,
                        blend,
                        ..Default::default()
                    },
                });
            }
            models.advance(&mut frame, clock, false)?;
            assert_eq!(frame.models[0].frame, 4.);
            assert_eq!(
                frame.models[0].blend_weight,
                if blend <= 1 { 1. } else { 0. }
            );
            assert_eq!(
                frame.models[0].bones[1][3][0],
                if blend == 1 { 6. } else { expected_x }
            );
            let shown = frame.models[0].clone();
            models.advance(&mut frame, clock, false)?;
            assert_eq!(frame.models[0], shown);
            if blend <= 1 {
                frame.update += 1;
                frame.paused_actors.clear();
                models.advance(&mut frame, crate::BattleClock::Running, false)?;
                if paused {
                    assert_eq!(frame.models[0].frame, 4.);
                    frame.update += 1;
                    models.advance(&mut frame, crate::BattleClock::Running, false)?;
                }
                assert_eq!(frame.models[0].frame, 4.5);
                assert_eq!(frame.models[0].bones[1][3][0], 6.5);
            }
        }
    }
    Ok(())
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
                    .play(
                        MotionBinding {
                            model: ResourceId(7),
                            clip: 1
                        },
                        frame,
                        rate,
                        false,
                        4
                    )
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
    for clip in [41, 42, 43, 44] {
        definition
            .motions
            .insert(clip, definition.motions[&0].clone());
    }
    definition.initial.clip = 42;
    definition.idle_motions = [Some(41), Some(42)];
    definition.idle_expression = [1, 2, 3, 4];
    definition.reactions.walk = Some(43);
    definition.reactions.run = Some(44);
    let mut subject = actor();
    subject.hp = 24;
    let mut model = Model::new(Arc::new(definition), ActorId(0), &subject)?;
    assert_eq!(model.shown.texture_layers, [1, 2, 3, 4]);
    model.shown.texture_layers = [9; 4];
    model.settle(&subject, Activity::Idle, &mut false)?;
    assert_eq!(model.shown.texture_layers, [1, 2, 3, 4]);
    model.step(&subject, true)?;
    assert_eq!(model.shown.clip, 42);
    let shown = model.shown.clone();
    model.settle(&subject, Activity::Idle, &mut false)?;
    model.step(&subject, false)?;
    assert_eq!(
        model.shown, shown,
        "rest selection does not restart playback"
    );
    subject.hp = 25;
    model.settle(&subject, Activity::Idle, &mut false)?;
    model.step(&subject, false)?;
    assert_eq!(
        model.shown.clip, 42,
        "health changes preserve the current idle cycle"
    );
    model.animation.clock.finished = true;
    model.settle(&subject, Activity::Idle, &mut false)?;
    model.step(&subject, false)?;
    assert_eq!(model.shown.clip, 41);
    model.play(
        MotionBinding {
            model: ResourceId(7),
            clip: 1,
        },
        0.,
        1.,
        false,
        0,
    )?;
    model.settle(&subject, Activity::Action, &mut false)?;
    model.step(&subject, false)?;
    assert_eq!(model.shown.clip, 1);
    model.settle(&subject, Activity::Recovering, &mut false)?;
    assert_eq!(model.clip, 1, "unfinished recovery keeps its pose");
    subject.side = Side::Enemy;
    model.settle(&subject, Activity::Idle, &mut false)?;
    assert_eq!(model.clip, 1, "enemy idle waits for its unfinished pose");
    for _ in 0..40 {
        model.step(&subject, true)?;
    }
    subject.side = Side::Enemy;
    model.settle(&subject, Activity::Recovering, &mut false)?;
    assert_eq!(model.clip, 1, "enemy recovery retains its final pose");
    let mut completed = model.clone();
    completed.settle(&subject, Activity::Idle, &mut false)?;
    assert_eq!(completed.clip, 41, "enemy idle follows the finished pose");
    subject.side = Side::Party;
    model.settle(&subject, Activity::Recovering, &mut false)?;
    model.step(&subject, false)?;
    assert_eq!(model.shown.clip, 41);
    model.play(
        MotionBinding {
            model: ResourceId(7),
            clip: 1,
        },
        0.,
        0.5,
        true,
        0,
    )?;
    subject.movement.forward = 5.;
    model.settle(&subject, Activity::Idle, &mut false)?;
    model.step(&subject, false)?;
    assert_eq!(
        model.shown.clip, 1,
        "AI repositioning keeps its movement pose"
    );
    subject.movement.forward = 0.;
    model.settle(&subject, Activity::Recovering, &mut false)?;
    assert_eq!(model.clip, 1, "looping recovery keeps its pose");
    model.settle(&subject, Activity::Idle, &mut false)?;
    assert_eq!(model.clip, 1, "idle waits for an unfinished action pose");
    let end = model.definition.motions[&1].duration_frames;
    for side in [Side::Party, Side::Enemy] {
        subject.side = side;
        for clip in [43, 44] {
            model.play(
                MotionBinding {
                    model: ResourceId(7),
                    clip,
                },
                0.,
                0.5,
                true,
                0,
            )?;
            model.settle(&subject, Activity::Idle, &mut false)?;
            assert_eq!(model.clip, 41, "stopping locomotion permits immediate rest");
        }
        model.play(
            MotionBinding {
                model: ResourceId(7),
                clip: 1,
            },
            end,
            1.,
            true,
            0,
        )?;
        model.settle(&subject, Activity::Idle, &mut false)?;
        assert_eq!(model.clip, 1, "looping poses finish a cycle before rest");
        model.step(&subject, true)?;
        model.step(&subject, true)?;
        model.settle(&subject, Activity::Idle, &mut false)?;
        assert_eq!(model.clip, 41, "a finished loop may return to rest");
    }
    subject.side = Side::Enemy;
    subject.hp = 1;
    model.settle(&subject, Activity::Idle, &mut false)?;
    assert_eq!(model.clip, 41);
    model.play(
        MotionBinding {
            model: ResourceId(7),
            clip: 41,
        },
        3.,
        0.5,
        false,
        0,
    )?;
    model.settle(&subject, Activity::Idle, &mut false)?;
    model.step(&subject, false)?;
    assert_eq!(model.shown.frame, 3.);
    assert!(model.animation.clock.repeat);
    let end = model.definition.motions[&41].duration_frames;
    model.play(
        MotionBinding {
            model: ResourceId(7),
            clip: 41,
        },
        end,
        0.5,
        false,
        0,
    )?;
    model.step(&subject, true)?;
    model.step(&subject, true)?;
    model.settle(&subject, Activity::Idle, &mut false)?;
    model.step(&subject, true)?;
    assert!(
        model.shown.frame < end,
        "an already bound rest clip must loop"
    );
    let mut initial_override = (*model.definition).clone();
    initial_override.initial = Playback {
        clip: 1,
        frame: 0.,
        rate: 0.5,
        repeat: true,
    };
    let mut resting = Model::new(Arc::new(initial_override), ActorId(0), &subject)?;
    for _ in 0..50 {
        resting.settle(&subject, Activity::Idle, &mut false)?;
        resting.step(&subject, true)?;
        assert_eq!(resting.clip, 1, "a looping entry override remains bound");
    }
    let mut models = Models::new(
        std::slice::from_ref(&subject),
        vec![Some(Arc::clone(&model.definition))],
        Default::default(),
        Default::default(),
    )?;
    models.models[0] = Some(model);
    let mut frame = prepare_actors(vec![subject], 1).finish()?.snapshot();
    for repeat in [true, false] {
        models.models[0].as_mut().unwrap().play(
            MotionBinding {
                model: ResourceId(7),
                clip: 41,
            },
            3.,
            0.5,
            repeat,
            0,
        )?;
        frame.update += 1;
        frame.model_requests.push(ModelRequest::Common {
            actor: ActorId(0),
            pose: CommonPose::Idle,
        });
        models.advance(&mut frame, crate::BattleClock::Running, false)?;
        let pose = models.main_motion_observation(ActorId(0)).unwrap();
        assert!(pose.repeat);
        assert_eq!(
            pose.frame, 3.,
            "rest selection preserves the current idle phase"
        );
    }
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
fn forced_eyes_survive_expressions_and_blinking() -> Result<()> {
    let mut body = (*definition()).clone();
    body.forced_eye_frame = Some(15);
    body.blink = Some(resonance_content::battle_profile::Blink {
        channel: 0,
        frames: [1, 2],
        excluded_expressions: vec![],
    });
    let actor = actor();
    let mut model = Model::new(Arc::new(body), ActorId(0), &actor)?;
    for tick in 0..BLINK_PERIOD_TICKS {
        let expression = (tick % 8) as u8;
        model.shown.texture_layers = [expression; 4];
        model.step(&actor, true)?;
        assert_eq!(
            model.render_frame(&actor).texture_layers,
            [15, expression, expression, expression]
        );
    }
    Arc::make_mut(&mut model.definition).forced_eye_frame = None;
    assert_eq!(
        model.render_frame(&actor).texture_layers,
        model.shown.texture_layers
    );
    Ok(())
}

#[test]
fn shadow_appearance_does_not_depend_on_the_combat_capsule() -> Result<()> {
    let mut body = (*definition()).clone();
    body.shadow = Some(ShadowDefinition {
        scale: 6.,
        color: [16, 16, 16, 176],
    });
    let mut actor = actor();
    actor.body.collider = Some(crate::Collider::standing(30., 150.));
    let mut model = Model::new(Arc::new(body), ActorId(0), &actor)?;
    let initial = model.shown.shadow.unwrap();
    actor.body.collider = Some(crate::Collider::standing(60., 180.));
    model.sample_shadow(&actor);
    assert_eq!(model.shown.shadow, Some(initial));
    actor.position = [100., 400., 0.];
    model.sample_shadow(&actor);
    let shadow = model.shown.shadow.unwrap();
    assert_eq!(shadow.position[0], actor.position[0]);
    assert!(shadow.scale < initial.scale);
    Ok(())
}

#[test]
fn melee_groups_select_each_carried_weapon_without_combining_their_blades() -> Result<()> {
    use resonance_content::battle_action::ContactAttachments;
    let mut model = (*definition()).clone();
    model.skeleton.bones[1].name = "at03_hand".into();
    for slot in 0..2 {
        let mut skeleton = model.skeleton.clone();
        for bone in &mut skeleton.bones {
            bone.name = "at00_blade".into();
        }
        if slot == 1 {
            let mut tip = skeleton.bones[1].clone();
            tip.name = "at20_tip".into();
            skeleton.bones.push(tip);
        }
        model.weapons.push(Arc::new(WeaponDefinition::new(
            WeaponSlot(slot),
            1,
            vec![WeaponLayerDefinition {
                texture_animation: None,
                resource: ResourceId(u32::from(slot) + 8),
                skeleton,
                motions: BTreeMap::new(),
                playback: WeaponPlayback::Rigid,
                secondary_motion: vec![],
            }],
            vec![],
        )?));
    }
    for (group, expected) in [
        (0, vec![(8, 0), (8, 1)]),
        (1, vec![(9, 0), (9, 1)]),
        (2, vec![(9, 2)]),
        (3, vec![(7, 1)]),
    ] {
        let anchors = model.contact_anchors(&ContactAttachments::Groups(vec![group]))?;
        assert_eq!(
            anchors
                .iter()
                .map(|a| (a.resource.0, a.bone))
                .collect::<Vec<_>>(),
            expected
        );
    }
    model
        .weapons
        .retain(|weapon| weapon.slot == Some(WeaponSlot(0)));
    let mut opening_groups = vec![0, 1];
    opening_groups.retain(|group| !model.contact_group_anchors(*group).is_empty());
    assert_eq!(opening_groups, [0]);
    assert_eq!(
        model
            .contact_anchors(&ContactAttachments::Groups(opening_groups))?
            .len(),
        2
    );
    assert!(
        model
            .contact_anchors(&ContactAttachments::Groups(vec![0, 1]))
            .is_err()
    );
    Ok(())
}

#[test]
fn melee_uses_this_updates_pose_and_drawing_does_not_advance_it_twice() -> Result<()> {
    use crate::{
        ActionDefinition, ActionExecution, ActionKey, ActionRequest, AttackEvent, AttackPose,
        BattleInput, HitRule, MeleeDefinition, MeleeVolume, PreparedAttack,
    };
    let mut definition = (*definition()).clone();
    definition.skeleton.bones[1].name = "at00_hand".into();
    use resonance_content::battle_action::ContactAttachments;
    let anchors = definition.contact_anchors(&ContactAttachments::Groups(vec![0, 0]))?;
    let direct = definition.contact_anchors(&ContactAttachments::BodyBone(1))?;
    assert_eq!(direct.len(), 1);
    assert_eq!(
        (direct[0].resource, direct[0].bone),
        (anchors[0].resource, anchors[0].bone)
    );
    assert!(
        definition
            .contact_anchors(&ContactAttachments::BodyBone(u16::MAX))
            .is_err()
    );
    let definition = Arc::new(definition);
    let mut owner = crate::tests::actor(Side::Party);
    owner.body.collider = None;
    let mut target = crate::tests::actor(Side::Enemy);
    target.position = [3., 3., 0.];
    target.body.collider = Some(crate::Collider::sphere(0.01));
    let hp = target.hp;
    let action = ActionDefinition {
        normal: None,
        tp_cost: 0,
        execution: ActionExecution::Attack(PreparedAttack {
            contact_recovery: None,
            chain_at: None,
            end_at: 4,
            recovery: 1,
            opening: Some(AttackPose {
                motion: MotionBinding {
                    model: ResourceId(7),
                    clip: 1,
                },
                start: 0.,
                end: Some(5.),
                rate: 1.,
                blend: 0,
                repeat: false,
            }),
            events: vec![(
                1,
                AttackEvent::Contact {
                    duration: 3,
                    definition: Arc::new(MeleeDefinition {
                        hit: HitRule {
                            power: crate::Power::Fixed(1),
                            ..crate::tests::hit()
                        },
                        volume: MeleeVolume::Attached {
                            anchors,
                            radius: 0.1,
                            half_height: 0.1,
                            shape: crate::HitShape::Sphere,
                        },
                        trail: None,
                    }),
                },
            )],
        }),
    };
    let mut prepared = PreparedBattle::new(
        vec![(owner, Default::default()), (target, Default::default())],
        vec![action].into(),
        1,
    )?;
    crate::tests::assign_action(&mut prepared, 0, ActionKey(0));
    let mut battle = prepared.finish()?;
    battle.set_models(models(battle.actors(), [Some(definition), None])?);
    battle.step(BattleInput::from(ActionRequest {
        actor: ActorId(0),
        target: ActorId(1),
        action: ActionKey(0),
    }))?;
    assert_eq!(battle.actors()[1].hp, hp);
    let mut frame = battle.step(BattleInput::default())?;
    // The old pose's hand is at X=2; this update moves it to X=3 before contact.
    assert_eq!(
        battle.actors()[1].hp,
        hp - 1,
        "overlapping anchors must produce one hit"
    );
    let observe = |battle: &crate::Battle| {
        battle
            .models()
            .unwrap()
            .main_motion_observation(ActorId(0))
            .unwrap()
    };
    assert_eq!(observe(&battle).frame, 1.);
    battle.advance_models(&mut frame, crate::BattleClock::Running, false)?;
    assert_eq!(observe(&battle).frame, 1.);
    assert_eq!(frame.models[0].frame, 1.);
    battle.advance_models(&mut frame, crate::BattleClock::Running, false)?;
    assert_eq!(observe(&battle).frame, 1.);
    battle.actors[0].hit_stop = 1;
    let mut stopped = battle.step(BattleInput::default())?;
    assert_eq!(stopped.actors[0].hit_stop, 0);
    assert!(stopped.paused_actors.contains(&ActorId(0)));
    battle.advance_models(&mut stopped, crate::BattleClock::Running, false)?;
    assert_eq!(observe(&battle).frame, 1.);
    let mut held = battle.step(BattleInput {
        paused: true,
        ..Default::default()
    })?;
    battle.advance_models(&mut held, crate::BattleClock::Held, false)?;
    assert_eq!(observe(&battle).frame, 1.);
    assert_eq!(observe(&battle).end, 5.);
    Ok(())
}

#[test]
fn bone_projectile_uses_current_pose_and_moves_on_its_authored_update() -> Result<()> {
    use crate::{
        ActionKey, ActionRequest, AttackEvent, AttackPose, BattleInput, Cue, EffectAppearance,
        ProjectileBoneDirection, ProjectileDefinition, ProjectileMotion, ProjectileOrigin,
    };
    let projectile = Arc::new(ProjectileDefinition {
        lifetime: Some(10),
        birth: Some(EffectAppearance {
            resource: ResourceId(1),
            member: 0,
        }),
        motion: ProjectileMotion {
            origin: ProjectileOrigin::Bone(1),
            bone_direction: Some(ProjectileBoneDirection { from: 0, to: 1 }),
            speed: Some(2.),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut owner = crate::tests::actor(Side::Party);
    owner.body.collider = None;
    let mut target = crate::tests::actor(Side::Enemy);
    target.position = [1000., 0., 0.];
    let mut prepared = crate::tests::prepared(vec![owner, target], 4);
    let action =
        crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0]));
    action.opening = Some(AttackPose {
        motion: MotionBinding {
            model: ResourceId(7),
            clip: 1,
        },
        start: 0.,
        end: None,
        rate: 0.5,
        blend: 0,
        repeat: false,
    });
    action.events = vec![(1, AttackEvent::Projectile(projectile))];
    let mut battle = prepared.finish()?;
    battle.set_models(models(battle.actors(), [Some(definition()), None])?);
    let first = battle.step(BattleInput::from(ActionRequest {
        actor: ActorId(0),
        target: ActorId(1),
        action: ActionKey(0),
    }))?;
    assert!(first.projectiles.is_empty());
    let mut frame = battle.step(BattleInput::default())?;
    let models = battle.models().unwrap();
    assert_eq!(
        models.main_motion_observation(ActorId(0)).unwrap().frame,
        0.5
    );
    let anchor = |bone| {
        models.anchor(
            ActorId(0),
            crate::MeleeAnchor {
                resource: ResourceId(7),
                bone,
            },
        )
    };
    let origin = anchor(1)?;
    let root = anchor(0)?;
    let direction =
        crate::distance::normalize(std::array::from_fn(|axis| origin[axis] - root[axis]));
    assert_eq!(frame.projectiles.len(), 1);
    let shot = &frame.projectiles[0];
    assert_eq!(shot.age, 0);
    for axis in 0..3 {
        assert!((shot.velocity[axis] - direction[axis]).abs() < 0.00001);
        assert!((shot.position[axis] - origin[axis] - 2. * direction[axis]).abs() < 0.00001);
    }
    assert!(
        frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::InitializedEffect(effect)
        if effect.appearance.resource == ResourceId(1) && effect.origin == origin))
    );
    let position = shot.position;
    battle.advance_models(&mut frame, crate::BattleClock::Running, false)?;
    assert_eq!(
        frame.models[0].frame, 0.5,
        "drawing must not advance the launch pose twice"
    );
    let next = battle.step(BattleInput::default())?;
    assert_eq!(next.projectiles[0].age, 1);
    for axis in 0..3 {
        assert!(
            (next.projectiles[0].position[axis] - position[axis] - 2. * direction[axis]).abs()
                < 0.00001
        );
    }
    Ok(())
}

#[test]
fn external_model_samples_hold_and_blend_without_advancing_a_combat_clock() -> Result<()> {
    let actor = actor();
    let mut sampled = ModelSampler::new(definition(), ActorId(0), &actor)?;
    let sample = ModelSample {
        clip: 1,
        frame: 8.,
        blend_weight: 0.25,
        restart: true,
        rate: 0.,
        repeat: false,
        blend_ticks: 4,
    };
    let (shown, _) = sampled.sample(&actor, sample, true)?;
    assert_eq!(shown.frame, 8.);
    assert_eq!(shown.bones[1][3][0], 4.);
    let held = ModelSample {
        restart: false,
        ..sample
    };
    assert_eq!(sampled.sample(&actor, held, false)?.0, shown);
    let complete = ModelSample {
        blend_weight: 1.,
        ..held
    };
    assert_eq!(
        sampled.sample(&actor, complete, true)?.0.bones[1][3][0],
        10.
    );
    assert_eq!(sampled.0.animation.clock.rate, 0.);
    Ok(())
}

#[test]
fn reception_markers_follow_current_placement_for_both_sides() -> Result<()> {
    use glam::{Mat4, Vec3};

    let id = ActorId(0);
    let mut subject = actor();
    subject.position = [100., 20., -30.];
    subject.body.scale = 2.;
    let mut battle = crate::tests::prepared(vec![subject], 1).finish()?;
    let fallback = battle.hurt_points(id, 0., None).next().unwrap();
    assert_eq!(fallback.center, [100., 20., -30.]);
    assert_eq!(fallback.radius, 1.);

    let mut definition = (*definition()).clone();
    definition.skeleton.bones.push(Bone {
        name: "dm8_upper".into(),
        parent: Some(0),
        bind_channels: TransformChannels(8),
        bind: Transform::default(),
    });
    definition.reception_spheres = Some(vec![
        BodySphere {
            bone: 1,
            radius: 20.,
        },
        BodySphere {
            bone: 2,
            radius: 160.,
        },
    ]);
    battle.set_models(models(battle.actors(), [Some(Arc::new(definition))])?);
    let translation = |point| Mat4::from_translation(Vec3::from_array(point)).to_cols_array_2d();
    let model = battle.models.as_mut().unwrap().models[0].as_mut().unwrap();
    model.shown.bones = Arc::new(vec![
        translation([-1000.; 3]),
        translation([-10., 10., -4.]),
        translation([30., 110., 8.]),
    ]);
    for side in [Side::Party, Side::Enemy] {
        battle.actors[0].side = side;
        // Actor-profile capsule dimensions do not replace authored reception markers.
        battle.actors[0].body.collider = Some(crate::Collider::standing(42., 180.));
        let points: Vec<_> = battle.hurt_points(id, 0., None).collect();
        assert_eq!(points.len(), 2);
        assert_eq!(
            (points[0].center, points[0].radius),
            ([80., 12., -50.], 10.)
        );
        assert_eq!(
            (points[1].center, points[1].radius),
            ([160., 36., -250.], 80.)
        );
    }
    let placement = battle.models.as_ref().unwrap().body_placements()[0];
    battle.actors[0].position[0] += 7.;
    assert_eq!(
        battle.hurt_points(id, 0., None).next().unwrap().center,
        [87., 12., -50.]
    );
    // Ordinary contacts retain placement while using the completed pose.
    let model = battle.models.as_mut().unwrap().models[0].as_mut().unwrap();
    Arc::make_mut(&mut model.shown.bones)[1][3][0] += 4.;
    for side in [Side::Party, Side::Enemy] {
        battle.actors[0].side = side;
        assert_eq!(
            battle.hurt_points(id, 0., placement).next().unwrap().center,
            [88., 12., -50.]
        );
    }

    let model = battle.models.as_mut().unwrap().models[0].as_mut().unwrap();
    Arc::make_mut(&mut model.definition).reception_spheres = Some(vec![]);
    assert_eq!(battle.hurt_points(id, 0., None).count(), 0);
    battle.models = None;
    assert_eq!(battle.hurt_points(id, 0., None).next().unwrap().radius, 42.);
    battle.actors[0].body.collider = None;
    assert_eq!(battle.hurt_points(id, 0., None).count(), 0);
    Ok(())
}
