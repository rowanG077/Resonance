use super::*;
use crate::{
    Activity, Battle, Cue, ParticleDefinition, PreparedBattle, Side, SoundBinding, StunBinding,
};
use resonance_content::{
    animation::{Bone, Transform, TransformChannels},
    battle_effect::declaration::Declaration,
};
use serde_json::Value;

fn number(value: &Value) -> f32 {
    f32::from_bits(value.as_u64().unwrap() as u32)
}
fn vector(value: &Value) -> [f32; 3] {
    std::array::from_fn(|i| number(&value[i]))
}

#[test]
fn natural_stun_updates_match_controller_motion_particles_sound_and_rng() {
    let particle: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/stun-particle-source.json"
    ))
    .unwrap();
    let declaration: Declaration = serde_json::from_value(particle["declaration"].clone()).unwrap();
    let uv = serde_json::from_value::<Vec<_>>(particle["uv"].clone()).unwrap();
    let particle = Arc::new(ParticleDefinition {
        model: None,
        resource: 8,
        member: 19,
        data: declaration.particle(&uv).unwrap(),
    });
    for source in [
        include_str!("../../tests/fixtures/opening-stun-update.json"),
        include_str!("../../tests/fixtures/opening-stun-recovery.json"),
    ] {
        let fixture: Value = serde_json::from_str(source).unwrap();
        for row in fixture["observations"].as_array().unwrap() {
            let before = &row["before"];
            let expected = &row["after"];
            let motion = &before["motion"];
            let mut actor = crate::tests::actor(Side::Enemy);
            actor.control = match row["control"].as_u64().unwrap() {
                0 => crate::Control::Manual,
                1 => crate::Control::SemiAuto,
                2 => crate::Control::Auto,
                3 => crate::Control::Enemy,
                value => panic!("unexpected control {value}"),
            };
            actor.position = vector(&before["position_bits"]);
            actor.heading = number(&row["heading_bits"]);
            actor.body.scale = number(&row["scale_bits"]);
            actor.hit_stop = before["hit_stop"].as_u64().unwrap() as u8;
            actor.guard.auto_chance = before["auto_guard_chance"].as_u64().unwrap() as u8;
            actor.guard.recovery_bonus = row["guard_recovery_bonus"].as_u64().unwrap() as u8;
            let v = &before["velocity_bits"];
            actor.movement.forward = number(&v[2]);
            actor.movement.vertical = number(&v[3]);
            actor.movement.acceleration = number(&v[4]);
            actor.movement.gravity = number(&v[5]);
            actor.movement.braking = number(&before["braking_bits"]);
            actor.movement.flying = row["flying"].as_bool().unwrap();
            actor.movement.fixed_height = row["fixed_height"].as_bool().unwrap();
            actor.reaction.direction = vector(&row["direction_bits"]);
            let head = row["head"].as_u64().unwrap() as u16;
            let definition = Arc::new(ModelDefinition {
                secondary_motion: vec![],
                resource: 7,
                skeleton: Skeleton {
                    bones: (0..=head)
                        .map(|i| Bone {
                            name: i.to_string(),
                            parent: (i != 0).then_some(0),
                            bind_channels: TransformChannels(8),
                            bind: Transform::default(),
                        })
                        .collect(),
                },
                motions: [3, 7, 9, 21]
                    .into_iter()
                    .map(|clip| {
                        (
                            clip,
                            Motion {
                                duration_frames: if clip == motion["clip"].as_u64().unwrap() as u16
                                {
                                    number(&motion["end_bits"])
                                } else {
                                    32.
                                },
                                tracks: vec![],
                            },
                        )
                    })
                    .collect(),
                initial: Playback {
                    clip: motion["clip"].as_u64().unwrap() as u16,
                    frame: number(&motion["frame_bits"]),
                    // Resume the observed drawing sample without another
                    // entry-clock advance; restore its live rate below.
                    rate: 0.,
                    repeat: motion["flags"].as_u64().unwrap() & 1 != 0,
                },
                hurt_motions: [None; 2],
                idle_motions: [None; 2],
                guard_motions: [None; 2],
                knockdown: None,
                stun: Some(StunBinding {
                    particle: particle.clone(),
                    head,
                    offset: vector(&row["offset_bits"]),
                    loop_motion: 21,
                    down_motion: 7,
                    recovery_motion: 9,
                    sound: SoundBinding {
                        resource: 9,
                        index: 117,
                    },
                }),
                anchors: vec![],
                approach_bones: vec![],
                target_bones: vec![],
                shadow: None,
                target_marker: None,
                weapons: vec![],
                hurt_bones: vec![],
                suppress_root_translation: [false; 3],
            });
            let mut battle = Battle::new(Arc::new(
                PreparedBattle::new(
                    vec![actor],
                    vec![],
                    row["random_before"].as_u64().unwrap() as u32,
                    vec![Some(definition)],
                    vec![],
                )
                .unwrap(),
            ));
            // These original sampled head coordinates are inputs to the controller;
            // this comparison makes no claim about skeletal sampling fidelity.
            let model = battle.models[0].as_mut().unwrap();
            model.animation.clock.rate = number(&motion["rate_bits"]);
            model.shown.world = Transform {
                translation: vector(
                    &if expected["particle"].is_null() {
                        &before["particle"]
                    } else {
                        &expected["particle"]
                    }["origin_bits"],
                ),
                ..Default::default()
            }
            .matrix();
            model.shown.bones =
                Arc::new(vec![Transform::default().matrix(); usize::from(head) + 1]);
            model.animation.clock.finished = motion["state"].as_u64().unwrap() & 0x01000000 != 0;
            model.animation.clock.blend = number(&motion["blend_bits"]) as u8;
            model.animation.clock.blend_age = number(&motion["blend_age_bits"]) as u8;
            let held = model.shown.clone();
            let id = battle
                .spawn_particle(
                    particle.clone(),
                    None,
                    ActorId(0),
                    ActorId(0),
                    vector(&before["particle"]["origin_bits"]),
                    0.,
                )
                .unwrap()
                .unwrap();
            let actor = &mut battle.actors[0];
            actor.activity = Activity::Stunned;
            actor.reaction.remaining = before["hitstun"].as_i64().unwrap() as i16;
            actor.reaction.combo_hits = before["combo_hits"].as_i64().unwrap() as i32;
            actor.reaction.combo_damage = before["combo_damage"].as_i64().unwrap() as i32;
            actor.reaction.stun.pulse = before["pulse"].as_u64().unwrap() as u8;
            actor.reaction.stun.particle = Some(id);
            let mut cues = vec![];
            battle.advance_stun(ActorId(0), false, &mut cues).unwrap();
            let actor = &battle.actors[0];
            assert_eq!(
                actor.activity,
                if expected["activity"] == 2 {
                    Activity::Idle
                } else {
                    Activity::Stunned
                }
            );
            assert_eq!(
                u64::from(actor.guard.auto_chance),
                expected["auto_guard_chance"].as_u64().unwrap()
            );
            assert_eq!(
                i64::from(actor.reaction.combo_hits),
                expected["combo_hits"].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(actor.reaction.combo_damage),
                expected["combo_damage"].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(actor.reaction.remaining),
                expected["hitstun"].as_i64().unwrap()
            );
            assert_eq!(
                u64::from(actor.reaction.stun.pulse),
                expected["pulse"].as_u64().unwrap()
            );
            assert_eq!(
                actor.position.map(f32::to_bits),
                vector(&expected["position_bits"]).map(f32::to_bits)
            );
            assert_eq!(
                actor.movement.previous_position.map(f32::to_bits),
                vector(&expected["origin_bits"]).map(f32::to_bits)
            );
            assert_eq!(
                [
                    actor.movement.forward,
                    actor.movement.vertical,
                    actor.movement.acceleration,
                    actor.movement.gravity
                ]
                .map(f32::to_bits),
                std::array::from_fn(|i| number(&expected["velocity_bits"][i + 2]).to_bits())
            );
            assert_eq!(
                u64::from(battle.random_state()),
                row["random_after"].as_u64().unwrap()
            );
            if expected["particle"].is_null() {
                assert!(battle.particles.is_empty());
                assert!(actor.reaction.stun.particle.is_none());
                assert!(cues.contains(&Cue::ParticleExpired { particle: id }));
            } else {
                let frame = &battle.particles[&id].frame;
                let state = &frame.state;
                assert_eq!(
                    frame.origin.map(f32::to_bits),
                    vector(&expected["particle"]["origin_bits"]).map(f32::to_bits)
                );
                assert_eq!(
                    state.offset.map(f32::to_bits),
                    vector(&expected["particle"]["offset_bits"]).map(f32::to_bits)
                );
                assert_eq!(
                    state.angular_velocity.map(f32::to_bits),
                    vector(&expected["particle"]["angular_velocity_bits"]).map(f32::to_bits)
                );
                assert_eq!(
                    state.palettes,
                    serde_json::from_value::<[u8; 2]>(expected["particle"]["palettes"].clone())
                        .unwrap()
                );
                assert_eq!(
                    u64::from(state.geometry_count),
                    expected["particle"]["geometry_count"].as_u64().unwrap()
                );
            }
            let model = battle.models[0].as_ref().unwrap();
            let motion = &expected["motion"];
            assert_eq!(u64::from(model.clip), motion["clip"].as_u64().unwrap());
            assert_eq!(
                model.animation.clock.frame.to_bits(),
                number(&motion["frame_bits"]).to_bits()
            );
            assert_eq!(
                model.animation.clock.rate.to_bits(),
                number(&motion["rate_bits"]).to_bits()
            );
            assert_eq!(
                model.animation.clock.blend,
                number(&motion["blend_bits"]) as u8
            );
            assert_eq!(model.shown, held);
            let sounds: Vec<_> = cues
                .iter()
                .filter_map(|cue| match cue {
                    Cue::Sound {
                        actor,
                        sound,
                        priority,
                        ..
                    } => {
                        assert_eq!(*actor, ActorId(0));
                        Some((u64::from(sound.index), u64::from(*priority)))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                sounds,
                row["sounds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| (
                        s["index"].as_u64().unwrap(),
                        s["priority"].as_u64().unwrap()
                    ))
                    .collect::<Vec<_>>()
            );
        }
    }
}
