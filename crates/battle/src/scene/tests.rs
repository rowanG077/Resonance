use super::*;
use crate::{
    ActionDefinition, ActionPhase, ActionRequest, BattleInput, Control, ModelDefinition,
    MotionBinding, Playback, PreparedBattle, ResourceBinding, Side, SoundBinding,
};
use resonance_content::animation::{Bone, Motion, Skeleton, Transform, TransformChannels};
use std::{collections::BTreeMap, sync::Arc};

const NURSE: &str = r#"
    battle::play_motion(chant, ticks(8), 0.0, 0.5, true, 0.0);
    await casting::stored(spell, ticks(310), ticks(90), entry,
        battle::CastMotion { age: ticks(0), motion: release, blend: ticks(4), frame: 0.0,
            rate: 0.5, repeat: false, loop_start: 0.0 },
        casting::Effects { effect: common, pulse_member: 4, release_member: 8,
            release_sound: release_sound, scale: 1.0,
            tint: battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 } },
        entry_sound, casting::Voices { chant: silent_voice, fallback: silent_voice, release: release_voice });
    battle::finish();
"#;

fn battle(body: &str) -> Result<Battle> {
    let compiled = symphonia_script_compiler::compile(
        "test",
        &BTreeMap::from([
            (
                "test".into(),
                format!(
                    r#"
                script battle;
                use battle;
                use battle::casting;
                use battle::nurse;
                asset spell: battle::Spell = "test/spell";
                asset common: battle::Effect = "test/common";
                asset chant: battle::Motion = "test/11";
                asset release: battle::Motion = "test/12";
                asset entry: battle::Motion = "test/13";
                asset entry_sound: battle::Sound = "test/122";
                asset release_sound: battle::Sound = "test/123";
                asset release_voice: battle::Voice = "test/release_voice";
                asset silent_voice: battle::Voice = "test/silent_voice";
                pub task run() {{ {body} }}
                pub task resident() {{ await nurse::run(); }}
            "#
                ),
            ),
            (
                "battle::casting".into(),
                include_str!("../../../../scripts/battle/casting.sym").into(),
            ),
            (
                "battle::nurse".into(),
                include_str!("../../../../scripts/battle/nurse.sym").into(),
            ),
        ]),
        &crate::native_declarations(),
    )?;
    let assets = compiled
        .assets
        .iter()
        .map(|asset| match asset.path.as_str() {
            "test/silent_voice" => ResourceBinding::Voice(vec![None; 4]),
            "test/release_voice" => ResourceBinding::Voice(vec![
                None,
                None,
                Some(crate::VoiceLine {
                    sound: SoundBinding {
                        resource: 1,
                        index: 0x81a7,
                    },
                    duration: 0,
                }),
                None,
            ]),
            "battle/voices/relative/42" => ResourceBinding::Voice(vec![
                Some(crate::VoiceLine {
                    sound: SoundBinding {
                        resource: 1,
                        index: 43,
                    },
                    duration: 0,
                }),
                Some(crate::VoiceLine {
                    sound: SoundBinding {
                        resource: 1,
                        index: 163,
                    },
                    duration: 0,
                }),
                Some(crate::VoiceLine {
                    sound: SoundBinding {
                        resource: 1,
                        index: 404,
                    },
                    duration: 0,
                }),
                None,
            ]),
            "battle/effects/tints.json" => crate::tests::actor_tints(),
            "test/spell" => ResourceBinding::Spell(237),
            "test/common" => ResourceBinding::Effect(1),
            "battle/scenes/237.json" => ResourceBinding::Effect(2),
            "test/122" | "test/123" => ResourceBinding::Sound(SoundBinding {
                resource: 1,
                index: asset.path[5..].parse().unwrap(),
            }),
            _ => ResourceBinding::Motion(MotionBinding {
                model: 1,
                clip: asset.path[5..].parse().unwrap(),
            }),
        })
        .collect::<Vec<_>>();
    let program = Arc::new(compiled.program);
    let entry = |name: &str| {
        program
            .authored()
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .entry
    };
    let actions = vec![
        ActionDefinition {
            id: 99,
            phase: ActionPhase::Casting,
            program: program.clone(),
            entry: entry("test::run"),
            duration: 1000,
            tp_cost: 28,
            resources: assets.clone(),
        },
        ActionDefinition {
            id: 237,
            phase: ActionPhase::Resident,
            program: program.clone(),
            entry: entry("test::resident"),
            duration: 250,
            tp_cost: 0,
            resources: assets,
        },
    ];
    let mut actors = vec![crate::tests::actor(Side::Party); 3];
    for (actor, (hp, max_hp)) in actors.iter_mut().zip([(100, 328), (50, 172), (150, 413)]) {
        actor.hp = hp;
        actor.max_hp = max_hp;
        actor.tp = 90;
        actor.max_tp = 90;
        actor.control = Control::Auto;
    }
    actors[0].control = Control::Manual;
    actors.push(crate::tests::actor(Side::Enemy));
    let model = Arc::new(ModelDefinition {
        resource: 1,
        skeleton: Skeleton {
            bones: vec![Bone {
                name: "root".into(),
                parent: None,
                bind_channels: TransformChannels(8),
                bind: Transform::default(),
            }],
        },
        motions: [(0, 40.), (11, 2.), (12, 15.), (13, 4.)]
            .into_iter()
            .map(|(id, duration_frames)| {
                (
                    id,
                    Motion {
                        duration_frames,
                        tracks: vec![],
                    },
                )
            })
            .collect(),
        secondary_motion: vec![],
        initial: Playback {
            clip: 0,
            frame: 0.,
            rate: 0.5,
            repeat: true,
        },
        hurt_motions: [None; 2],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        stun: None,
        knockdown: None,
        anchors: vec![],
        approach_bones: vec![],
        target_bones: vec![],
        shadow: None,
        target_marker: None,
        weapons: vec![],
        hurt_bones: vec![],
        suppress_root_translation: [false; 3],
    });
    Ok(Battle::new(Arc::new(
        PreparedBattle::new(
            actors,
            actions,
            1,
            vec![Some(model); 4],
            vec![
                crate::tests::effect_binding(1, [4, 8, 37]),
                crate::tests::effect_binding(2, [1, 2, 3, 4, 5, 6]),
            ],
        )?
        .with_camera(crate::CameraDefinition {
            leader: ActorId(0),
            target: ActorId(3),
            stage_pitch: -1.,
            adaptive: true,
            initial: crate::CameraPose {
                eye: [0., 490., 2050.],
                focus: [0., 120., 0.],
                pitch: 7.,
                yaw: 90.,
                radius: 2050.,
            },
        })?
        .with_stage_colors(
            [64, 64, 64, 255],
            [Some([64, 64, 64, 255]), None, None, None],
        ),
    )))
}

