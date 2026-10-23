use super::*;
use crate::tests::{actor, prepare_actors, step_idle};

fn actors() -> Vec<Actor> {
    let mut leader = actor(crate::Side::Party);
    leader.control = Control::Manual;
    let mut target = actor(crate::Side::Enemy);
    target.position = [400., 0., 0.];
    let mut other = target.clone();
    other.position = [-4000., 0., 0.];
    for actor in [&mut leader, &mut target, &mut other] {
        actor.body.collider = None;
        actor.body.center_offset = [0., 60., 0.];
    }
    vec![leader, target, other]
}

fn definition(auto_zoom: bool) -> CameraDefinition {
    CameraDefinition {
        leader: ActorId(0),
        stage_pitch: 0.,
        auto_zoom,
        yaw_limits: None,
    }
}

fn camera(actors: &[Actor], auto_zoom: bool) -> Camera {
    Camera::new(definition(auto_zoom), actors, ActorId(1)).unwrap()
}

#[test]
fn arena_yaw_limits_keep_entry_tracking_and_selection_in_front_of_wall() -> Result<()> {
    let mut actors = actors();
    actors.truncate(2);
    actors[0].position = [-300., 0., 0.];
    actors[1].position = [850., 0., 0.];
    let mut camera = Camera::new(
        CameraDefinition {
            yaw_limits: Some([102., 256.]),
            ..definition(true)
        },
        &actors,
        ActorId(1),
    )?;
    assert_eq!(camera.pose.yaw, 102.);
    for entry in 0..=crate::prepare::ENTRY_PAN_UPDATES {
        camera.frame_entry(entry);
        assert!(camera.frame().eye[0] < camera.frame().focus[0]);
    }
    for control in [Control::Manual, Control::Auto] {
        actors[0].control = control;
        for step in 0..720 {
            let angle = (step as f32).to_radians();
            actors[0].position = [500. * angle.cos(), 0., 500. * angle.sin()];
            let update = if step % 60 < 10 {
                Update::Selecting(ActorId(1))
            } else {
                Update::Tracking
            };
            camera.step(&actors, ActorId(1), false, update)?;
            assert!((102. ..=256.).contains(&camera.pose.yaw));
            assert!(camera.frame().eye[0] < actors[1].position[0]);
        }
    }
    Ok(())
}

#[test]
fn controlled_camera_initializes_before_first_tracking_update() -> Result<()> {
    let mut actors = actors();
    actors.truncate(2);
    actors[0].position = [-300., 0., 0.];
    actors[1].position = [350., 0., 0.];
    let mut camera = camera(&actors, true);
    let observed_heights = [
        575.791_8, 611.791_8, 640.5918, 663.6318, 682.0638, 695.791_8,
    ];
    for (update, expected) in observed_heights.into_iter().enumerate() {
        if update != 0 {
            camera.step(&actors, ActorId(1), false, Update::Tracking)?;
        }
        assert!((camera.frame().eye[1] - expected).abs() < 0.003);
        assert_eq!(camera.frame().focus, [25., 120., 0.]);
    }
    Ok(())
}

#[test]
fn controlled_camera_matches_recorded_combat_updates() -> Result<()> {
    #[derive(serde::Deserialize)]
    struct Sample {
        positions: [[f32; 3]; 2],
        eye: [f32; 3],
        focus: [f32; 3],
    }
    #[derive(serde::Deserialize)]
    struct Recording {
        name: String,
        enemy_count: usize,
        auto_zoom: bool,
        stage_pitch: f32,
        previous_positions: [[f32; 3]; 2],
        radius: f32,
        yaw: f32,
        samples: Vec<Sample>,
    }
    let recordings: Vec<Recording> = serde_json::from_str(include_str!("recorded-combat.json"))?;
    for recording in recordings {
        let mut actors = actors();
        actors.resize(recording.enemy_count + 1, actors[1].clone());
        for (actor, position) in actors.iter_mut().zip(recording.previous_positions) {
            actor.position = position;
        }
        let mut camera = Camera::new(
            CameraDefinition {
                stage_pitch: recording.stage_pitch,
                ..definition(recording.auto_zoom)
            },
            &actors,
            ActorId(1),
        )?;
        camera.pose.radius = recording.radius;
        camera.pose.yaw = recording.yaw;
        camera.pose.eye = controlled_eye(camera.pose);
        for (tick, sample) in recording.samples.iter().enumerate() {
            for (actor, &position) in actors.iter_mut().zip(&sample.positions) {
                actor.position = position;
            }
            camera.step(&actors, ActorId(1), false, Update::Tracking)?;
            for (actual, expected) in camera
                .pose
                .eye
                .into_iter()
                .chain(camera.pose.focus)
                .zip(sample.eye.into_iter().chain(sample.focus))
            {
                assert!(
                    (actual - expected).abs() < 0.003,
                    "{} sample {tick}: {actual} != {expected}",
                    recording.name
                );
            }
        }
    }
    Ok(())
}

