use super::*;
use crate::{
    ActionPhase, ActionRequest, Battle, BattleInput, PreparedBattle, ResourceBinding, Side,
};
use resonance_content::animation::{
    Bone, Track, Transform, TransformChannels, VectorCurve, VectorInterpolation,
};

#[test]
fn body_clocks_match_original_dolphin_model_visits() {
    for source in [
        include_str!("../../tests/fixtures/opening-motion.json"),
        include_str!("../../tests/fixtures/nurse-transition-motion.json"),
    ] {
        let fixture: serde_json::Value = serde_json::from_str(source).unwrap();
        for row in fixture["observations"].as_array().unwrap() {
            let before = &row["before"];
            let float = |name: &str| f32::from_bits(before[name].as_u64().unwrap() as u32);
            let mut clock = Clock {
                frame: float("frame_bits"),
                start: float("start_bits"),
                end: float("end_bits"),
                loop_start: float("loop_start_bits"),
                rate: float("rate_bits"),
                repeat: before["repeat"].as_bool().unwrap(),
                stopped: before["stopped"].as_bool().unwrap(),
                finished: before["finished"].as_bool().unwrap(),
                blend: before["blend"].as_u64().unwrap() as u8,
                blend_age: before["blend_age"].as_u64().unwrap() as u8,
            };
            if row["flags"].as_u64().unwrap() & 1 != 0 {
                clock.step();
            }
            let after = &row["after"];
            assert_eq!(
                clock.frame.to_bits(),
                after["frame_bits"].as_u64().unwrap() as u32,
                "{row}"
            );
            assert_eq!(
                clock.finished,
                after["finished"].as_bool().unwrap(),
                "{row}"
            );
            assert_eq!(clock.stopped, after["stopped"].as_bool().unwrap(), "{row}");
            assert_eq!(
                clock.blending(),
                after["blending"].as_bool().unwrap(),
                "{row}"
            );
            if clock.blending() {
                assert_eq!(
                    clock.blend_age,
                    after["blend_age"].as_u64().unwrap() as u8,
                    "{row}"
                );
            }
        }
    }
}

fn clock(frame: f32, rate: f32, repeat: bool, blend: u8) -> Clock {
    Clock::new(
        Playback {
            clip: 1,
            frame,
            rate,
            repeat,
        },
        10.,
        blend,
    )
    .unwrap()
}

#[test]
fn endpoints_wrap_once_reverse_tests_zero_and_completion_latches() {
    let mut forward = clock(2., 0.5, true, 0);
    forward.frame = 10.;
    forward.step();
    assert_eq!(forward.frame, 2.5);
    assert!(forward.finished);
    forward.step();
    assert!(forward.finished);
    forward.rate = 40.;
    forward.step();
    assert_eq!(forward.frame, 2.);
    let mut reverse = clock(2., -0.5, true, 0);
    for _ in 0..4 {
        reverse.step();
    }
    assert_eq!(reverse.frame, 0.);
    assert!(!reverse.finished);
    reverse.step();
    assert_eq!(reverse.frame, 9.5);
    let mut clamp = clock(2., -0.5, false, 0);
    for _ in 0..5 {
        clamp.step();
    }
    assert_eq!(clamp.frame, 2.);
    assert!(clamp.stopped && clamp.finished);
    clamp.step();
    assert_eq!(clamp.frame, 2.);
}

#[test]
fn blending_holds_time_and_stopped_tracks_still_blend() {
    let mut c = clock(1., 0.5, false, 4);
    c.stopped = true;
    for age in 1..=4 {
        assert_eq!(c.step(), age as f32 / 5.);
        assert_eq!(c.frame, 1.);
    }
    assert!(!c.blending());
    assert_eq!(c.step(), 1.);
    assert_eq!(c.frame, 1.);
    c.stopped = false;
    c.step();
    assert_eq!(c.frame, 1.5);
    assert!(!clock(0., 0.5, false, 1).blending());
}

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
        secondary_motion: vec![],
        hurt_motions: [None; 2],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        stun: None,
        knockdown: None,
        resource: 7,
        skeleton,
        motions: BTreeMap::from([(0, motion.clone()), (1, motion)]),
        initial: Playback {
            clip: 0,
            frame: 0.,
            rate: 0.,
            repeat: true,
        },
        anchors: vec![Anchor {
            bone: 1,
            offset: [4., 0., 0.],
        }],
        approach_bones: vec![],
        target_bones: vec![],
        shadow: None,
        target_marker: None,
        weapons: vec![],
        hurt_bones: vec![1],
        suppress_root_translation: [false; 3],
    })
}

#[test]
fn entry_composition_matches_original_initial_and_early_idle_cursors() -> Result<()> {
    // Source watch04: P0 is the completed 52AA8 composition; C0=P59.
    // Rows are Lloyd, Genis and Zombie from the unchanged opening replay.
    for (phase, duration, expected) in [
        (10., 60., [10.5, 40., 42.5, 50.]),
        (19., 40., [19.5, 9., 11.5, 19.]),
        (18., 40., [18.5, 8., 10.5, 18.]),
    ] {
        let mut definition = (*definition()).clone();
        let motion = definition.motions.get_mut(&0).unwrap();
        motion.duration_frames = duration;
        motion.tracks.clear();
        definition.initial.frame = phase;
        definition.initial.rate = 0.5;
        let mut actor = actor();
        let mut model = Model::new(Arc::new(definition), ActorId(0), &mut actor)?;
        assert_eq!(model.shown.frame, expected[0]);
        assert_eq!(model.animation.clock.frame, expected[0]);
        for visit in 1..=79 {
            model.step(&mut actor, true, PlacementUpdate::Actor)?;
            if let Some(index) = [59, 64, 79].iter().position(|&sample| sample == visit) {
                assert_eq!(model.shown.frame, expected[index + 1], "P{visit}");
                assert_eq!(model.animation.clock.frame, model.shown.frame);
            }
        }
    }
    // Colette's non-looping entry first draws .5, reaches30 at P59, and
    // finishes on P60/C1. That callback can then request her idle blend.
    let mut definition = (*definition()).clone();
    definition.motions.get_mut(&0).unwrap().duration_frames = 30.;
    definition.initial.rate = 0.5;
    definition.initial.repeat = false;
    let mut actor = actor();
    let mut model = Model::new(Arc::new(definition), ActorId(0), &mut actor)?;
    assert_eq!(model.shown.frame, 0.5);
    for _ in 0..59 {
        model.step(&mut actor, true, PlacementUpdate::Actor)?;
    }
    assert_eq!(model.shown.frame, 30.);
    assert!(!model.finished());
    model.step(&mut actor, true, PlacementUpdate::Actor)?;
    assert_eq!(model.shown.frame, 30.);
    assert!(model.finished());
    Ok(())
}

#[test]
fn entry_composition_advances_secondary_once_at_the_advanced_animation_pose() -> Result<()> {
    use glam::Vec3;
    use resonance_content::secondary_motion::{Chain, Environment, Joint, Simulation, UpAxis};
    let mut definition = (*definition()).clone();
    definition.initial.rate = 0.5;
    let chain = Chain {
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
    };
    definition.secondary_motion.push(chain.clone());
    let mut actor = actor();
    actor.position[1] = 50.;
    let model = Model::new(Arc::new(definition), ActorId(0), &mut actor)?;
    let mut expected = Simulation::default();
    expected.advance(
        &chain,
        &[Vec3::new(0., 50., 0.), Vec3::new(2.5, 53., 0.)],
        None,
        chain.attraction,
        1,
        Environment {
            up: UpAxis::Y,
            acceleration: Vec3::ZERO,
            floor: Some(5.),
        },
    );
    assert_eq!(model.shown.frame, 0.5);
    assert_eq!(model.secondary[0].positions(), expected.positions());
    assert_eq!(model.secondary[0].velocity(), expected.velocity());
    Ok(())
}

#[test]
fn entry_placement_retains_origin_then_recovers_at_first_actor_visit() -> Result<()> {
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
        resource: 8,
        attachment: 1,
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
        anchors: vec![],
        links: vec![],
    }));
    let mut actor = actor();
    actor.position = [-300., 50., 0.];
    actor.heading = 90.;
    actor.body.scale = 2.;
    let mut model = Model::new(Arc::new(definition), ActorId(0), &mut actor)?;
    // Generic construction can still resume a sampled world pose.
    assert_eq!(model.shown.world, world(&actor));
    assert_eq!(model.shown.frame, 0.5);
    assert_eq!(model.weapon_frames()[0].frame, 2.);
    model.initialize_entry_placement(&mut actor)?;
    // P0 in source04: all four bodies have origin, -90 X and unit scale.
    assert_eq!(
        model.shown.world,
        [
            [1., 0., 0., 0.],
            [0., 0., -1., 0.],
            [0., 1., 0., 0.],
            [0., 0., 0., 1.],
        ]
    );
    assert_eq!(model.sampled_heading(), 0.);
    assert_eq!(model.shown.frame, 0.5);
    assert_eq!(model.animation.clock.frame, 0.5);
    assert_eq!(model.weapon_frames()[0].frame, 2.);
    assert_eq!(actor.position, [-300., 50., 0.]);
    assert_eq!(actor.heading, 90.);
    assert_eq!(actor.body.scale, 2.);
    let retained = model.secondary[0].positions().to_vec();
    model.step(&mut actor, false, PlacementUpdate::Held)?;
    assert_eq!(model.secondary[0].positions(), retained);
    assert_eq!(model.shown.frame, 0.5);
    model.step(&mut actor, true, PlacementUpdate::Actor)?;
    assert_eq!(model.shown.frame, 1.);
    assert_eq!(model.shown.world, world(&actor));
    assert_eq!(model.weapon_frames()[0].frame, 2.5);
    // P1 relocation uses the shared >100 recovery-radius rule, resetting all
    // momentum. No entry-only reset is applied during this ordinary visit.
    for (position, target) in model.secondary[0]
        .positions()
        .iter()
        .zip(model.secondary[0].targets())
    {
        assert!((*position - *target).length() < 0.0001);
    }
    for velocity in model.secondary[0].velocity() {
        assert!(velocity.length() < 0.0001);
    }
    model.step(&mut actor, true, PlacementUpdate::Actor)?;
    assert!(
        model.secondary[0]
            .velocity()
            .iter()
            .any(|v| v.length() > 0.01)
    );
    Ok(())
}