fn request(actor: u8) -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(actor),
            action: 99,
            target: ActorId(3),
        }],
        ..Default::default()
    }
}

#[test]
fn maintained_nurse_cast_matches_original_payment_transition_motion_release_and_retirement()
-> Result<()> {
    #[derive(serde::Deserialize)]
    struct CenterProfile {
        center_offset: [f32; 3],
        model_scale: f32,
    }
    #[derive(serde::Deserialize)]
    struct Centers {
        combat_tick: u16,
        position_bits: [[u32; 3]; 4],
        heading_bits: [u32; 4],
        center_bits: [[u32; 3]; 4],
    }
    #[derive(serde::Deserialize)]
    struct CenterFixture {
        actors: [CenterProfile; 4],
        observations: Vec<Centers>,
    }
    let centers: CenterFixture =
        serde_json::from_str(include_str!("../../tests/fixtures/nurse-centers.json"))?;
    let observed: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/nurse-transition-motion.json"
    ))?;
    let presentation: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/nurse-presentation.json"))?;
    let camera: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/nurse-camera.json"))?;
    let mut battle = battle(NURSE)?;
    for (actor, profile) in battle.actors.iter_mut().zip(&centers.actors) {
        actor.body.center_offset = profile.center_offset;
        actor.body.scale = profile.model_scale;
    }
    let mut held = None;
    let mut released = None;
    for tick in 5..=630 {
        // Supply original movement at the component boundary. Ownership and
        // center sampling still run through the maintained casting sequence.
        if let Some(previous) = centers
            .observations
            .iter()
            .find(|row| row.combat_tick == tick - 1)
        {
            for (i, actor) in battle.actors.iter_mut().enumerate() {
                actor.position = previous.position_bits[i].map(f32::from_bits);
                actor.heading = f32::from_bits(previous.heading_bits[i]);
                if tick == 301 {
                    actor.body.center = previous.center_bits[i].map(f32::from_bits);
                }
            }
        }
        if tick == 301 {
            battle.camera.as_mut().unwrap().pose = observed_camera(&camera["observations"][0]);
        }
        let frame = battle.step(if tick == 5 {
            request(2)
        } else {
            BattleInput::default()
        })?;
        if tick > 300 {
            let row = &camera["observations"][(tick - 300) as usize];
            let expected = observed_camera(row);
            let actual = frame.camera.unwrap();
            let values = |pose: crate::CameraPose| {
                pose.eye
                    .into_iter()
                    .chain(pose.focus)
                    .chain([pose.pitch, pose.yaw, pose.radius])
            };
            for (value, expected) in values(actual).zip(values(expected)) {
                assert_eq!(
                    value.to_bits(),
                    expected.to_bits(),
                    "camera at original tick {tick}: {actual:?}, expected {row}"
                );
            }
            let state = battle.camera.as_ref().unwrap();
            let flags = row["battle_camera_flags_transition_word"].as_u64().unwrap() >> 16;
            let phase = match flags & 0xc0 {
                0x40 => crate::camera::Phase::Entry,
                0x80 => crate::camera::Phase::Returning,
                _ => crate::camera::Phase::Tracking,
            };
            assert_eq!(state.phase, phase, "camera phase at {tick}");
            assert_eq!(
                u64::from(state.remaining),
                row["battle_camera_impulse_timer_word"].as_u64().unwrap() >> 16,
                "camera bounds duration at {tick}"
            );
            for (value, field) in [
                (state.minimum_radius, "battle_camera_impulse_low_bits"),
                (state.minimum_pitch, "battle_camera_impulse_high_bits"),
            ] {
                assert_eq!(
                    u64::from(value.to_bits()),
                    row[field].as_u64().unwrap(),
                    "{field} at {tick}"
                );
            }
        }
        if tick >= 300 {
            let row = &presentation["observations"][(tick - 300) as usize];
            assert_eq!(row["tick"], tick);
            assert_eq!(
                u32::from_be_bytes(frame.stage_colors[0].unwrap()),
                row["battle_stage_model_0_tint_word"].as_u64().unwrap() as u32,
                "stage model color at original tick {tick}"
            );
            for (i, channel) in battle.stage_colors.channels.iter().enumerate() {
                assert_eq!(
                    u32::from_be_bytes(channel.target),
                    row[format!("battle_stage_tint_target_{i}_word")]
                        .as_u64()
                        .unwrap() as u32,
                    "stage color channel {i} at original tick {tick}"
                );
            }
            assert_eq!(
                (u32::from(battle.stage_colors.channels[0].remaining) << 16)
                    | u32::from(battle.stage_colors.channels[1].remaining),
                row["battle_stage_tint_timers_word"].as_u64().unwrap() as u32,
                "stage color timers at original tick {tick}"
            );
            let channels = &battle.stage_colors.channels;
            let flags =
                (u32::from(channels[0].active) | (u32::from(channels[1].active) << 1)) << 16;
            assert_eq!(
                (1 << 24)
                    | flags
                    | (u32::from(channels[0].step) << 8)
                    | u32::from(channels[1].step),
                row["battle_stage_tint_flags_word"].as_u64().unwrap() as u32,
                "stage color flags at original tick {tick}"
            );
            for i in 0..3 {
                assert_eq!(
                    u32::from_be_bytes(frame.actors[i].body.tint),
                    row[format!("battle_actor_{i}_tint_word")].as_u64().unwrap() as u32,
                    "actor {i} tint at original tick {tick}"
                );
                // Healing finishes fading at 546, sampled at 547. At 588
                // Lloyd has a separate gray overlay (actor+1dc/290), outside
                // this recovery component. Its model path remains unaccepted.
                if tick <= 547 {
                    assert_eq!(
                        u32::from_be_bytes(frame.models[i].tint),
                        row[format!("battle_actor_{i}_model_tint_word")]
                            .as_u64()
                            .unwrap() as u32,
                        "model {i} tint at original tick {tick}"
                    );
                }
            }
        }
        if tick > 300 {
            let row = &centers.observations[(tick - 300) as usize];
            for (i, actor) in frame.actors.iter().enumerate() {
                assert_eq!(
                    actor.body.center.map(f32::to_bits),
                    row.center_bits[i],
                    "center of actor {i} at original tick {tick}"
                );
            }
        }
        assert_eq!(
            frame.actors[2].tp,
            if tick < 316 { 90 } else { 62 },
            "tick {tick}"
        );
        assert_eq!(
            frame.hud_holds,
            crate::HudHolds::default(),
            "stored transition filters actor pause flags at tick {tick}"
        );
        assert_eq!(
            frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Notice { .. }))
                .cloned()
                .collect::<Vec<_>>(),
            if tick == 316 {
                vec![Cue::Notice {
                    actor: ActorId(2),
                    action: 99,
                    duration: 315,
                    kind: 1,
                }]
            } else {
                vec![]
            },
            "stored notice starts with payment, before scene activation, at tick {tick}"
        );
        match tick {
            316..=375 => assert_eq!(frame.scenes[0].remaining, Some(375 - tick)),
            376..=628 => assert_eq!(frame.scenes[0].remaining, None),
            _ => assert!(frame.scenes.is_empty(), "tick {tick}"),
        }
        assert!(
            frame
                .models
                .iter()
                .all(|m| m.visible == !(377..=628).contains(&tick)),
            "visibility at {tick}"
        );
        for cue in &frame.cues {
            if let Cue::Released { action, .. } = cue {
                assert_eq!(tick, 376);
                assert!(released.replace(*action).is_none());
            }
        }
        if tick == 316 {
            held = Some(frame.models[0].clone());
        } else if (317..=376).contains(&tick) {
            let model = &frame.models[0];
            let held = held.as_ref().unwrap();
            // World transforms recompose from the supplied root, while the
            // sampled local pose and playback remain held.
            assert_eq!(
                (
                    model.clip,
                    model.frame,
                    model.blend_weight,
                    model.root_translation,
                    &model.bones
                ),
                (
                    held.clip,
                    held.frame,
                    held.blend_weight,
                    held.root_translation,
                    &held.bones
                )
            );
        } else if tick == 377 {
            assert_ne!(frame.models[0].frame, held.as_ref().unwrap().frame);
            assert_eq!(battle.sequences[&ActionId(1)].recovery, Some(90));
        }
        // 1C40 increments combat time before callbacks. Its preceding model
        // visit is labelled tick-1; this is one fixed source clock registration.
        if let Some(row) = observed["observations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["actor"] == "Raine" && row["combat_tick"] == tick - 1)
        {
            assert_eq!(
                frame.models[2].frame.to_bits(),
                row["after"]["frame_bits"].as_u64().unwrap() as u32,
                "model before callback tick {tick}"
            );
        }
        if tick == 498 {
            assert_eq!(
                frame.actors[..3].iter().map(|a| a.hp).collect::<Vec<_>>(),
                [231, 118, 315]
            );
        }
        if tick == 377 || tick == 628 {
            let sequence = &battle.sequences[&released.unwrap()];
            assert_eq!(sequence.age, if tick == 377 { 0 } else { 251 });
        }
        if tick == 629 {
            assert!(!battle.sequences.contains_key(&released.unwrap()));
            assert!(frame.cues.contains(&Cue::Completed {
                action: released.unwrap()
            }));
        }
    }
    Ok(())
}