#[test]
fn selection_uses_explicit_target_while_gameplay_is_held() -> Result<()> {
    let actors = actors();
    let mut camera = camera(&actors, false);
    let before = camera.pose;
    camera.step(&actors, ActorId(1), false, Update::Held)?;
    assert_eq!(camera.pose, before);
    camera.step(&actors, ActorId(1), false, Update::Selecting(ActorId(2)))?;
    assert!(camera.pose.focus[0] < before.focus[0]);
    Ok(())
}

#[test]
fn controlled_camera_keeps_its_viewing_side_and_catches_up_to_the_combat_line() -> Result<()> {
    let mut actors = actors();
    let mut camera = camera(&actors, true);
    let initial_yaw = camera.pose.yaw;
    actors[1].position = [-400., 0., 0.];
    camera.step(&actors, ActorId(1), false, Update::Tracking)?;
    assert_eq!(
        camera.pose.yaw, initial_yaw,
        "crossing must not orbit the camera"
    );

    actors[1].position = [400., 0., 400.];
    camera.step(&actors, ActorId(1), false, Update::Tracking)?;
    assert!((camera.pose.yaw - initial_yaw) > 1.);
    assert!((camera.pose.yaw - initial_yaw) < 5.);
    for _ in 0..90 {
        camera.step(&actors, ActorId(1), false, Update::Tracking)?;
    }
    assert!((camera.pose.yaw - (VIEW_ANGLE + 45.)).abs() < 15.);
    Ok(())
}

#[test]
fn retargeting_pans_without_a_selection_or_tracking_jump() -> Result<()> {
    let actors = actors();
    for auto_zoom in [false, true] {
        let mut camera = camera(&actors, auto_zoom);
        // Retarget an already settled view; entry easing is checked separately.
        camera.pose.eye = controlled_eye(camera.pose);
        let before = camera.pose;
        camera.step(&actors, ActorId(2), true, Update::Tracking)?;
        assert!(distance::length(sub(camera.pose.focus, before.focus)) <= MAXIMUM_FOCUS_SPEED);
        assert!(distance::length(sub(camera.pose.eye, before.eye)) <= MAXIMUM_FOCUS_SPEED);

        // A held target selector and its release use the same eye geometry as
        // ordinary tracking; changing mode must not introduce a vertical cut.
        for update in [Update::Selecting(ActorId(1)), Update::Tracking] {
            let before = camera.pose;
            camera.step(&actors, ActorId(1), false, update)?;
            assert!(distance::length(sub(camera.pose.focus, before.focus)) <= MAXIMUM_FOCUS_SPEED);
            assert!((camera.pose.eye[1] - before.eye[1]).abs() < 40.);
        }
    }
    Ok(())
}

#[test]
fn leader_death_and_revival_ease_between_controlled_and_group_views() -> Result<()> {
    for auto_zoom in [false, true] {
        let mut subjects = actors();
        let mut companion = subjects[0].clone();
        companion.control = Control::Auto;
        companion.position = [-2000., 0., 0.];
        subjects.push(companion);
        let mut battle = prepare_actors(subjects, 1)
            .with_camera(definition(auto_zoom))?
            .finish()?;
        battle.set_decision_target(ActorId(0), ActorId(1))?;
        step_idle(&mut battle, 120)?;
        let controlled = battle.frame(vec![], None).camera.unwrap();
        battle.actors[0].hp = 0;
        battle.enter_death(ActorId(0), &mut vec![]);
        let group = camera(battle.actors(), auto_zoom).frame();
        let maximum_eye_step = distance::length(sub(group.eye, controlled.eye)) * 0.1;
        for reviving in [false, true] {
            if reviving {
                crate::tests::revive(&mut battle, ActorId(0), 100, &mut vec![])?;
            }
            let mut previous = battle.frame(vec![], None).camera.unwrap();
            let held = battle.step(crate::BattleInput {
                paused: true,
                ..Default::default()
            })?;
            assert_eq!(held.camera, Some(previous));
            for update in 0..120 {
                let frame = battle.step(Default::default())?;
                let current = frame.camera.unwrap();
                let eye_step = distance::length(sub(current.eye, previous.eye));
                assert!(
                    eye_step < maximum_eye_step,
                    "auto_zoom={auto_zoom}, reviving={reviving}, update={update}: eye jumped {eye_step}"
                );
                assert!(
                    distance::length(sub(current.focus, previous.focus))
                        <= MAXIMUM_FOCUS_SPEED + 0.001
                );
                previous = current;
            }
            let destination = if reviving { controlled } else { group };
            assert!(distance::length(sub(previous.eye, destination.eye)) < 1.);
            assert_eq!(battle.phase(), crate::BattlePhase::Combat);
        }
    }
    Ok(())
}