#[test]
fn secondary_motion_keeps_history_and_leaves_terminal_guides_at_the_sampled_pose() {
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
    definition.anchors = (0..3)
        .map(|bone| Anchor {
            bone,
            offset: [0.; 3],
        })
        .collect();
    let mut subject = actor();
    subject.position = [0., 50., 0.];
    let mut animated = Model::new(Arc::new(definition.clone()), ActorId(0), &mut subject).unwrap();
    let before = animated.shown.clone();
    for _ in 0..8 {
        animated
            .step(&mut subject, true, PlacementUpdate::Actor)
            .unwrap();
    }
    assert_ne!(before.bones[1], animated.shown.bones[1]);
    let mut fresh = definition.clone();
    fresh.initial.frame = animated.shown.frame;
    let restarted = Model::new(Arc::new(fresh), ActorId(0), &mut subject.clone()).unwrap();
    assert_ne!(animated.shown.bones[1], restarted.shown.bones[1]);
    let mut ordinary = definition;
    ordinary.secondary_motion.clear();
    ordinary.initial.frame = animated.shown.frame;
    let mut comparison = subject.clone();
    let sampled = Model::new(Arc::new(ordinary), ActorId(0), &mut comparison).unwrap();
    assert_eq!(animated.shown.bones[2], sampled.shown.bones[2]);
    assert_ne!(subject.body.anchors[1], comparison.body.anchors[1]);
    assert_eq!(subject.body.anchors[2], comparison.body.anchors[2]);

    let mut resumed = animated.clone();
    let positions = animated.secondary[0].positions().to_vec();
    let velocity = animated.secondary[0].velocity().to_vec();
    let anchors = subject.body.anchors.clone();
    subject.position[0] += 10.;
    for _ in 0..60 {
        animated
            .step(&mut subject, false, PlacementUpdate::Actor)
            .unwrap();
        assert_eq!(animated.secondary[0].positions(), positions);
        assert_eq!(animated.secondary[0].velocity(), velocity);
        // Driven joints retain world positions, while the undriven terminal
        // guide follows the recomposed actor transform.
        assert!((subject.body.anchors[1][0] - anchors[1][0]).abs() < 0.00001);
        assert_eq!(subject.body.anchors[2][0], anchors[2][0] + 10.);
    }
    animated
        .step(&mut subject, true, PlacementUpdate::Actor)
        .unwrap();
    resumed
        .step(&mut subject.clone(), true, PlacementUpdate::Actor)
        .unwrap();
    assert_eq!(animated.shown, resumed.shown);

    // The model-local jitter vector drives the retained chain, leaving root
    // placement and terminal guides alone, and is consumed by composition.
    let mut jittered = animated.clone();
    let mut jittered_actor = subject.clone();
    jittered_actor.body.jitter.request(8);
    jittered_actor
        .body
        .jitter
        .advance(&mut crate::Random::from_state(1));
    jittered
        .step(&mut jittered_actor, true, PlacementUpdate::Actor)
        .unwrap();
    resumed
        .step(&mut subject.clone(), true, PlacementUpdate::Actor)
        .unwrap();
    assert_eq!(jittered.shown.world, resumed.shown.world);
    assert_eq!(jittered.shown.bones[2], resumed.shown.bones[2]);
    assert_ne!(jittered.shown.bones[1], resumed.shown.bones[1]);
    assert_eq!(jittered_actor.body.jitter.take_acceleration(), [0.; 3]);
    let positions = jittered.secondary[0].positions().to_vec();
    jittered_actor
        .body
        .jitter
        .advance(&mut crate::Random::from_state(1));
    jittered
        .step(&mut jittered_actor, false, PlacementUpdate::Held)
        .unwrap();
    assert_eq!(jittered.secondary[0].positions(), positions);
    assert_eq!(jittered_actor.body.jitter.take_acceleration(), [0.; 3]);
    assert_eq!(jittered_actor.body.jitter.remaining, 6);

    let mut invalid = (*animated.definition).clone();
    invalid.secondary_motion[0].joints[1].node = 255;
    assert!(Model::new(Arc::new(invalid), ActorId(0), &mut comparison).is_err());

    let mut battle = Battle::new(Arc::new(
        PreparedBattle::new(
            vec![subject],
            vec![],
            1,
            vec![Some(animated.definition.clone())],
            vec![],
        )
        .unwrap(),
    ));
    let shown = battle.step(BattleInput::default()).unwrap().models;
    for _ in 0..3 {
        assert_eq!(
            battle
                .step(BattleInput {
                    menu_open: true,
                    ..Default::default()
                })
                .unwrap()
                .models,
            shown
        );
    }
    assert_ne!(battle.step(BattleInput::default()).unwrap().models, shown);
}

fn actor() -> Actor {
    let mut actor = crate::tests::actor(Side::Party);
    actor.body.points.push(crate::HurtPoint {
        center: [0.; 3],
        radius: 1.,
    });
    actor
}

#[test]
fn contact_shake_moves_sampled_bones_and_attached_weapons_without_moving_the_actor() -> Result<()> {
    let mut definition = (*definition()).clone();
    definition.target_bones = vec![1];
    definition.weapons.push(Arc::new(WeaponDefinition {
        slot: 0,
        resource: 8,
        attachment: 1,
        skeleton: definition.skeleton.clone(),
        motions: BTreeMap::new(),
        playback: WeaponPlayback::Rigid,
        anchors: vec![Anchor {
            bone: 1,
            offset: [0.; 3],
        }],
        links: vec![],
    }));
    let mut actor = actor();
    actor.position = [100., 40., -20.];
    actor.heading = 90.;
    actor.body.scale = 2.;
    actor.reaction.direction = [-2., 0.25, 0.5];
    let mut model = Model::new(Arc::new(definition), ActorId(0), &mut actor)?;
    let baseline = model.shown.clone();
    let anchors = actor.body.anchors.clone();
    let hurt = actor.body.points[0].center;
    let target = actor.body.target_center;
    let weapon = model.weapon_frames()[0].clone();
    actor.hud.contact(crate::GuardResult::None);
    assert_eq!(actor.hud.portrait_bounce, 10);
    model.step(&mut actor, false, PlacementUpdate::Actor)?;
    assert_eq!(model.shown, baseline);
    actor.hud.common();
    assert_eq!(actor.hud.portrait_bounce, 9);
    model.step(&mut actor, false, PlacementUpdate::Actor)?;
    // Original integer18 * rodata2.0 scales the unnormalized world vector.
    let offset = [-72., 9., 18.];
    let shifted = |actual: [f32; 3], before: [f32; 3]| {
        for axis in 0..3 {
            assert!((actual[axis] - (before[axis] + offset[axis])).abs() < 0.0001);
        }
    };
    assert_eq!(actor.position, [100., 40., -20.]);
    assert_eq!(model.shown.world[3], [28., 49., -2., 1.]);
    assert_eq!(model.shown.bones, baseline.bones);
    assert_eq!(model.shown.tint, baseline.tint);
    for (&actual, &before) in actor.body.anchors.iter().zip(&anchors) {
        shifted(actual, before);
    }
    shifted(actor.body.points[0].center, hurt);
    shifted(actor.body.target_center, target);
    shifted(
        model.weapon_attachment(0)?,
        weapon.world[3][..3].try_into().unwrap(),
    );
    shifted(
        model.weapon_frames()[0].world[3][..3].try_into().unwrap(),
        weapon.world[3][..3].try_into().unwrap(),
    );

    // A detached weapon keeps its independent world, while its return hand
    // still comes from the shaken body sample.
    model.detach_weapon(0, Some(weapon.world), &mut actor)?;
    model.step(&mut actor, false, PlacementUpdate::Actor)?;
    assert_eq!(model.weapon_frames()[0].world, weapon.world);
    shifted(
        model.weapon_attachment(0)?,
        weapon.world[3][..3].try_into().unwrap(),
    );
    actor.hud.common();
    model.step(&mut actor, false, PlacementUpdate::Actor)?;
    assert_eq!(actor.hud.portrait_bounce, 8);
    assert_eq!(model.shown, baseline);
    Ok(())
}

#[test]
fn shake_uses_sdk_direction_threshold_and_retains_transition_placement() -> Result<()> {
    let mut actor = actor();
    actor.hud.portrait_bounce = 9;
    let mut model = Model::new(definition(), ActorId(0), &mut actor)?;
    assert_eq!(model.shown.world[3], [36., 0., 0., 1.]);
    actor.reaction.direction = [0., 0.05, 0.];
    model.step(&mut actor, false, PlacementUpdate::Actor)?;
    assert_eq!(model.shown.world[3], [36., 0., 0., 1.]);
    // SDK length's estimate straddles 0.1 differently from a host sqrt.
    for (bits, admitted) in [(0x3dcccccc, true), (0x3dcccccd, false), (0x3dccccce, true)] {
        let value = f32::from_bits(bits);
        actor.reaction.direction = [0., value, 0.];
        model.step(&mut actor, false, PlacementUpdate::Actor)?;
        assert_eq!(
            model.shown.world[3],
            if admitted {
                [0., value * 36., 0., 1.]
            } else {
                [36., 0., 0., 1.]
            },
        );
    }
    actor.reaction.direction = [0., 0.125, 0.];
    model.step(&mut actor, false, PlacementUpdate::Actor)?;
    assert_eq!(model.shown.world[3], [0., 4.5, 0., 1.]);
    let held = model.shown.clone();
    actor.heading = 90.;
    actor.body.scale = 2.;
    actor.position = [100., 200., 300.];
    actor.hud.portrait_bounce = 8;
    model.step(&mut actor, false, PlacementUpdate::Held)?;
    assert_eq!(model.shown, held);
    assert_eq!(model.sampled_heading(), 0.);
    model.step(&mut actor, true, PlacementUpdate::TransitionOwner)?;
    assert_eq!(model.shown.world[3], held.world[3]);
    assert_eq!(model.sampled_heading(), 90.);
    assert_ne!(model.shown.world[..3], held.world[..3]);
    actor.hud.portrait_bounce = 7;
    model.step(&mut actor, true, PlacementUpdate::TransitionOwner)?;
    assert_eq!(model.shown.world[3], [100., 203.5, 300., 1.]);
    actor.hud.portrait_bounce = 6;
    model.step(&mut actor, true, PlacementUpdate::Actor)?;
    assert_eq!(model.shown.world[3], [100., 200., 300., 1.]);
    Ok(())
}