const SHORT: &str = r#"
    battle::begin_scene(spell, ticks(2));
    battle::pay_tp(battle::tp_cost());
    await battle::next_update();
    while battle::scene_remaining() > ticks(0) { await battle::next_update(); }
    battle::activate_scene();
    await battle::next_update();
    battle::finish();
"#;

#[test]
fn scene_pause_holds_actor_timers_menu_and_residents_but_not_late_effects() -> Result<()> {
    let mut battle = battle(SHORT)?;
    for late in [false, true] {
        battle.show_effect(
            crate::effect::Spawn {
                scene: None,
                action: ActionId(0),
                owner: ActorId(0),
                target: ActorId(0),
                appearance: crate::EffectAppearance {
                    resource: 1,
                    member: 4,
                },
                origin: [0.; 3],
                heading: 0.,
                follow: None,
                scale: 1.,
                late,
                tint: Default::default(),
            },
            &mut vec![],
        )?;
    }
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/stun-particle-source.json"
    ))?;
    let declaration: resonance_content::battle_effect::declaration::Declaration =
        serde_json::from_value(fixture["declaration"].clone())?;
    let uv: Vec<resonance_content::battle_effect::UvRecord> =
        serde_json::from_value(fixture["uv"].clone())?;
    let mut particles = Vec::new();
    for late in [false, true] {
        let mut data = declaration.particle(&uv)?;
        data.late = late;
        particles.push(
            battle
                .spawn_particle(
                    Arc::new(crate::ParticleDefinition {
                        model: None,
                        resource: 1,
                        member: 19,
                        data,
                    }),
                    None,
                    ActorId(0),
                    ActorId(0),
                    [0.; 3],
                    0.,
                )?
                .unwrap(),
        );
    }
    battle.actors[0].movement.direction = [1., 0., 0.];
    battle.actors[0].movement.forward = 1.;
    battle.actors[0].hit_stop = 7;
    battle.actors[0].body.tint = [40, 112, 40, 255];
    let first = battle.step(request(2))?;
    assert_eq!(first.scenes[0].remaining, Some(1));
    assert_eq!(first.actors[0].hit_stop, 6);
    assert_eq!(first.actors[0].body.tint, [41, 111, 41, 255]);
    assert_eq!(first.models[0].tint, [40, 112, 40, 255]);
    assert!(
        !battle.sequences[&ActionId(1)]
            .effect
            .as_ref()
            .unwrap()
            .retiring
    );
    assert!(
        battle.sequences[&ActionId(2)]
            .effect
            .as_ref()
            .unwrap()
            .retiring
    );
    assert_eq!(first.particles.len(), 1);
    assert_eq!(first.particles[0].id, particles[1]);
    assert_eq!(first.particles[0].age, 0);
    let paused = battle.step(BattleInput {
        menu_open: true,
        ..Default::default()
    })?;
    assert_eq!(paused.scenes, first.scenes);
    assert_eq!(paused.models, first.models);
    assert_eq!(paused.update, first.update);
    assert_eq!(paused.actors[0].body.tint, first.actors[0].body.tint);
    let held = battle.step(BattleInput::default())?;
    assert_eq!(held.scenes[0].remaining, Some(0));
    assert_eq!(held.actors[0].hit_stop, 6);
    assert_eq!(held.actors[0].position, first.actors[0].position);
    assert_eq!(held.actors[0].body.tint, first.actors[0].body.tint);
    assert_eq!(held.models[0].tint, first.actors[0].body.tint);
    assert!(!battle.sequences.contains_key(&ActionId(2)));
    assert_eq!(held.particles[0].age, 1);
    let released = battle.step(request(0))?;
    assert!(released.cues.contains(&Cue::Rejected {
        actor: ActorId(0),
        reason: crate::Rejection::Busy
    }));
    let id = released
        .cues
        .iter()
        .find_map(|c| match c {
            Cue::Released { action, .. } => Some(*action),
            _ => None,
        })
        .unwrap();
    assert!(matches!(
        battle.sequences[&id].resident.as_ref().unwrap().phase,
        crate::script::ResidentPhase::Initializing
    ));
    let resumed = battle.step(BattleInput::default())?;
    assert_eq!(resumed.actors[0].hit_stop, 5);
    assert_eq!(resumed.actors[0].body.tint, [42, 110, 42, 255]);
    assert_eq!(
        resumed
            .particles
            .iter()
            .map(|p| (p.id, p.age))
            .collect::<Vec<_>>(),
        [(particles[0], 0), (particles[1], 3)]
    );
    assert!(
        battle.sequences[&ActionId(1)]
            .effect
            .as_ref()
            .unwrap()
            .retiring
    );
    assert_eq!(battle.sequences[&id].age, 0);
    Ok(())
}