#[test]
fn results_camera_matches_recorded_victory_transition() {
    // Eye positions sampled at result-scene ages in the opening Iselia battle.
    let samples = [
        (0, [2165.064, 308.6477, -1249.9991]),
        (20, [2165.064, 308.6477, -1249.9991]),
        (21, [2121.228, 304.72473, -1225.9247]),
        (29, [1801.0105, 276.03772, -1049.265]),
        (49, [1203.481, 222.34271, -715.2856]),
        (110, [1184.0175, 222.34271, -747.0627]),
        (230, [1143.2941, 222.34271, -808.009]),
        (409, [1076.7684, 222.34271, -894.74567]),
    ];
    let mut camera = camera(&actors(), false);
    let mut next_tick = 0;
    for (tick, eye) in samples {
        while next_tick <= tick {
            camera.frame_results();
            next_tick += 1;
        }
        assert_eq!(camera.pose.focus, [0., 112.5, 0.]);
        for (actual, expected) in camera.pose.eye.into_iter().zip(eye) {
            assert!(
                (actual - expected).abs() < 0.012,
                "result tick {tick}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn result_party_matches_recorded_three_and_four_member_victories() -> Result<()> {
    for (celebrant, positions) in [
        (2, vec![[200., 0., 0.], [-196.96156, 0., -34.7295], [0.; 3]]),
        (1, vec![[200., 0., 0.], [0.; 3], [-196.96156, 0., -34.7295]]),
        (
            1,
            vec![
                [200., 0., 0.],
                [0.; 3],
                [-128.55745, 0., 153.20895],
                [-34.72981, 0., -196.96152],
            ],
        ),
    ] {
        let mut battle =
            prepare_actors(vec![actor(crate::Side::Party); positions.len()], 1).finish()?;
        battle.terminal.result = Some(crate::BattleResult::Victory);
        battle.retire_combat()?;
        battle.arrange_result_actors(ActorId(celebrant))?;
        for (actor, position) in battle.actors().iter().zip(positions) {
            crate::tests::assert_vector_close(actor.position, position, 0.001);
        }
    }
    Ok(())
}

#[test]
fn stored_spell_framing_matches_recorded_casts_and_returns() -> Result<()> {
    #[derive(serde::Deserialize)]
    struct Sample {
        age: u32,
        view: [f32; 6],
    }
    #[derive(serde::Deserialize)]
    struct Recording {
        name: String,
        initial: [f32; 6],
        stage_pitch: f32,
        caster: [f32; 3],
        samples: Vec<Sample>,
    }
    let recordings: Vec<Recording> =
        serde_json::from_str(include_str!("recorded-stored-spells.json"))?;
    for recording in recordings {
        let ordinary = CameraPose {
            eye: recording.initial[..3].try_into().unwrap(),
            focus: recording.initial[3..].try_into().unwrap(),
            pitch: 0.,
            yaw: 0.,
            radius: 0.,
        };
        let mut shot = spell::Shot::new(ordinary);
        let mut age = 0;
        for sample in recording.samples {
            while age < sample.age {
                age += 1;
                if age <= 60 {
                    shot.cast(recording.caster, recording.stage_pitch);
                } else {
                    shot.return_to(ordinary);
                }
            }
            let frame = shot.frame();
            for (actual, expected) in frame.eye.into_iter().chain(frame.focus).zip(sample.view) {
                assert!(
                    (actual - expected).abs() < 0.003,
                    "{} at {age}: {actual} != {expected}",
                    recording.name
                );
            }
        }
    }
    Ok(())
}

#[test]
fn active_scene_camera_starts_at_initialization_holds_then_restores_tracking() -> Result<()> {
    use resonance_content::battle_scene::CameraBounds;
    let mut actors = actors();
    actors.truncate(2);
    actors[1].position = [100., 0., 0.];
    let mut battle = prepare_actors(actors.clone(), 1)
        .with_camera(definition(true))?
        .finish()?;
    let framing = SpellCamera {
        bounds: CameraBounds {
            minimum_distance: 2450.,
            minimum_elevation: 12.,
        },
        duration: 170,
    };
    let mut definition = crate::tests::volley();
    definition.transition_ticks = 1;
    definition.camera = Some(framing);
    definition.duration = 220;
    definition.projectiles.truncate(1);
    definition.projectiles[0].at = 200;
    definition.validate()?;
    let id = battle
        .release_volley(
            std::sync::Arc::new(definition),
            ActorId(0),
            ActorId(1),
            crate::SpellSlot::Primary,
            None,
            &mut vec![],
        )?
        .unwrap();
    let mut volley = battle.volleys.remove(&id).unwrap();
    for _ in 0..2 {
        volley.step(&mut battle, id, &mut vec![])?;
        assert!(battle.camera.as_ref().unwrap().scene.is_none());
    }
    volley.step(&mut battle, id, &mut vec![])?;
    let camera = battle.camera.as_mut().unwrap();
    assert_eq!(camera.scene, Some(framing));
    let held = camera.frame();
    camera.step(&actors, ActorId(1), false, Update::Held)?;
    assert_eq!(camera.frame(), held);
    for _ in 0..60 {
        camera.step(&actors, ActorId(1), false, Update::Spell(ActorId(1)))?;
    }
    assert_eq!(camera.scene, Some(framing));
    for _ in 0..60 {
        camera.step(&actors, ActorId(1), false, Update::Tracking)?;
    }
    assert_eq!((camera.pose.radius, camera.pose.pitch), (2450., 12.));
    assert_eq!(camera.scene.unwrap().duration, 110);
    camera.step(&actors, ActorId(1), false, Update::Selecting(ActorId(1)))?;
    assert_eq!(camera.scene.unwrap().duration, 110);
    camera.request_scene(SpellCamera {
        bounds: CameraBounds {
            minimum_distance: 2600.,
            minimum_elevation: 13.,
        },
        duration: 20,
    });
    for _ in 0..110 {
        camera.step(&actors, ActorId(1), false, Update::Tracking)?;
    }
    assert_eq!((camera.pose.radius, camera.pose.pitch), (2600., 13.));
    assert_eq!(camera.scene.unwrap().duration, 0);
    for _ in 0..120 {
        camera.step(&actors, ActorId(1), false, Update::Tracking)?;
    }
    assert!(camera.scene.is_none());
    assert_eq!((camera.pose.radius, camera.pose.pitch), (2050., 8.));
    camera.request_scene(framing);
    assert_eq!(camera.scene, Some(framing));
    Ok(())
}

#[test]
fn target_settling_waits_for_actor_updates_and_then_resumes_normal_tracking() -> Result<()> {
    let mut subjects = actors();
    subjects[1].position = [100., 0., 0.];
    subjects[2].position = [200., 0., 0.];
    let mut battle = prepare_actors(subjects, 1)
        .with_camera(definition(true))?
        .finish()?;
    battle.set_decision_target(ActorId(0), ActorId(2))?;
    battle.camera.as_mut().unwrap().pose.focus = [75., FOCUS_HEIGHT, 0.];
    for _ in 0..150 {
        battle.step(crate::BattleInput {
            paused: true,
            ..Default::default()
        })?;
    }
    assert_eq!(battle.camera.as_ref().unwrap().pose.focus[0], 75.);
    battle.step(Default::default())?;
    assert_eq!(battle.camera.as_ref().unwrap().pose.focus[0], 81.25);
    for _ in 1..crate::state::TARGET_SETTLE_UPDATES {
        battle.step(Default::default())?;
    }
    battle.camera.as_mut().unwrap().pose.focus = [75., FOCUS_HEIGHT, 0.];
    battle.step(Default::default())?;
    assert_eq!(battle.camera.as_ref().unwrap().pose.focus[0], 85.375);
    Ok(())
}