#[test]
fn body_shake_samples_before_common_decrement_and_survives_local_hit_stop() -> Result<()> {
    let mut actor = actor();
    actor.hit_stop = 20;
    actor.reaction.direction = [0., 0., 1.];
    let mut battle = Battle::new(Arc::new(PreparedBattle::new(
        vec![actor],
        vec![],
        1,
        vec![Some(definition())],
        vec![],
    )?));
    // This contact countdown does not depend on optional effect resources.
    battle.actors[0].hud.contact(crate::GuardResult::None);
    for (clock, translation) in [(9, 0.), (8, 36.), (7, 0.), (6, 28.)] {
        let frame = battle.step(BattleInput::default())?;
        assert_eq!(frame.actors[0].hud.portrait_bounce, clock);
        assert_eq!(frame.models[0].world[3], [0., 0., translation, 1.]);
        assert_eq!(frame.actors[0].position, [0.; 3]);
        assert!(frame.actors[0].hit_stop > 0);
        let paused = battle.step(BattleInput {
            menu_open: true,
            ..Default::default()
        })?;
        assert_eq!(paused.models, frame.models);
        assert_eq!(paused.actors, frame.actors);
    }
    battle.actors[0].petrified = true;
    battle.actors[0].hud.portrait_bounce = 5;
    for _ in 0..2 {
        let frame = battle.step(BattleInput::default())?;
        assert_eq!(frame.actors[0].hud.portrait_bounce, 5);
        assert_eq!(frame.models[0].world[3], [0., 0., 20., 1.]);
    }
    Ok(())
}

#[test]
fn target_bounds_use_selected_bones_and_exclude_points_below_source_floor() -> Result<()> {
    let mut definition = (*definition()).clone();
    for (name, translation) in [("top", [8., 4., 7.]), ("below", [999., 0., 0.05])] {
        definition.skeleton.bones.push(Bone {
            name: name.into(),
            parent: Some(0),
            bind_channels: TransformChannels(8),
            bind: Transform {
                translation,
                ..Default::default()
            },
        });
    }
    definition.target_bones = vec![0, 1, 2, 3];
    let mut subject = actor();
    Model::new(Arc::new(definition), ActorId(0), &mut subject)?;
    assert_eq!(subject.body.target_center, [5., 5., -2.]);
    assert_eq!(subject.body.center, [0.; 3]);
    Ok(())
}

#[test]
fn actor_shadow_uses_sampled_bounds_and_root_height_with_native_radius_floor() -> Result<()> {
    let mut definition = (*definition()).clone();
    definition.skeleton.bones.push(Bone {
        name: "top".into(),
        parent: Some(0),
        bind_channels: TransformChannels(8),
        bind: Transform {
            translation: [8., 4., 7.],
            ..Default::default()
        },
    });
    definition.target_bones = vec![1, 2];
    definition.shadow = Some(ShadowDefinition {
        scale: 6.,
        color: [10, 20, 30, 40],
    });
    let mut subject = actor();
    let mut model = Model::new(Arc::new(definition), ActorId(0), &mut subject)?;
    for (height, radius) in [(0., 40.), (400., 20.), (600., 10.), (900., 10.)] {
        subject.position[1] = height;
        model.step(&mut subject, false, PlacementUpdate::Actor)?;
        assert_eq!(
            model.shown.shadow,
            Some(ActorShadowFrame {
                position: [5., 1.1, -2.],
                radius,
                color: [10, 20, 30, 40],
            })
        );
    }
    subject.position[1] = 0.;
    model.step(&mut subject, false, PlacementUpdate::Actor)?;
    subject.position[1] = 400.;
    model.sample_shadow(&subject);
    assert_eq!(model.shown.shadow.unwrap().radius, 20.);
    subject.position[1] = -900.;
    model.step(&mut subject, false, PlacementUpdate::Actor)?;
    assert_eq!(model.shown.shadow, None);
    Ok(())
}

#[test]
fn hurt_restarts_the_selected_clip_and_preserves_the_contact_frames_drawing_pose() {
    let mut definition = Arc::try_unwrap(definition()).unwrap();
    definition.hurt_motions = [Some(1), Some(0)];
    let mut actor = actor();
    let mut model = Model::new(Arc::new(definition), ActorId(0), &mut actor).unwrap();
    let held = model.shown.clone();
    model.hurt(false).unwrap();
    assert_eq!(model.shown, held);
    for weight in [0.2, 0.4, 0.6, 0.8] {
        model
            .step(&mut actor, true, PlacementUpdate::Actor)
            .unwrap();
        assert_eq!(model.shown.clip, 1);
        assert_eq!(model.shown.frame, 0.);
        assert_eq!(model.shown.blend_weight, weight);
    }
    model
        .step(&mut actor, true, PlacementUpdate::Actor)
        .unwrap();
    assert_eq!(model.shown.frame, 0.5);
    model.hurt(false).unwrap(); // A repeated contact restarts the same clip.
    model
        .step(&mut actor, true, PlacementUpdate::Actor)
        .unwrap();
    assert_eq!(model.shown.frame, 0.);
    assert_eq!(model.shown.blend_weight, 0.2);
    model.hurt(true).unwrap();
    model
        .step(&mut actor, true, PlacementUpdate::Actor)
        .unwrap();
    assert_eq!(model.shown.clip, 0);
}

#[test]
fn absent_reaction_slots_preserve_playback_and_declared_missing_clips_fail() {
    let mut definition = Arc::try_unwrap(definition()).unwrap();
    definition.initial.rate = 0.5;
    definition.hurt_motions = [None, Some(1)];
    definition.guard_motions = [Some(1), None];
    let mut actor = actor();
    let mut model = Model::new(Arc::new(definition.clone()), ActorId(0), &mut actor).unwrap();
    model.hurt(false).unwrap();
    model.guard(true).unwrap();
    model
        .step(&mut actor, true, PlacementUpdate::Actor)
        .unwrap();
    assert_eq!((model.shown.clip, model.shown.frame), (0, 1.));
    model.hurt(true).unwrap();
    model
        .step(&mut actor, true, PlacementUpdate::Actor)
        .unwrap();
    assert_eq!((model.shown.clip, model.shown.frame), (1, 0.));
    definition.guard_motions = [Some(99), None];
    assert!(Model::new(Arc::new(definition), ActorId(0), &mut actor).is_err());
}

#[test]
fn contact_motion_binding_matches_original_hurt_restarts_and_held_guard_clips() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/opening-contact-entry.json"
    ))
    .unwrap();
    for row in fixture["observations"].as_array().unwrap() {
        let before = &row["before"]["motion"];
        let after = &row["after"]["motion"];
        let number = |v: &serde_json::Value| f32::from_bits(v.as_u64().unwrap() as u32);
        let mut definition = Arc::try_unwrap(definition()).unwrap();
        definition.hurt_motions = [Some(3), Some(4)];
        definition.guard_motions = [Some(2), Some(4)];
        definition.motions = [2, 3, 4]
            .into_iter()
            .map(|clip| {
                (
                    clip,
                    Motion {
                        duration_frames: 3.,
                        tracks: vec![],
                    },
                )
            })
            .collect();
        let clip = before["clip"].as_u64().unwrap() as u16;
        definition.motions.insert(
            clip,
            Motion {
                duration_frames: number(&before["end_bits"]),
                tracks: vec![],
            },
        );
        definition.initial = Playback {
            clip,
            frame: number(&before["frame_bits"]),
            // This fixture resumes an observed drawing sample, rather than
            // entering at a pre-composition cursor from 52AA8.
            rate: 0.,
            repeat: false,
        };
        let mut actor = actor();
        let mut model = Model::new(Arc::new(definition), ActorId(0), &mut actor).unwrap();
        model.animation.clock.rate = number(&before["rate_bits"]);
        let held = model.shown.clone();
        if row["after"]["activity"] == 9 {
            model.hurt(row["flags"].as_u64().unwrap() & 2 != 0).unwrap();
        } else {
            model.guard(false).unwrap();
        }
        assert_eq!(model.clip, after["clip"].as_u64().unwrap() as u16);
        assert_eq!(
            model.animation.clock.frame.to_bits(),
            number(&after["frame_bits"]).to_bits()
        );
        assert_eq!(
            model.animation.clock.rate.to_bits(),
            number(&after["rate_bits"]).to_bits()
        );
        assert_eq!(
            model.animation.clock.blend,
            if after["replaced"] == true { 4 } else { 0 }
        );
        assert_eq!(model.shown, held);
    }
}