#[test]
fn petrification_refreshes_the_center_but_holds_actor_callbacks_and_timers() -> Result<()> {
    let mut battle = battle("await battle::next_update(); await battle::next_update();")?;
    let started = battle.step(request(0))?;
    let action = started.actions[0].0;
    let age = battle.action_age(action);
    battle.actors[0].petrified = true;
    battle.actors[0].position = [30., 20., 10.];
    battle.actors[0].body.center_offset = [0., 80., 0.];
    battle.actors[0].hit_stop = 6;
    let held = battle.step(BattleInput::default())?;
    assert_eq!(held.actors[0].body.center, [30., 100., 10.]);
    assert_eq!(held.actors[0].position, [30., 20., 10.]);
    assert_eq!(held.actors[0].hit_stop, 6);
    assert_eq!(battle.action_age(action), age);
    assert!(held.cues.is_empty());
    battle.actors[0].petrified = false;
    battle.step(BattleInput::default())?;
    assert!(battle.action_age(action) > age);
    Ok(())
}

#[test]
fn transition_entry_holds_later_actor_callbacks_and_their_sampled_centers() -> Result<()> {
    let mut battle = battle(SHORT)?;
    battle.actors[0].body.center_offset = [0., 80., 0.];
    battle.actors[1].body.center_offset = [0., 60., 0.];
    battle.actors[1].position = [50., 0., 0.];
    battle.actors[1].hit_stop = 6;
    let held = battle.actors[1].body.center;
    let entered = battle.step(request(0))?;
    assert_eq!(entered.actors[0].body.center[1], 80.);
    assert_eq!(entered.actors[1].body.center, held);
    assert_eq!(entered.actors[1].hit_stop, 6);
    battle.step(BattleInput::default())?;
    let activated = battle.step(BattleInput::default())?;
    assert_eq!(activated.actors[1].body.center, held);
    let resumed = battle.step(BattleInput::default())?;
    assert_eq!(resumed.actors[1].body.center, [50., 60., 0.]);
    assert_eq!(resumed.actors[1].hit_stop, 5);
    Ok(())
}