#[test]
fn body_weapon_and_drawn_pose_share_sampling_but_binding_does_not_change_held_pose() {
    let mut actor = actor();
    actor.position = [10., 20., 30.];
    actor.body.scale = 2.;
    let mut model = Model::new(definition(), ActorId(0), &mut actor).unwrap();
    assert_eq!(actor.body.points[0].center, [14., 26., 30.]);
    assert_eq!(actor.body.anchors[0], [22., 26., 30.]);
    let held = model.shown.clone();
    model
        .play(MotionBinding { model: 7, clip: 1 }, 4., 0.5, false, 4)
        .unwrap();
    assert_eq!(model.shown, held);
    for i in 1..=4 {
        model
            .step(&mut actor, true, PlacementUpdate::Actor)
            .unwrap();
        assert_eq!(model.shown.frame, 4.);
        assert_eq!(model.shown.blend_weight, i as f32 / 5.);
        assert!((actor.body.points[0].center[0] - (14. + 8. * i as f32 / 5.)).abs() < 0.00001);
    }
    model
        .step(&mut actor, true, PlacementUpdate::Actor)
        .unwrap();
    assert_eq!(model.shown.frame, 4.5);
    assert_eq!(actor.body.points[0].center, [23., 26., 30.]);
    assert_eq!(held.frame, 0.);
    assert_eq!(held.bones[1][3][0], 2.);
}

#[test]
fn replacement_during_blend_retains_last_unblended_source() {
    let mut actor = actor();
    let mut model = Model::new(definition(), ActorId(0), &mut actor).unwrap();
    let binding = MotionBinding { model: 7, clip: 1 };
    model.play(binding, 8., 0.5, false, 4).unwrap();
    model
        .step(&mut actor, true, PlacementUpdate::Actor)
        .unwrap();
    assert_eq!(model.shown.bones[1][3][0], 3.6);
    model.play(binding, 4., 0.5, false, 4).unwrap();
    model
        .step(&mut actor, true, PlacementUpdate::Actor)
        .unwrap();
    assert!((model.shown.bones[1][3][0] - 2.8).abs() < 0.000001);
}

#[test]
fn held_pose_recomposes_contacts_without_sampling_pending_bindings_or_finishing_a_blend() {
    let mut actor = actor();
    let mut model = Model::new(definition(), ActorId(0), &mut actor).unwrap();
    let binding = MotionBinding { model: 7, clip: 1 };
    model.play(binding, 8., 0.5, false, 4).unwrap();
    for _ in 0..3 {
        actor.position[0] += 10.;
        model
            .step(&mut actor, false, PlacementUpdate::Actor)
            .unwrap();
        assert_eq!((model.shown.clip, model.shown.frame), (0, 0.));
        assert_eq!(model.animation.clock.blend_age, 0);
        assert_eq!(actor.body.points[0].center[0], actor.position[0] + 2.);
        assert_eq!(actor.body.anchors[0][0], actor.position[0] + 6.);
    }
    for age in 1..=4 {
        model
            .step(&mut actor, true, PlacementUpdate::Actor)
            .unwrap();
        assert_eq!(model.shown.blend_weight, age as f32 / 5.);
        let held = model.shown.clone();
        for _ in 0..3 {
            model
                .step(&mut actor, false, PlacementUpdate::Actor)
                .unwrap();
            assert_eq!(model.shown, held);
            assert_eq!(model.animation.clock.blend_age, age);
        }
    }
    // At the final blend sample the native blending flag is already clear,
    // but a held visit must retain that blended pose and its original source.
    model.play(binding, 4., 0.5, false, 4).unwrap();
    model
        .step(&mut actor, false, PlacementUpdate::Actor)
        .unwrap();
    assert_eq!(model.shown.frame, 8.);
    assert_eq!(model.shown.blend_weight, 0.8);
    model
        .step(&mut actor, true, PlacementUpdate::Actor)
        .unwrap();
    assert!((model.shown.bones[1][3][0] - 2.8).abs() < 0.000001);
}

#[test]
fn later_animation_rows_hold_only_their_own_clock_on_dispatch() {
    // 2BC3C initializes at age0. 2C5B4 completes commands/hits before
    // 2B910 binds a later row; a declared blend1 still holds animation age.
    let source = r#"asset motion: battle::Motion = "test/motion";
        pub task run() {
            battle::action_guard(15, ticks(0), ticks(12));
            spawn commands();
            await battle::animate(motion, ticks(4), 0.0, 0.5, false);
            await battle::at_animation_age(ticks(2));
            await battle::animate(motion, ticks(2), 0.0, 0.5, false);
            await battle::at_animation_age(ticks(4));
            await battle::animate(motion, ticks(1), 0.0, 0.5, false);
            battle::end_animation();
            await battle::at_age(ticks(8));
            battle::finish();
        }
        task commands() {
            await battle::at_command_age(ticks(2));
            battle::heal_percent(battle::owner(), 1);
            await battle::at_command_age(ticks(5));
            battle::heal_percent(battle::owner(), 1);
        }"#;
    let p = Arc::try_unwrap(crate::tests::prepared(source, vec![actor()], 80)).unwrap();
    let mut actions = p.actions;
    actions[0].phase = ActionPhase::Actor;
    actions[0].tp_cost = 0;
    actions[0].resources = vec![ResourceBinding::Motion(MotionBinding { model: 7, clip: 1 })];
    let p = Arc::new(
        PreparedBattle::new(p.actors, actions, 1, vec![Some(definition())], vec![]).unwrap(),
    );
    let mut battle = Battle::new(p);
    let mut recoveries = vec![];
    for (update, expected) in [
        (0, (0, 0, 0)),
        (1, (0, 0, 0)),
        (2, (0, 0, 0)),
        (3, (0, 0, 0)),
        (4, (1, 1, 1)),
        (5, (2, 2, 2)),
        (6, (3, 3, 2)),
        (7, (3, 3, 2)),
        (8, (4, 4, 3)),
        (9, (5, 5, 4)),
        (10, (6, 6, 4)),
        (11, (7, 7, 4)),
        (12, (8, 8, 4)),
    ] {
        let frame = battle
            .step(BattleInput {
                actions: if update == 0 {
                    vec![ActionRequest {
                        actor: ActorId(0),
                        target: ActorId(0),
                        action: 99,
                    }]
                } else {
                    vec![]
                },
                ..Default::default()
            })
            .unwrap();
        let sequence = battle.sequences.values().next().unwrap();
        assert_eq!(
            (sequence.age, sequence.command_age, sequence.animation_age),
            expected,
            "update {update}"
        );
        assert_eq!(frame.actors[0].guard.enemy_chance, 15);
        assert!(matches!(
            frame.actors[0].activity,
            crate::Activity::Action {
                guard_window: [0, 12],
                ..
            }
        ));
        for cue in &frame.cues {
            if matches!(cue, crate::Cue::Recovered { .. }) {
                recoveries.push(update);
            }
        }
        if update == 6 {
            for _ in 0..3 {
                let paused = battle
                    .step(BattleInput {
                        menu_open: true,
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(paused.models, frame.models);
                assert_eq!(paused.actors, frame.actors);
                assert!(paused.cues.is_empty());
                let sequence = battle.sequences.values().next().unwrap();
                assert_eq!(
                    (sequence.age, sequence.command_age, sequence.animation_age),
                    expected
                );
            }
        }
    }
    assert_eq!(recoveries, [6, 10]);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert!(frame.actions.is_empty());
    assert!(
        frame
            .cues
            .iter()
            .any(|cue| matches!(cue, crate::Cue::Completed { .. }))
    );
}

#[test]
fn ordinary_animation_clocks_match_original_zombie_row_visits() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/opening-animation-streams.json"
    ))
    .unwrap();
    let source = r#"asset motion: battle::Motion = "test/motion";
        pub task run() {
            await battle::animate(motion, ticks(4), 0.0, 0.5, false);
            await battle::at_animation_age(ticks(19));
            await battle::animate(motion, ticks(2), 0.0, 0.5, false);
            await battle::at_animation_age(ticks(39));
            await battle::animate(motion, ticks(2), 0.0, 0.5, false);
            battle::end_animation();
            await battle::action_end();
        }"#;
    let p = Arc::try_unwrap(crate::tests::prepared(source, vec![actor()], 80)).unwrap();
    let mut actions = p.actions;
    actions[0].phase = ActionPhase::Actor;
    actions[0].tp_cost = 0;
    actions[0].resources = vec![ResourceBinding::Motion(MotionBinding { model: 7, clip: 1 })];
    let mut battle = Battle::new(Arc::new(
        PreparedBattle::new(p.actors, actions, 1, vec![Some(definition())], vec![]).unwrap(),
    ));
    let initial = fixture["initial_tick"].as_u64().unwrap();
    for update in 0..=75 {
        battle
            .step(BattleInput {
                actions: if update == 0 {
                    vec![ActionRequest {
                        actor: ActorId(0),
                        target: ActorId(0),
                        action: 99,
                    }]
                } else {
                    vec![]
                },
                ..Default::default()
            })
            .unwrap();
        if let Some(row) = fixture["observations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["tick"].as_u64() == Some(initial + update))
        {
            let after = row["after"].as_array().unwrap();
            let sequence = battle.sequences.values().next().unwrap();
            assert_eq!(sequence.age, after[0].as_u64().unwrap() as u32 + 1, "{row}");
            assert_eq!(
                sequence.command_age,
                after[1].as_i64().unwrap() as i16,
                "{row}"
            );
            assert_eq!(
                sequence.animation_age,
                after[3].as_i64().unwrap() as i16,
                "{row}"
            );
        }
    }
}

#[test]
fn initial_blend_defers_zero_commands_and_completion_follows_contact_stream() {
    for (boundary, duration) in [("action_end()", 3), ("action_end_at(ticks(3))", 4)] {
        let source = r#"asset motion: battle::Motion = "test/motion";
        pub task run() {
            spawn commands();
            spawn hits();
            await battle::animate(motion, ticks(4), 0.0, 0.5, false);
            battle::end_animation();
            await battle::action_end();
            battle::heal_percent(battle::owner(), 20);
            await battle::recover(ticks(2));
            battle::finish();
        }
        task commands() {
            battle::heal_percent(battle::owner(), 1);
            await battle::at_command_age(ticks(3));
            battle::heal_percent(battle::owner(), 2);
        }
        task hits() {
            await battle::at_hit_age(ticks(3));
            battle::heal_percent(battle::owner(), 3);
        }"#;
        let source = source.replace("action_end()", boundary);
        let p = Arc::try_unwrap(crate::tests::prepared(&source, vec![actor()], duration)).unwrap();
        let mut actions = p.actions;
        actions[0].phase = ActionPhase::Actor;
        actions[0].tp_cost = 0;
        actions[0].resources = vec![ResourceBinding::Motion(MotionBinding { model: 7, clip: 1 })];
        let mut battle = Battle::new(Arc::new(
            PreparedBattle::new(p.actors, actions, 1, vec![Some(definition())], vec![]).unwrap(),
        ));
        let mut observed = vec![];
        for update in 0..12 {
            let frame = battle
                .step(BattleInput {
                    actions: if update == 0 {
                        vec![ActionRequest {
                            actor: ActorId(0),
                            target: ActorId(0),
                            action: 99,
                        }]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                })
                .unwrap();
            for cue in &frame.cues {
                if let crate::Cue::Recovered { nominal, .. } = cue {
                    observed.push((update, *nominal));
                }
            }
            if update < 4 {
                assert_eq!(frame.actors[0].hp, 50);
            }
            if update == 7 {
                assert_eq!(frame.actors[0].activity, crate::Activity::Recovering);
                assert_eq!(frame.actions[0].2, 3);
                assert_eq!(battle.sequences.values().next().unwrap().hit_age, 4);
            }
        }
        assert_eq!(observed, [(4, 1), (7, 2), (7, 3), (7, 20)]);
        assert!(matches!(battle.actors()[0].activity, crate::Activity::Idle));
    }
}

#[test]
fn scripted_animation_waits_hold_actor_clock_pause_and_cancel_cleanly() {
    let source = r#"asset swing: battle::Motion = "test/swing";
        pub task run() {
            await battle::animate(swing, ticks(4), 1.0, 0.5, false);
            await battle::animation_end();
            battle::heal_percent(battle::owner(), 10);
            battle::finish();
        }"#;
    let p = Arc::try_unwrap(crate::tests::prepared(source, vec![actor()], 80)).unwrap();
    let mut actions = p.actions;
    actions[0].phase = ActionPhase::Actor;
    actions[0].tp_cost = 0;
    actions[0].resources = vec![ResourceBinding::Motion(MotionBinding { model: 7, clip: 1 })];
    let p = Arc::new(
        PreparedBattle::new(
            p.actors,
            actions,
            1,
            vec![Some(definition())],
            p.effects.into_values().collect(),
        )
        .unwrap(),
    );
    let id = p.actor_ids().next().unwrap();
    let mut battle = Battle::new(p);
    battle.actors[0].movement.direction = [1., 0., 0.];
    battle.actors[0].movement.forward = 2.;
    let initial_x = battle.actors[0].position[0];
    let frame = battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: id,
                target: id,
                action: 99,
            }],
            ..Default::default()
        })
        .unwrap();
    let action = frame.actions[0].0;
    assert_eq!(frame.models[0].clip, 0);
    assert_eq!(battle.action_age(action), Some(0));
    let paused = battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(frame.models, paused.models);
    for visit in 0..3 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.action_age(action), Some(0));
        assert_eq!(
            frame.actors[0].position[0],
            initial_x + (visit + 2) as f32 * 2.
        );
        assert_eq!(
            frame.models[0].world[3][0],
            initial_x + (visit + 1) as f32 * 2.
        );
    }
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.action_age(action), Some(1));
    for _ in 0..18 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(
        battle.actors()[0].hp,
        50,
        "equality at clip end is not completion"
    );
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.actors()[0].hp, 60);
    let started = battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: id,
                target: id,
                action: 99,
            }],
            ..Default::default()
        })
        .unwrap();
    let action = started.actions[0].0;
    battle
        .step(BattleInput {
            interrupt: vec![action],
            ..Default::default()
        })
        .unwrap();
    for _ in 0..30 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(
        battle.actors()[0].hp,
        60,
        "cancelled animation wait cannot resume"
    );
}

#[test]
fn invalid_models_fail_preparation_before_activation() {
    let mut invalid = (*definition()).clone();
    invalid.anchors[0].bone = 2;
    assert!(
        PreparedBattle::new(
            vec![actor()],
            vec![],
            1,
            vec![Some(Arc::new(invalid))],
            vec![]
        )
        .is_err()
    );
    let mut invalid = (*definition()).clone();
    invalid.initial.frame = f32::NAN;
    assert!(
        PreparedBattle::new(
            vec![actor()],
            vec![],
            1,
            vec![Some(Arc::new(invalid))],
            vec![]
        )
        .is_err()
    );
}

#[test]
fn melee_contacts_use_the_current_sampled_weapon_position() {
    let source = r#"asset motion: battle::Motion = "test/motion";
        asset hit: battle::Melee = "test/hit";
        pub task run() {
            await battle::animate(motion, ticks(0), 0.0, 1.0, false);
            await battle::hit_window(hit, ticks(8), ticks(1));
        }"#;
    let mut enemy = crate::tests::actor(Side::Enemy);
    enemy.body.points.push(crate::HurtPoint {
        center: [14., 3., 0.],
        radius: 0.1,
    });
    let p = Arc::try_unwrap(crate::tests::prepared(source, vec![actor(), enemy], 30)).unwrap();
    let mut actions = p.actions;
    actions[0].phase = ActionPhase::Actor;
    actions[0].resources = vec![
        ResourceBinding::Motion(MotionBinding { model: 7, clip: 1 }),
        ResourceBinding::Melee(Arc::new(crate::MeleeDefinition {
            hit: crate::HitRule {
                impact: None,
                arte: false,
                reaction: Default::default(),
                kind: crate::DamageKind::Slash,
                power: crate::Power::Fixed(5),
                element: crate::HitElement::Neutral,
                prevents_defeat: false,
                guard: Default::default(),
            },
            cooldown: 1,
            radius: 0.1,
            height: 0.1,
            shape: crate::HitShape::Sphere,
            anchors: vec![0],
            trail: None,
        })),
    ];
    let p = Arc::new(
        PreparedBattle::new(p.actors, actions, 1, vec![Some(definition()), None], vec![]).unwrap(),
    );
    let mut battle = Battle::new(p);
    let first = battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(0),
                target: ActorId(1),
                action: 99,
            }],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first.actors[0].body.anchors[0], [6., 3., 0.]);
    for _ in 1..8 {
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.actors()[1].hp, 50);
    }
    let hit = battle.step(BattleInput::default()).unwrap();
    assert_eq!(hit.models[0].frame, 8.);
    assert_eq!(hit.actors[0].body.anchors[0], [14., 3., 0.]);
    assert_eq!(hit.actors[1].hp, 45);
}