#[test]
fn stored_chant_jitter_rearms_before_transition_and_holds_until_actor_callbacks_resume()
-> Result<()> {
    let mut battle = battle(&NURSE.replace("ticks(310)", "ticks(1)"))?;
    battle.step(request(2))?;
    assert!(!battle.actors[2].body.jitter.active);
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[2].body.jitter.remaining, 7);
    let entered = battle.step(BattleInput::default())?;
    assert!(
        entered
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Notice { duration: 315, .. }))
    );
    assert_eq!(battle.actors[2].body.jitter.remaining, 7);
    // Stored 3898C writes 1B0=22 at scene admission, before 385A0.
    assert!(battle.actors[2].hud.cast_released);
    let mut held = 0;
    while battle.transition_owner().is_some() {
        battle.step(BattleInput::default())?;
        assert!(battle.actors[2].body.jitter.active);
        assert_eq!(battle.actors[2].body.jitter.remaining, 7);
        held += 1;
        assert!(held <= 60);
    }
    assert!(held > 0);
    // The owner's transition compositions consumed the pending vector but
    // never advanced the actor-common timer or requested a replacement.
    assert_eq!(battle.actors[2].body.jitter.take_acceleration(), [0.; 3]);
    battle.step(BattleInput::default())?;
    // The stored branch already selected385A0 before its transition hold.
    // The resumed, finished release pose reaches the same recovery writer.
    assert_eq!(battle.actors[2].activity, crate::Activity::Recovering);
    assert_eq!(battle.actors[2].body.jitter.remaining, 89);
    assert!(battle.actors[2].hud.cast_released);
    for remaining in (81..=88).rev() {
        battle.step(BattleInput::default())?;
        assert!(battle.actors[2].body.jitter.active);
        assert_eq!(battle.actors[2].body.jitter.remaining, remaining);
    }
    Ok(())
}