#[test]
fn maintained_normal_attacks_run_motion_movement_hits_and_recovery_together() {
    use crate::{Activity, DamageKind, GuardRule, HitRule, HitShape, MeleeDefinition, Power};
    #[derive(serde::Deserialize)]
    struct Request {
        kind: String,
        index: u16,
        priority: u8,
        mode: Option<u8>,
    }
    #[derive(serde::Deserialize)]
    struct Observation {
        clip: u16,
        clock: u32,
        requests: Vec<Request>,
    }
    #[derive(serde::Deserialize)]
    struct Fixture {
        observations: Vec<Observation>,
    }
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../tests/fixtures/lloyd-action-sounds.json"
    ))
    .expect("original sound request fixture");
    for (entry, duration, recovery, hit_updates) in [
        ("neutral", 30, 10, vec![12]),
        ("finisher", 40, 8, vec![14, 22]),
    ] {
        let source = include_str!("../../../../scripts/battle/normal_lloyd.sym")
            .replace("script battle;", "")
            .replace("use battle;", "")
            + &format!("\npub task run() {{ await {entry}(); }}");
        let mut target = crate::tests::actor(Side::Enemy);
        target.body.points.push(crate::HurtPoint {
            center: [0.; 3],
            radius: 1.,
        });
        let p = Arc::try_unwrap(crate::tests::prepared(
            &source,
            vec![actor(), target],
            duration,
        ))
        .unwrap();
        let mut actions = p.actions;
        actions[0].phase = ActionPhase::Actor;
        actions[0].tp_cost = 0;
        let mut model = (*definition()).clone();
        model.anchors = vec![model.anchors[0]; 4];
        model.motions.insert(30, model.motions[&1].clone());
        model.motions.insert(31, model.motions[&1].clone());
        let melee = |anchors| {
            ResourceBinding::Melee(Arc::new(MeleeDefinition {
                hit: HitRule {
                    impact: None,
                    arte: false,
                    reaction: Default::default(),
                    kind: DamageKind::Slash,
                    power: Power::Fixed(5),
                    element: crate::HitElement::Neutral,
                    prevents_defeat: false,
                    guard: GuardRule::default(),
                },
                cooldown: 2,
                radius: 100.,
                height: 100.,
                shape: HitShape::Box,
                anchors,
                trail: None,
            }))
        };
        actions[0].resources = vec![
            melee(vec![0, 1]),
            melee(vec![2, 3]),
            ResourceBinding::Motion(MotionBinding { model: 7, clip: 30 }),
            ResourceBinding::Motion(MotionBinding { model: 7, clip: 31 }),
            ResourceBinding::Sound(crate::SoundBinding {
                resource: 1,
                index: 60,
            }),
            ResourceBinding::Voice(vec![
                Some(crate::VoiceLine {
                    sound: crate::SoundBinding {
                        resource: 1,
                        index: 502,
                    },
                    duration: 0,
                }),
                None,
            ]),
            ResourceBinding::Voice(vec![
                Some(crate::VoiceLine {
                    sound: crate::SoundBinding {
                        resource: 1,
                        index: 504,
                    },
                    duration: 0,
                }),
                None,
            ]),
        ];
        let mut actors = p.actors;
        actors[0].movement.direction = [1., 0., 0.];
        actors[0].movement.braking = 0.41250002;
        let p = Arc::new(
            PreparedBattle::new(
                actors,
                actions,
                1,
                vec![Some(Arc::new(model)), None],
                vec![],
            )
            .unwrap(),
        );
        let mut battle = Battle::new(p);
        let mut hits = vec![];
        let mut sounds = vec![];
        let mut sound_commands = vec![];
        let mut voice_commands = vec![];
        let end = 4 + duration + recovery + 1;
        for update in 0..=end {
            let input = if update == 0 {
                BattleInput {
                    actions: vec![ActionRequest {
                        actor: ActorId(0),
                        target: ActorId(1),
                        action: 99,
                    }],
                    ..Default::default()
                }
            } else {
                BattleInput::default()
            };
            let age = battle.sequences.values().next().map_or(0, |s| s.age);
            let frame = battle.step(input).unwrap();
            for cue in &frame.cues {
                if let crate::Cue::Sound {
                    actor,
                    sound,
                    priority,
                    ..
                } = cue
                {
                    assert_eq!(*actor, ActorId(0));
                    assert_eq!(sound.index, 60);
                    assert_eq!(*priority, 1);
                    sounds.push(update);
                    sound_commands.push((age, sound.index, *priority));
                }
                if let crate::Cue::Voice { actor, sound, .. } = cue {
                    assert_eq!(*actor, ActorId(0));
                    assert_eq!(update, 8);
                    let voice = &battle.voices[actor.index()];
                    assert_eq!(voice.mode, 2);
                    voice_commands.push((age, sound.index - 501, voice.priority));
                }
            }
            if update < 8 {
                assert_eq!(frame.actors[0].position[0], 0.);
            }
            if update == 8 {
                assert_eq!(frame.actors[0].position[0], 6.);
            }
            if frame.cues.iter().any(|c| {
                matches!(
                    c,
                    crate::Cue::Hit {
                        actor: ActorId(1),
                        ..
                    }
                )
            }) {
                hits.push(update);
            }
            assert_eq!(
                frame.actions.is_empty(),
                update == end,
                "{entry} at {update}"
            );
            if update >= 4 + duration && update < end {
                assert_eq!(frame.actors[0].activity, Activity::Recovering);
                assert_eq!(frame.actions[0].2, u32::from(duration));
            }
        }
        assert_eq!(hits, hit_updates, "{entry}");
        let clip = if entry == "neutral" { 30 } else { 31 };
        let mut original: Vec<_> = fixture
            .observations
            .iter()
            .filter(|row| row.clip == clip)
            .flat_map(|row| {
                row.requests
                    .iter()
                    .filter(|request| request.kind == "sound")
                    .map(|request| (row.clock, request.index, request.priority))
            })
            .collect();
        // Four independently observed attacks retain the same request sequence.
        assert_eq!(original.len(), sound_commands.len() * 4);
        original.sort();
        original.dedup();
        assert_eq!(sound_commands, original);
        let mut original: Vec<_> = fixture
            .observations
            .iter()
            .filter(|row| row.clip == clip)
            .flat_map(|row| {
                row.requests
                    .iter()
                    .filter(|request| request.kind == "voice")
                    .map(|request| {
                        assert_eq!(request.mode, Some(2));
                        (row.clock, request.index, request.priority)
                    })
            })
            .collect();
        assert_eq!(original.len(), voice_commands.len() * 4);
        original.sort();
        original.dedup();
        assert_eq!(voice_commands, original);
        assert_eq!(
            sounds,
            if entry == "neutral" {
                vec![12]
            } else {
                vec![12, 16]
            }
        );
        assert_eq!(battle.actors()[0].activity, Activity::Idle);
    }
}

#[test]
fn recovery_completes_while_model_blending_and_local_hit_stop_continue() {
    let source = "pub task run() { await battle::recover(ticks(2)); battle::finish(); }";
    let p = Arc::try_unwrap(crate::tests::prepared(source, vec![actor()], 0)).unwrap();
    let mut actions = p.actions;
    actions[0].phase = ActionPhase::Actor;
    let p = Arc::new(
        PreparedBattle::new(
            p.actors,
            actions,
            1,
            vec![Some(definition())],
            p.effects.into_values().collect(),
        )
        .unwrap(),
    );
    let mut battle = Battle::new(p);
    battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(0),
                target: ActorId(0),
                action: 99,
            }],
            ..Default::default()
        })
        .unwrap();
    battle.models[0]
        .as_mut()
        .unwrap()
        .play(MotionBinding { model: 7, clip: 1 }, 0., 1., false, 8)
        .unwrap();
    battle.actors[0].hit_stop = 5;
    for _ in 0..2 {
        assert!(
            !battle
                .step(BattleInput::default())
                .unwrap()
                .actions
                .is_empty()
        );
    }
    let end = battle.step(BattleInput::default()).unwrap();
    assert!(end.actions.is_empty());
    assert_eq!(end.actors[0].hit_stop, 2);
    assert_eq!(end.models[0].blend_weight, 3. / 9.);
}

#[test]
fn effect_model_rebinding_during_a_hold_keeps_the_sampled_pose() -> Result<()> {
    let source = definition();
    let mut model = PreparedEffectModel::new(Arc::new(EffectModelDefinition {
        resource: source.resource,
        skeleton: source.skeleton.clone(),
        motions: source.motions.clone(),
        secondary_motion: vec![],
    }))?;
    let source: serde_json::Value = serde_json::from_str(include_str!(
        "../../../game/tests/fixtures/nurse-model-particles.json"
    ))?;
    let bank: resonance_content::battle_effect::SourceBank =
        serde_json::from_value(source["bank"].clone())?;
    let mut particle = crate::ParticleFrame {
        id: crate::ParticleId(1),
        owner: ActorId(0),
        draw_after: None,
        resource: 237,
        member: 0,
        origin: [0.; 3],
        heading: 0.,
        age: 0,
        state: bank.particle(0)?.state,
        model: None,
    };
    let mut static_model = PreparedEffectModel::new(Arc::new(EffectModelDefinition {
        resource: model.resource(),
        skeleton: definition().skeleton.clone(),
        motions: Default::default(),
        secondary_motion: vec![],
    }))?;
    let static_frame = static_model.step(&particle, true)?;
    assert_eq!(static_frame.clip, None);
    assert_eq!(
        static_frame.bones.as_ref(),
        &definition().skeleton.bind_pose()?.global
    );
    model.play(0)?;
    let first = model.step(&particle, true)?;
    model.play(1)?;
    particle.origin[0] = 20.;
    let held = model.step(&particle, false)?;
    assert_eq!((held.clip, held.frame), (first.clip, first.frame));
    assert_eq!(held.bones, first.bones);
    assert_ne!(held.world, first.world);
    let next = model.step(&particle, true)?;
    assert_eq!((next.clip, next.frame), (Some(1), 0.5));
    Ok(())
}

#[test]
fn result_construction_hides_enemies_without_waiting_for_death_fade() -> Result<()> {
    for (availability, hp) in [
        (crate::ActorAvailability::Dead, 0),
        (crate::ActorAvailability::Petrified, 50),
    ] {
        let party = actor();
        let mut enemy = actor();
        enemy.side = Side::Enemy;
        enemy.availability = availability;
        enemy.hp = hp;
        enemy.petrified = availability == crate::ActorAvailability::Petrified;
        let prepared = PreparedBattle::new(
            vec![party, enemy],
            vec![],
            1,
            vec![Some(definition()), Some(definition())],
            vec![],
        )?;
        let mut live = Battle::new(Arc::new(prepared));
        assert!(live.hide_result_enemies().is_err());
        assert!(live.step(BattleInput::default())?.models[1].visible);
        assert_eq!(live.recognize_result(), Some(crate::BattleResult::Victory));
        assert_eq!(live.phase(), crate::BattlePhase::Ending);
        assert!(live.hide_result_enemies().is_err());
        live.retire_combat()?;
        live.actors[0].body.jitter.request(8);
        live.actors[0].body.jitter.advance(&mut live.random);
        assert!(live.actors[0].body.jitter.active);
        let mut pending_jitter = live.actors[0].body.jitter;
        assert_eq!(pending_jitter.take_acceleration()[1], 0.2);
        let random = live.random_state();
        live.reset_result_actor(ActorId(0), true)?;
        assert!(!live.actors[0].body.jitter.active);
        assert_eq!(live.actors[0].body.jitter.remaining, 7);
        assert_eq!(live.actors[0].body.jitter.take_acceleration(), [0.; 3]);
        assert_eq!(live.random_state(), random);
        assert!(live.snapshot().models[1].visible);
        live.hide_result_enemies()?;
        let shown = live.step(BattleInput::default())?;
        assert!(shown.models[0].visible);
        assert!(!shown.models[1].visible);
        assert_eq!(shown.actors[1].availability, crate::ActorAvailability::Dead);
        assert_eq!(shown.actors[1].hp, hp);
        live.actors_visible = false;
        assert!(
            live.step(BattleInput::default())?
                .models
                .iter()
                .all(|model| !model.visible)
        );
        live.actors_visible = true;
        assert!(!live.step(BattleInput::default())?.models[1].visible);
    }
    Ok(())
}