#[test]
fn interrupted_pending_scene_frees_its_slot_without_refunding_or_releasing() -> Result<()> {
    let mut battle = battle(SHORT)?;
    battle.step(request(2))?;
    let frame = battle.step(BattleInput {
        interrupt: vec![ActionId(1)],
        ..Default::default()
    })?;
    assert!(frame.scenes.is_empty());
    assert_eq!(frame.actors[2].tp, 62);
    assert!(frame.cues.contains(&Cue::Interrupted {
        action: ActionId(1)
    }));
    assert!(!frame.cues.iter().any(|c| matches!(c, Cue::Released { .. })));
    assert!(battle.step(request(0))?.scenes[0].remaining.is_some());
    Ok(())
}

#[test]
fn active_scene_survives_caster_completion_and_retains_only_its_resident() -> Result<()> {
    let mut battle = battle(SHORT)?;
    battle.step(request(2))?;
    battle.step(BattleInput::default())?;
    let released = battle.step(BattleInput::default())?;
    let resident = released
        .cues
        .iter()
        .find_map(|c| match c {
            Cue::Released { action, .. } => Some(*action),
            _ => None,
        })
        .unwrap();
    let frame = battle.step(BattleInput::default())?;
    assert!(frame.cues.contains(&Cue::Completed {
        action: ActionId(1)
    }));
    assert_eq!(frame.scenes.len(), 1);
    assert!(frame.models.iter().all(|m| !m.visible));
    let frame = battle.step(BattleInput {
        interrupt: vec![resident],
        ..Default::default()
    })?;
    assert!(frame.scenes.is_empty());
    assert!(frame.models.iter().all(|m| m.visible));
    Ok(())
}

#[test]
fn stored_slots_keep_independent_owners_and_reuse_only_cleaned_slots() -> Result<()> {
    let mut battle = battle(SHORT)?;
    battle.step(request(2))?;
    battle.step(BattleInput::default())?;
    let first = battle.step(BattleInput::default())?;
    let resident = first
        .cues
        .iter()
        .find_map(|c| match c {
            Cue::Released { action, .. } => Some(*action),
            _ => None,
        })
        .unwrap();
    let second = battle.step(request(0))?;
    assert_eq!(
        second
            .scenes
            .iter()
            .map(|s| (s.slot, s.actor, s.remaining))
            .collect::<Vec<_>>(),
        [(0, ActorId(2), None), (1, ActorId(0), Some(1))]
    );
    let parent = second
        .cues
        .iter()
        .find_map(|c| match c {
            Cue::Started { action, .. } => Some(*action),
            _ => None,
        })
        .unwrap();
    let interrupted = battle.step(BattleInput {
        interrupt: vec![parent],
        ..Default::default()
    })?;
    assert_eq!(interrupted.scenes.len(), 1);
    assert_eq!(interrupted.scenes[0].slot, 0);
    assert!(battle.sequences.contains_key(&resident));
    assert_eq!(battle.step(request(1))?.scenes[1].remaining, Some(1));
    battle.step(BattleInput::default())?;
    let both = battle.step(BattleInput::default())?;
    assert!(both.scenes.iter().all(|s| s.remaining.is_none()));
    assert!(!battle.scene_available());
    let cleaned = battle.step(BattleInput {
        interrupt: vec![resident],
        ..Default::default()
    })?;
    assert_eq!(cleaned.scenes.len(), 1);
    assert_eq!(cleaned.scenes[0].slot, 1);
    let reused = battle.step(request(0))?;
    assert_eq!(reused.scenes[0].slot, 0);
    assert_eq!(reused.scenes[0].actor, ActorId(0));
    assert_eq!(reused.scenes[0].remaining, Some(1));
    assert_eq!(reused.scenes[1].remaining, None);
    Ok(())
}

#[test]
fn invalid_scene_calls_fault_and_clear_owned_slots() -> Result<()> {
    for body in [
        "battle::begin_scene(spell, ticks(0));",
        "battle::activate_scene();",
        "battle::hide_actors();",
        "battle::scene_remaining();",
        "battle::begin_scene(spell, ticks(2)); battle::activate_scene();",
        "battle::begin_scene(spell, ticks(2)); battle::begin_scene(spell, ticks(2));",
        "battle::begin_scene(spell, ticks(2)); await battle::recover(ticks(2));",
    ] {
        let mut battle = battle(body)?;
        assert!(battle.step(request(2)).is_err(), "{body}");
        assert!(battle.scenes.iter().all(Option::is_none));
        assert!(battle.sequences.is_empty());
        assert!(battle.step(BattleInput::default()).is_err());
    }
    Ok(())
}

#[test]
fn actor_tint_approaches_prepared_ambient_and_retains_alpha() -> Result<()> {
    let prepared = Arc::try_unwrap(battle("")?.prepared)
        .unwrap()
        .with_ambient_color([0, 128, 255]);
    let mut battle = Battle::new(Arc::new(prepared));
    let initial = battle.step(BattleInput {
        menu_open: true,
        ..Default::default()
    })?;
    assert_eq!(initial.actors[0].body.tint, [0, 128, 255, 255]);
    assert_eq!(initial.models[0].tint, initial.actors[0].body.tint);
    for actor in &mut battle.actors[..3] {
        actor.body.tint = [2, 130, 253, 17];
    }
    battle.actors[1].petrified = true;
    battle.actors[2].hp = 0;
    for (step, expected) in [[1, 129, 254, 17], [0, 128, 255, 17], [0, 128, 255, 17]]
        .into_iter()
        .enumerate()
    {
        let before = battle.actors[0].body.tint;
        let frame = battle.step(BattleInput::default())?;
        assert_eq!(frame.actors[0].body.tint, expected, "update {step}");
        assert_eq!(frame.models[0].tint, before);
        assert_eq!(frame.actors[1].body.tint, [2, 130, 253, 17]);
        assert_eq!(frame.actors[2].body.tint, [2, 130, 253, 17]);
    }
    Ok(())
}

#[test]
fn stage_color_outlives_its_requesting_action_and_menu_holds_its_clock() -> Result<()> {
    let mut battle = battle(
        r#"
        battle::tint_stage(0, battle::Color { red: 16, green: 16, blue: 16, alpha: 255 }, ticks(3), 4);
        await battle::at_age(ticks(100));
    "#,
    )?;
    let first = battle.step(request(0))?;
    assert_eq!(first.stage_colors[0], Some([60, 60, 60, 255]));
    let frame = battle.step(BattleInput {
        interrupt: vec![ActionId(1)],
        ..Default::default()
    })?;
    assert_eq!(frame.stage_colors[0], Some([56, 56, 56, 255]));
    assert_eq!(battle.stage_colors.channels[0].remaining, 1);
    let paused = battle.step(BattleInput {
        menu_open: true,
        ..Default::default()
    })?;
    assert_eq!(paused.stage_colors, frame.stage_colors);
    assert_eq!(battle.stage_colors.channels[0].remaining, 1);
    let expired = battle.step(BattleInput::default())?;
    assert_eq!(expired.stage_colors[0], Some([58, 58, 58, 255]));
    assert_eq!(battle.stage_colors.channels[0].remaining, 0);
    Ok(())
}