#[test]
fn idle_pose_uses_optional_injured_clip_with_original_party_and_timer_gates() -> Result<()> {
    let mut prepared_model = (*definition()).clone();
    prepared_model
        .motions
        .insert(26, prepared_model.motions[&0].clone());
    prepared_model.idle_motions = [Some(0), Some(26)];
    let mut subject = actor();
    subject.hp = 24;
    subject.control = crate::Control::Auto;
    let prepared = PreparedBattle::new(
        vec![subject],
        vec![],
        0,
        vec![Some(Arc::new(prepared_model))],
        vec![],
    )?;
    let mut battle = Battle::new(Arc::new(prepared));
    battle.play_idle_pose(ActorId(0), 8)?;
    let model = battle.models[0].as_mut().unwrap();
    assert_eq!((model.clip, model.animation.clock.blend), (26, 12));
    model.animation.clock.frame = 6.;
    battle.actors[0].control = crate::Control::SemiAuto;
    battle.idle_timers[0] = 30;
    battle.play_idle_pose(ActorId(0), 8)?;
    assert_eq!(
        battle.models[0].as_ref().unwrap().animation.clock.frame,
        6.,
        "an already active26 is retained even while the manual delay is positive"
    );
    battle.actors[0].hp = 25;
    battle.play_idle_pose(ActorId(0), 8)?;
    assert_eq!(
        battle.models[0].as_ref().unwrap().clip,
        0,
        "25 percent selects normal idle"
    );
    battle.models[0].as_mut().unwrap().animation.clock.frame = 3.;
    battle.play_idle_pose(ActorId(0), 8)?;
    assert_eq!(
        battle.models[0].as_ref().unwrap().animation.clock.frame,
        3.,
        "2C05C enabled0 preserves the same normal idle clip"
    );
    battle.actors[0].hp = 24;
    battle.play_idle_pose(ActorId(0), 8)?;
    assert_eq!(
        battle.models[0].as_ref().unwrap().clip,
        0,
        "Manual/Semi wait for timer zero"
    );
    battle.idle_timers[0] = 0;
    battle.play_idle_pose(ActorId(0), 8)?;
    assert_eq!(battle.models[0].as_ref().unwrap().clip, 26);
    battle.actors[0].side = Side::Enemy;
    battle.play_idle_pose(ActorId(0), 8)?;
    assert_eq!(
        battle.models[0].as_ref().unwrap().clip,
        0,
        "enemy side has no injured-idle branch"
    );
    Ok(())
}

#[test]
fn ordinary_reset_restores_expression_baseline_only_for_prepared_channels() -> Result<()> {
    for expression in [None, Some([3, 4, 0, 0])] {
        let prepared =
            PreparedBattle::new(vec![actor()], vec![], 0, vec![Some(definition())], vec![])?
                .with_idle_expressions(vec![expression])?;
        let mut battle = Battle::new(Arc::new(prepared));
        battle.eye_expressions[0] = 9;
        battle.models[0].as_mut().unwrap().shown.texture_layers = [9; 4];
        battle.reset_ordinary_actor(ActorId(0));
        let expected = expression.unwrap_or([9; 4]);
        assert_eq!(battle.eye_expressions[0], expected[0]);
        assert_eq!(
            battle.models[0].as_ref().unwrap().shown.texture_layers,
            expected
        );
    }
    Ok(())
}

fn flash_battle() -> Battle {
    let mut body = (*definition()).clone();
    body.weapons.push(Arc::new(WeaponDefinition {
        slot: 0,
        resource: 8,
        attachment: 1,
        skeleton: body.skeleton.clone(),
        motions: BTreeMap::new(),
        playback: WeaponPlayback::Rigid,
        anchors: vec![],
        links: vec![],
    }));
    let p = Arc::try_unwrap(crate::tests::prepared(
        "pub task run() { await battle::next_update(); }",
        vec![actor(), crate::tests::actor(Side::Enemy)],
        20,
    ))
    .unwrap();
    let mut actions = p.actions;
    actions[0].phase = ActionPhase::Actor;
    actions[0].tp_cost = 0;
    let mut prepared = PreparedBattle::new(
        p.actors,
        actions,
        1,
        vec![Some(Arc::new(body)), None],
        vec![],
    )
    .unwrap()
    .with_admission_flashes(BTreeMap::from([(99, [40, 40, 112])]))
    .unwrap();
    prepared.actors[0].body.tint = [64, 64, 64, 173];
    Battle::new(Arc::new(prepared))
}

#[test]
fn contact_flash_is_retained_with_body_and_weapon_until_next_model_visit() -> Result<()> {
    let mut battle = flash_battle();
    let base = [64, 64, 64, 173];
    let prior = battle.step(BattleInput::default())?;
    assert_eq!(prior.models[0].tint, base);
    // 3B370 runs after this visit's models, as at source C150.
    battle.contact_feedback[0].flash([192, 128, 128]);
    let contact = battle.frame(vec![], None);
    assert_eq!(contact.models[0].tint, base);
    assert_eq!(contact.weapons[0].tint, base);
    for expected in [[192, 128, 128, 173], [192, 128, 128, 173], base] {
        let frame = battle.step(BattleInput::default())?;
        assert_eq!(frame.models[0].tint, expected);
        assert_eq!(frame.weapons[0].tint, expected);
        assert_eq!(frame.actors[0].body.tint, base);
    }
    Ok(())
}

#[test]
fn enabled_action_admission_flashes_next_model_once_and_rejections_do_not_reset_it() -> Result<()> {
    let mut battle = flash_battle();
    let request = ActionRequest {
        actor: ActorId(0),
        target: ActorId(1),
        action: 99,
    };
    let admitted = battle.step(BattleInput {
        actions: vec![request],
        ..Default::default()
    })?;
    assert_eq!(admitted.models[0].tint, [64, 64, 64, 173]);
    // Successful admission precedes common decrement, unlike contact feedback.
    let shown = battle.step(BattleInput {
        actions: vec![request],
        ..Default::default()
    })?;
    assert_eq!(shown.models[0].tint, [40, 40, 112, 173]);
    assert_eq!(shown.weapons[0].tint, shown.models[0].tint);
    assert!(shown.cues.iter().any(|cue| matches!(
        cue,
        crate::Cue::Rejected {
            reason: crate::Rejection::Busy,
            ..
        }
    )));
    let next = battle.step(BattleInput::default())?;
    assert_eq!(next.models[0].tint, [64, 64, 64, 173]);
    assert_eq!(next.weapons[0].tint, next.models[0].tint);
    Ok(())
}

#[test]
fn held_global_flash_uses_stage_rgb_and_preserves_alpha() {
    let mut state = crate::contact_feedback::State::default();
    state.flash([192, 64, 64]);
    let mut tint = [60, 61, 62, 173];
    state.appearance(&mut tint, Some([45, 46, 47]));
    assert_eq!(tint, [45, 46, 47, 173]);
    state.appearance(&mut tint, None);
    assert_eq!(tint, [192, 64, 64, 173]);
}

#[test]
fn command_model_visits_hold_pose_and_flash_clock_but_consume_queued_acceleration() -> Result<()> {
    // Original command v4 VI1833->1834 clears only Genis body acceleration;
    // watched tracks/placement/chains hold through close1941, then resume1942.
    let mut battle = flash_battle();
    Arc::get_mut(&mut battle.prepared).unwrap().ambient_color = [45, 46, 47];
    let before = battle.step(BattleInput::default())?;
    let elapsed = battle.ledger().elapsed_ticks;
    let combat = battle.ledger().combat_ticks;
    let random = battle.random_state();
    battle.actors[0].position[0] += 10.;
    battle.actors[0].heading += 35.;
    battle.models[0].as_mut().unwrap().play(
        MotionBinding { model: 7, clip: 1 },
        8.,
        0.5,
        false,
        4,
    )?;
    battle.actors[0].body.jitter.request(8);
    battle.actors[0]
        .body
        .jitter
        .advance(&mut crate::Random::from_state(1));
    battle.contact_feedback[0].flash([192, 128, 128]);
    for visit in 1..=108 {
        let held = battle.step(BattleInput {
            command_pause: true,
            ..Default::default()
        })?;
        assert_eq!(held.update, before.update + visit);
        assert!(held.hud_holds.intro);
        assert_eq!(battle.ledger().elapsed_ticks, elapsed);
        assert_eq!(battle.ledger().combat_ticks, combat);
        assert_eq!(battle.random_state(), random);
        assert_eq!(
            (held.models[0].clip, held.models[0].frame),
            (before.models[0].clip, before.models[0].frame)
        );
        assert_eq!(held.models[0].blend_weight, before.models[0].blend_weight);
        assert_eq!(held.models[0].world, before.models[0].world);
        assert_eq!(held.models[0].bones, before.models[0].bones);
        assert_eq!(held.weapons[0].world, before.weapons[0].world);
        assert_eq!(held.weapons[0].bones, before.weapons[0].bones);
        assert_eq!(held.models[0].tint, [45, 46, 47, 173]);
        assert_eq!(held.weapons[0].tint, held.models[0].tint);
        assert_eq!(battle.actors[0].body.jitter.take_acceleration(), [0.; 3]);
        assert_eq!(battle.actors[0].body.jitter.remaining, 7);
        assert!(battle.actors[0].body.jitter.active);
    }
    let resumed = battle.step(BattleInput::default())?;
    assert_eq!(resumed.models[0].clip, 1);
    assert_ne!(resumed.models[0].world, before.models[0].world);
    assert_eq!(resumed.models[0].tint, [192, 128, 128, 173]);
    assert_eq!(resumed.weapons[0].tint, resumed.models[0].tint);
    assert_eq!(battle.ledger().elapsed_ticks, elapsed + 1);
    assert_eq!(battle.ledger().combat_ticks, combat + 1);
    assert_eq!(
        battle.step(BattleInput::default())?.models[0].tint,
        [192, 128, 128, 173]
    );
    assert_eq!(
        battle.step(BattleInput::default())?.models[0].tint,
        // The ordinary common visit has approached the lower stage RGB once
        // before the pause and twice after it; the flash did not change it.
        [61, 61, 61, 173]
    );
    Ok(())
}