#[test]
fn stage_color_calls_validate_channel_color_duration_and_step() -> Result<()> {
    for (channel, red, duration, step, message) in [
        (-1, 16, 3, 4, "stage color channel"),
        (2, 16, 3, 4, "stage color channel"),
        (0, -1, 3, 4, "stage color must"),
        (0, 256, 3, 4, "stage color must"),
        (0, 16, 32768, 4, "stage color duration"),
        (0, 16, 3, -1, "stage color step"),
        (0, 16, 3, 256, "stage color step"),
    ] {
        let mut battle = battle(&format!(
            r#"
            battle::tint_stage({channel}, battle::Color {{ red: {red}, green: 16, blue: 16, alpha: 255 }}, ticks({duration}), {step});
        "#
        ))?;
        assert!(
            battle
                .step(request(0))
                .unwrap_err()
                .to_string()
                .contains(message)
        );
    }
    Ok(())
}

fn observed_camera(row: &serde_json::Value) -> crate::CameraPose {
    let scalar = |name: &str| {
        f32::from_bits(row[format!("battle_camera_{name}_word")].as_u64().unwrap() as u32)
    };
    let vector = |name: &str| {
        ["x", "y", "z"].map(|axis| {
            f32::from_bits(
                row[format!("battle_camera_output_{name}_{axis}_bits")]
                    .as_u64()
                    .unwrap() as u32,
            )
        })
    };
    crate::CameraPose {
        eye: vector("eye"),
        focus: vector("focus"),
        pitch: scalar("pitch"),
        yaw: scalar("yaw"),
        radius: scalar("radius"),
    }
}

#[test]
fn camera_bounds_merge_outlive_actions_and_pause_with_the_menu() -> Result<()> {
    let mut battle = battle(
        r#"
        battle::camera_bounds(ticks(3), 2600.0, 12.0);
        battle::camera_bounds(ticks(2), 2400.0, 14.0);
        await battle::at_age(ticks(100));
    "#,
    )?;
    battle.step(request(0))?;
    let state = battle.camera.as_ref().unwrap();
    assert_eq!(
        (state.remaining, state.minimum_radius, state.minimum_pitch),
        (3, 2600., 14.)
    );
    let initial = state.pose;
    let held = battle.step(BattleInput {
        menu_open: true,
        ..Default::default()
    })?;
    assert_eq!(held.camera, Some(initial));
    assert_eq!(battle.camera.as_ref().unwrap().remaining, 3);
    battle.step(BattleInput {
        interrupt: vec![ActionId(1)],
        ..Default::default()
    })?;
    assert_eq!(battle.camera.as_ref().unwrap().remaining, 2);
    battle.step(BattleInput::default())?;
    let expired = battle.step(BattleInput::default())?;
    assert_eq!(battle.camera.as_ref().unwrap().remaining, 0);
    assert!(expired.camera.unwrap().pitch > initial.pitch);
    battle.step(request(1))?;
    assert_eq!(battle.camera.as_ref().unwrap().remaining, 3);
    Ok(())
}

#[test]
fn interrupted_scene_returns_camera_without_a_fixed_return_duration() -> Result<()> {
    let mut battle = battle(SHORT)?;
    battle.actors[2].position = [-700., 0., 150.];
    battle.step(request(2))?;
    battle.step(BattleInput {
        interrupt: vec![ActionId(1)],
        ..Default::default()
    })?;
    assert_eq!(
        battle.camera.as_ref().unwrap().phase,
        crate::camera::Phase::Returning
    );
    for _ in 0..100 {
        battle.step(BattleInput::default())?;
        if battle.camera.as_ref().unwrap().phase == crate::camera::Phase::Tracking {
            return Ok(());
        }
    }
    panic!("interrupted scene never returned to ordinary camera tracking");
}

#[test]
fn camera_calls_reject_invalid_duration_radius_and_missing_preparation() -> Result<()> {
    for (duration, radius, message) in [
        (-1, "1950.0", "does not match Ticks"),
        (32768, "1950.0", "camera constraint duration"),
        (3, "-1.0", "invalid camera constraint"),
    ] {
        let mut battle = battle(&format!(
            "battle::camera_bounds(ticks({duration}), {radius}, 8.0);"
        ))?;
        let error = battle.step(request(0)).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
        assert!(battle.sequences.is_empty());
    }
    let mut battle = battle("battle::camera_bounds(ticks(2), 1950.0, 8.0);")?;
    battle.camera = None;
    assert!(
        battle
            .step(request(0))
            .unwrap_err()
            .to_string()
            .contains("camera is not prepared")
    );
    Ok(())
}