#[test]
fn target_pointer_follows_current_body_before_the_next_draw() -> Result<()> {
    // Original marker watch01: every C1..245 follow uses the current145C
    // body, while51038 draws the previous follow state. Entry's origin-only
    // P0 placement must not create a spurious long-distance pointer trail.
    for entry_origin in [false, true] {
        let mut definition = (*definition()).clone();
        definition.initial.rate = 1.;
        definition.target_marker = Some(Anchor {
            bone: 1,
            offset: [0., 40., 0.],
        });
        let mut party = crate::tests::actor(Side::Party);
        party.position[0] = -300.;
        let mut enemy = actor();
        enemy.side = Side::Enemy;
        enemy.position[0] = 300.;
        enemy.body.center_offset = [0., 21.5, 0.];
        let mut prepared = PreparedBattle::new(
            vec![party, enemy],
            vec![],
            1,
            vec![None, Some(Arc::new(definition))],
            vec![],
        )?;
        if entry_origin {
            prepared.models[1]
                .as_mut()
                .unwrap()
                .initialize_entry_placement(&mut prepared.actors[1])?;
        }
        let mut battle = Battle::new(Arc::new(prepared));
        let mut previous = [300., 43., 0.];
        for visit in 1..=4 {
            let drawn = battle.step(BattleInput::default())?;
            let marker = drawn.target_markers[0];
            let followed = battle.target_marker_frames()[0];
            // The two-bone fixture advances x by one per visit; -90 X
            // placement maps its local z=3 to world y=3, plus profile40.
            let current = [303. + visit as f32, 43., 0.];
            for axis in 0..3 {
                assert!((marker.position[axis] - previous[axis]).abs() < 0.0001);
                assert!((followed.position[axis] - current[axis]).abs() < 0.0001);
            }
            assert_eq!(marker.direction, [0.; 3]);
            assert_eq!(followed.direction, [0.; 3]);
            assert_eq!((marker.trail, followed.trail), (0, 0));
            assert_eq!(marker.phase, visit);
            assert_eq!(battle.snapshot().target_markers, drawn.target_markers);
            previous = current;
        }
    }
    Ok(())
}

#[test]
fn enemy_zero_chance_followup_waits_for_ready_normal_callback() -> Result<()> {
    let source = r#"asset motion: battle::Motion = "test/motion";
        pub task run() {
            await battle::animate(motion, ticks(4), 0.0, 0.5, false);
            battle::end_animation();
            await battle::at_age(ticks(30));
        }"#;
    let mut owner = actor();
    owner.side = Side::Enemy;
    owner.control = crate::Control::Enemy;
    owner.movement.turning_disabled = true;
    let p = Arc::try_unwrap(crate::tests::prepared(source, vec![owner], 30)).unwrap();
    let mut actions = p.actions;
    actions[0].phase = ActionPhase::Actor;
    actions[0].tp_cost = 0;
    actions[0].resources = vec![ResourceBinding::Motion(MotionBinding { model: 7, clip: 1 })];
    let seed = 0x1234_5678;
    let prepared = PreparedBattle::new(p.actors, actions, seed, vec![Some(definition())], vec![])?
        .with_enemy_decisions(vec![crate::EnemyDecisionDefinition {
            actor: ActorId(0),
            strategy: 1,
            difficulty: 0,
            choices: vec![crate::EnemyChoice {
                action: 99,
                weight: 1,
                requirements: 0,
                target_policy: 1,
                guard_chance: 0,
                combo_at: 0,
                followup_chance: 0,
                range: [0, 0],
                tp: 0,
                approach_minimum: 0.,
                approach_range: 80.,
            }],
            back_row: vec![],
            walk_speed: 1.3,
            turn_ticks: 8,
            body_flags: 0,
        }])?;
    let mut battle = Battle::new(Arc::new(prepared));
    battle.step(BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            target: ActorId(0),
            action: 99,
        }],
        ..Default::default()
    })?;
    assert_eq!(
        battle.random_state(),
        seed,
        "3DF34 initializer does not roll"
    );
    assert_eq!(battle.sequences.values().next().unwrap().age, 0);
    battle.step(BattleInput {
        menu_open: true,
        ..Default::default()
    })?;
    assert_eq!(battle.random_state(), seed);
    for _ in 0..3 {
        battle.step(BattleInput::default())?;
        assert_eq!(battle.random_state(), seed, "blend holds636A8");
        assert_eq!(battle.sequences.values().next().unwrap().age, 0);
    }
    battle.actors[0].hit_stop = 2;
    for _ in 0..2 {
        battle.step(BattleInput::default())?;
        assert_eq!(battle.random_state(), seed, "local hit-stop holds636A8");
        assert_eq!(battle.sequences.values().next().unwrap().age, 0);
    }
    let mut expected = crate::state::Random(seed);
    expected.next();
    battle.step(BattleInput::default())?;
    assert_eq!(battle.sequences.values().next().unwrap().age, 1);
    assert_eq!(
        battle.random_state(),
        expected.0,
        "zero chance still rolls once"
    );
    battle.step(BattleInput::default())?;
    assert_eq!(
        battle.random_state(),
        expected.0,
        "later callback is outside combo_at"
    );
    Ok(())
}

#[test]
fn selector_visits_compose_held_models_and_sample_the_new_target_before_release() -> Result<()> {
    let mut body = (*definition()).clone();
    body.initial.rate = 0.5;
    body.target_bones = vec![1];
    body.target_marker = Some(Anchor {
        bone: 1,
        offset: [0., 40., 0.],
    });
    body.weapons.push(Arc::new(WeaponDefinition {
        slot: 0,
        resource: 8,
        attachment: 1,
        skeleton: body.skeleton.clone(),
        motions: BTreeMap::new(),
        playback: WeaponPlayback::Rigid,
        anchors: vec![],
        links: vec![],
    }));
    let body = Arc::new(body);
    let actors = [-300., 100., 200., 300.].map(|x| {
        let mut subject = actor();
        subject.position[0] = x;
        subject.body.tint = [64, 65, 66, 173];
        if x > 0. {
            subject.side = Side::Enemy;
        }
        subject
    });
    let p = Arc::try_unwrap(crate::tests::prepared(
        "pub task run() { await battle::next_update(); }",
        actors.to_vec(),
        20,
    ))
    .unwrap();
    let mut actions = p.actions;
    actions[0].phase = ActionPhase::Actor;
    actions[0].tp_cost = 0;
    let action = actions[0].id;
    let mut prepared = PreparedBattle::new(p.actors, actions, 1, vec![Some(body); 4], vec![])?
        .with_controls(vec![crate::ControlDefinition {
            actor: ActorId(0),
            target: ActorId(1),
            combo_limit: 1,
            walk_speed: 5.,
            run_speed: 10.,
            turn_ticks: 8,
            motions: None,
            shortcuts: [None; 4],
            normals: std::array::from_fn(|_| crate::NormalControl {
                action,
                allowed_directions: 0,
                fallback: None,
                reach: 120.,
                minimum_reach: 0.,
                combo_at: [0; 2],
                buffer_until: 0,
            }),
        }])?;
    prepared.ambient_color = [45, 46, 47];
    let mut battle = Battle::new(Arc::new(prepared));
    let before = battle.step(BattleInput::default())?;
    let random = battle.random_state();
    let elapsed = battle.ledger().elapsed_ticks;
    battle.target_selector = Some(ActorId(0));
    // Retained state2 must skip selector shading. Keep its body visible for
    // this isolated model test rather than starting a death controller.
    battle.actors[3].availability = crate::ActorAvailability::Dead;
    battle.actors[0].hp = 39;
    for index in 0..3 {
        battle.contact_feedback[index].flash([192, 128, 128]);
    }
    battle.actors[0].position[0] += 10.;
    battle.actors[0].heading += 35.;
    battle.models[0].as_mut().unwrap().play(
        MotionBinding { model: 7, clip: 1 },
        8.,
        0.5,
        false,
        4,
    )?;
    battle.actors[0].body.jitter.request(8);
    battle.actors[0]
        .body
        .jitter
        .advance(&mut crate::Random::from_state(1));
    let mut select = crate::ControlInput::neutral(ActorId(0));
    select.target.held = true;
    select.target_step = 1;
    let held = battle.step(BattleInput {
        controllers: vec![select],
        ..Default::default()
    })?;
    assert_eq!(held.targets[0], Some(ActorId(2)));
    assert_eq!(held.target_markers[0].target, ActorId(2));
    assert_eq!(held.update, before.update);
    assert_eq!(held.hud_update, before.hud_update + 1);
    assert_eq!(held.actors[0].hud.hp, 39);
    assert!(held.hud_holds.notices && held.hud_holds.combo_tracking);
    assert_eq!(held.models[0].world, before.models[0].world);
    assert_eq!(held.models[0].bones, before.models[0].bones);
    assert_eq!(held.models[0].frame, before.models[0].frame);
    assert_eq!(held.weapons[0].world, before.weapons[0].world);
    assert_eq!(battle.actors[0].body.jitter.take_acceleration(), [0.; 3]);
    assert_eq!(battle.actors[0].body.jitter.remaining, 7);
    assert_eq!(battle.random_state(), random);
    assert_eq!(battle.ledger().elapsed_ticks, elapsed);
    for index in 0..4 {
        let expected = match index {
            2 => [45, 46, 47, 173],
            // The preceding ordinary actor visit approached ambient once.
            3 => before.actors[3].body.tint,
            _ => [22, 23, 23, 173],
        };
        assert_eq!(held.models[index].tint, expected);
        assert_eq!(held.weapons[index].tint, expected);
    }
    // Selected flash was cleared; other flash clocks remain held.
    for index in 0..3 {
        let mut tint = [64, 65, 66, 173];
        battle.contact_feedback[index].appearance(&mut tint, None);
        assert_eq!(
            tint,
            if index == 2 {
                [64, 65, 66, 173]
            } else {
                [192, 128, 128, 173]
            }
        );
    }
    let release = battle.step(BattleInput::default())?;
    assert_eq!(release.target_selector, None);
    assert_eq!(release.hud_update, held.hud_update + 1);
    assert!(release.hud_holds.notices && release.hud_holds.combo_tracking);
    assert_eq!(release.models, held.models);
    assert_eq!(release.weapons, held.weapons);
    let resumed = battle.step(BattleInput::default())?;
    assert!(!resumed.hud_holds.notices);
    assert_ne!(resumed.models[0].world, held.models[0].world);
    assert_eq!(resumed.models[0].tint, [192, 128, 128, 173]);
    assert_eq!(resumed.models[2].tint, before.actors[2].body.tint);
    Ok(())
}
