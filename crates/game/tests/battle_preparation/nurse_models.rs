use super::{
    actor,
    nurse_trails::{Visit, assert_particle},
};
use anyhow::{Context, Result};
use resonance_battle::{
    ActionDefinition, ActionPhase, ActionRequest, Battle, BattleInput, EffectBank,
    EffectModelDefinition, PreparedBattle, PreparedEffectModel, ResourceBinding, Side,
    SoundBinding, VoiceLine,
};
use resonance_content::{
    animation::{Motion, Skeleton},
    battle_effect::SourceBank,
    secondary_motion::Definition,
};
use resonance_game::battle::effect_program;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Deserialize)]
struct Particle {
    model: u8,
    render_model: u32,
    visits: Vec<Visit>,
}
#[derive(Deserialize)]
struct Fixture {
    bank: SourceBank,
    owner_heading_bits: u32,
    particles: Vec<Particle>,
}
#[derive(Deserialize)]
struct ObservedModel {
    skeleton: Skeleton,
    secondary_motion: Definition,
    observations: Vec<ObservedPose>,
}
#[derive(Deserialize)]
struct ObservedPose {
    model: u32,
    tick: u32,
    after: ObservedClock,
    matrices_after: Option<Vec<[u32; 12]>>,
}
#[derive(Deserialize)]
struct ObservedClock {
    frame_bits: u32,
}

fn prepare(fixture: &Fixture, model: &ObservedModel, party: usize) -> Result<PreparedBattle> {
    let compiled = symphonia_script_compiler::compile(
        "test",
        &BTreeMap::from([
            (
                "test".into(),
                r#"
            script battle;
            use battle;
            use battle::nurse;
            asset spell: battle::Spell = "test/nurse";
            pub task run() {
                battle::begin_scene(spell, ticks(60));
                while battle::scene_remaining() != ticks(0) {
                    await battle::next_update();
                }
                battle::activate_scene();
                // This launcher only supplies the captured resident lifetime.
                // The scene callback holds age on this activation visit;
                // wait90 therefore keeps the old recovery completion visit.
                // actor recovery/guard RNG belongs to the casting fixtures.
                await battle::wait_ticks(ticks(90));
                battle::finish();
            }
            pub task resident() { await nurse::run(); }
        "#
                .into(),
            ),
            (
                "battle::nurse".into(),
                include_str!("../../../../scripts/battle/nurse.sym").into(),
            ),
        ]),
        &resonance_battle::native_declarations(),
    )?;
    let resources: Vec<_> = compiled
        .assets
        .iter()
        .map(|asset| match asset.kind.as_str() {
            "battle::Voice" => ResourceBinding::Voice(
                (0..=party)
                    .map(|i| {
                        (i < party).then_some(VoiceLine {
                            sound: SoundBinding {
                                resource: 1,
                                index: 43 + i as u16,
                            },
                            duration: 0,
                        })
                    })
                    .collect(),
            ),
            "battle::ActorTints" => super::actor_tints(),
            "battle::Spell" => ResourceBinding::Spell(237),
            "battle::Effect" => ResourceBinding::Effect(237),
            _ => panic!("unexpected model fixture binding"),
        })
        .collect();
    let program = Arc::new(compiled.program);
    let actions = [
        (99, ActionPhase::Casting, "test::run", 500),
        (237, ActionPhase::Resident, "test::resident", 250),
    ]
    .into_iter()
    .map(|(id, phase, name, duration)| -> Result<_> {
        Ok(ActionDefinition {
            id,
            phase,
            entry: program
                .authored()
                .unwrap()
                .functions
                .iter()
                .find(|f| f.name == name)
                .context("missing fixture entry")?
                .entry,
            program: program.clone(),
            resources: resources.clone(),
            duration,
            tp_cost: 0,
        })
    })
    .collect::<Result<_>>()?;
    let motion = Motion::decode(include_bytes!(
        "../../../battle/tests/fixtures/nurse.motion"
    ))?;
    let names: Vec<_> = model
        .skeleton
        .bones
        .iter()
        .map(|bone| bone.name.clone())
        .collect();
    let models = (0..4)
        .map(|slot| {
            Ok((
                slot,
                PreparedEffectModel::new(Arc::new(EffectModelDefinition {
                    resource: 700 + u32::from(slot),
                    skeleton: model.skeleton.clone(),
                    motions: BTreeMap::from([(0, motion.clone())]),
                    secondary_motion: model.secondary_motion.prepare(&names)?,
                }))?,
            ))
        })
        .collect::<Result<_>>()?;
    let members = (1..=6)
        .map(|member| {
            Ok((
                member,
                Arc::new(effect_program::prepare(
                    &fixture.bank.program(usize::from(member))?,
                    237,
                    member,
                    &mut |index| Ok(resonance_battle::SoundBinding { resource: 1, index }),
                )?),
            ))
        })
        .collect::<Result<_>>()?;
    let mut actors = vec![actor(); party];
    actors[2].heading = f32::from_bits(fixture.owner_heading_bits);
    let mut enemy = actor();
    enemy.side = Side::Enemy;
    actors.push(enemy);
    let prepared = PreparedBattle::new(
        actors,
        actions,
        1,
        vec![],
        vec![EffectBank {
            resource: 237,
            models,
            members,
        }],
    )?;
    let ids: Vec<_> = prepared.actor_ids().collect();
    prepared.with_camera(resonance_battle::CameraDefinition {
        leader: ids[0],
        target: ids[party],
        stage_pitch: -1.,
        adaptive: true,
        initial: resonance_battle::CameraPose {
            eye: [0., 490., 2050.],
            focus: [0., 120., 0.],
            pitch: 7.,
            yaw: 90.,
            radius: 2050.,
        },
    })
}

fn fixtures() -> Result<(Fixture, ObservedModel)> {
    Ok((
        serde_json::from_str(include_str!("../fixtures/nurse-model-particles.json"))?,
        serde_json::from_str(include_str!(
            "../../../battle/tests/fixtures/nurse-scene-models.json"
        ))?,
    ))
}

#[test]
fn maintained_nurse_models_match_original_emission_motion_playback_and_expiry() -> Result<()> {
    let (fixture, poses) = fixtures()?;
    let prepared = Arc::new(prepare(&fixture, &poses, 3)?);
    let actors: Vec<_> = prepared.actor_ids().collect();
    let mut battle = Battle::new(prepared);
    let mut owner = None;
    let mut finished = false;
    let mut visits = 0;
    let mut matrices = 0;
    let mut maximum = 0f32;
    // Source transition begins at316, activates376 and initializes the resident
    // at377. Its ordinary-group model particles first update/draw at378.
    for tick in 316..=559 {
        let input = if tick == 316 {
            BattleInput {
                actions: vec![ActionRequest {
                    actor: actors[2],
                    target: actors[3],
                    action: 99,
                }],
                ..Default::default()
            }
        } else {
            BattleInput::default()
        };
        let frame = battle.step(input)?;
        for cue in &frame.cues {
            match cue {
                resonance_battle::Cue::Started { action, .. } if tick == 316 => {
                    owner = Some(*action)
                }
                resonance_battle::Cue::Completed { action } if Some(*action) == owner => {
                    finished = true
                }
                _ => {}
            }
        }
        if (378..=558).contains(&tick) {
            assert_eq!(
                frame.particles.iter().filter(|p| p.model.is_some()).count(),
                3,
                "tick {tick}"
            );
            for expected in &fixture.particles {
                let actual = frame
                    .particles
                    .iter()
                    .find(|p| {
                        p.model
                            .as_ref()
                            .is_some_and(|m| m.resource == 700 + u32::from(expected.model))
                    })
                    .context("missing model particle")?;
                let visit = &expected.visits[(tick - 378) as usize];
                assert_eq!(visit.combat_tick, tick);
                assert_eq!(actual.age, (tick - 378) as i16);
                assert_particle(actual, visit)?;
                assert_eq!(visit.random_before, visit.random_after);
                visits += 1;
                let model = actual.model.as_ref().unwrap();
                if let Some(observed) = poses
                    .observations
                    .iter()
                    .find(|o| o.model == expected.render_model && o.tick == tick)
                {
                    assert_eq!(model.frame.to_bits(), observed.after.frame_bits);
                    if let Some(expected) = &observed.matrices_after {
                        for (bone, expected) in model.bones.iter().zip(expected) {
                            let world = resonance_content::animation::multiply(model.world, *bone);
                            for i in 0..12 {
                                let error =
                                    (world[i % 4][i / 4] - f32::from_bits(expected[i])).abs();
                                maximum = maximum.max(error);
                                assert!(error < 0.001, "tick {tick}: matrix error {error}");
                            }
                        }
                        matrices += 1;
                    }
                }
            }
            if tick >= 500 {
                assert!(finished);
            }
        } else {
            assert!(
                frame.particles.iter().all(|p| p.model.is_none()),
                "tick {tick}"
            );
        }
        if tick == 450 {
            let held = battle.step(BattleInput {
                menu_open: true,
                ..Default::default()
            })?;
            assert_eq!(held.particles, frame.particles);
            assert_eq!(held.update, frame.update);
        }
        if tick < 498 {
            assert_eq!(battle.random_state(), 1);
        }
    }
    assert_eq!(visits, 543);
    assert_eq!(matrices, 21);
    eprintln!(
        "Nurse models: {visits} particle visits, {matrices} full poses, maximum matrix error {maximum}"
    );
    Ok(())
}

#[test]
fn overlapping_scenes_hold_models_and_cancel_only_their_own_instances() -> Result<()> {
    use resonance_battle::{ActorId, Cue};
    let (fixture, poses) = fixtures()?;
    let prepared = Arc::new(prepare(&fixture, &poses, 4)?);
    let actors: Vec<_> = prepared.actor_ids().collect();
    let mut battle = Battle::new(prepared);
    let request = |actor: ActorId| BattleInput {
        actions: vec![ActionRequest {
            actor,
            target: actors[4],
            action: 99,
        }],
        ..Default::default()
    };
    let mut frame = battle.step(request(actors[2]))?;
    for _ in 0..80 {
        frame = battle.step(BattleInput::default())?;
    }
    assert_eq!(frame.particles.len(), 4);
    let original = frame.particles.clone();
    frame = battle.step(request(actors[0]))?;
    assert_eq!(frame.scenes.len(), 2);
    assert_eq!(frame.particles, original);
    while frame
        .scenes
        .iter()
        .find(|scene| scene.actor == actors[0])
        .unwrap()
        .remaining
        != Some(0)
    {
        frame = battle.step(BattleInput::default())?;
        assert_eq!(frame.particles, original);
    }
    frame = battle.step(BattleInput::default())?;
    let second = frame
        .cues
        .iter()
        .find_map(|cue| match cue {
            Cue::Released { action, actor, .. } if *actor == actors[0] => Some(*action),
            _ => None,
        })
        .context("second resident was not released")?;
    // Activation clears the model hold after object dispatch. Existing particles
    // retain their age on this visit, but their sampled model clock advances.
    for (particle, held) in frame.particles.iter().zip(&original) {
        assert_eq!(particle.age, held.age);
        assert_ne!(
            particle.model.as_ref().unwrap().frame,
            held.model.as_ref().unwrap().frame
        );
    }
    battle.step(BattleInput::default())?; // Resident initialization emits models.
    frame = battle.step(BattleInput::default())?;
    assert_eq!(frame.particles.len(), 8);
    for model in 700..704 {
        let find = |owner| {
            frame
                .particles
                .iter()
                .find(|p| p.owner == owner && p.model.as_ref().is_some_and(|m| m.resource == model))
                .unwrap()
        };
        let old = find(actors[2]);
        let new = find(actors[0]);
        assert!(old.age > new.age);
        assert_eq!(new.age, 0);
        assert_eq!(new.model.as_ref().unwrap().frame, 0.5);
        assert_ne!(
            old.model.as_ref().unwrap().world,
            new.model.as_ref().unwrap().world
        );
    }
    frame = battle.step(BattleInput {
        interrupt: vec![second],
        ..Default::default()
    })?;
    assert_eq!(frame.scenes.len(), 1);
    assert_eq!(frame.particles.len(), 4);
    assert!(frame.particles.iter().all(|p| p.owner == actors[2]));
    assert!(
        battle
            .step(BattleInput {
                interrupt: vec![second],
                ..Default::default()
            })
            .is_err()
    );
    let next = battle.step(BattleInput::default())?;
    assert_eq!(next.update, frame.update + 1);
    assert_eq!(next.particles.len(), 4);
    Ok(())
}

#[test]
fn interrupting_active_scene_releases_effects_owned_by_other_recipients() -> Result<()> {
    let (fixture, poses) = fixtures()?;
    let prepared = Arc::new(prepare(&fixture, &poses, 3)?);
    let actors: Vec<_> = prepared.actor_ids().collect();
    let mut battle = Battle::new(prepared);
    let mut frame = battle.step(BattleInput {
        actions: vec![ActionRequest {
            actor: actors[2],
            target: actors[3],
            action: 99,
        }],
        ..Default::default()
    })?;
    let mut resident = None;
    for _ in 1..184 {
        frame = battle.step(BattleInput::default())?;
        for cue in &frame.cues {
            if let resonance_battle::Cue::Released { action, .. } = cue {
                resident = Some(*action);
            }
        }
    }
    assert_eq!(frame.particles.len(), 12); // Three models and three effects per recipient.
    assert!(frame.particles.iter().any(|p| p.owner == actors[0]));
    frame = battle.step(BattleInput {
        interrupt: vec![resident.context("missing resident")?],
        ..Default::default()
    })?;
    assert!(frame.particles.is_empty() && frame.scenes.is_empty());
    for _ in 0..40 {
        assert!(battle.step(BattleInput::default())?.particles.is_empty());
    }
    Ok(())
}

#[test]
fn unprepared_model_motion_and_invalid_model_selection_fail_at_their_boundaries() -> Result<()> {
    let (mut fixture, poses) = fixtures()?;
    fixture.bank.modifiers.get_mut(&1792).unwrap()[3] = 255;
    assert!(
        prepare(&fixture, &poses, 3)
            .err()
            .context("accepted absent motion")?
            .to_string()
            .contains("unprepared effect model motion")
    );
    fixture.bank.modifiers.get_mut(&1792).unwrap()[3] = 0;
    // Change the scratch model selector while retaining a valid prepared motion.
    fixture.bank.modifiers.get_mut(&1792).unwrap()[10] = 255;
    let prepared = Arc::new(prepare(&fixture, &poses, 3)?);
    let actors: Vec<_> = prepared.actor_ids().collect();
    let mut battle = Battle::new(prepared);
    battle.step(BattleInput {
        actions: vec![ActionRequest {
            actor: actors[2],
            target: actors[3],
            action: 99,
        }],
        ..Default::default()
    })?;
    for _ in 1..=60 {
        battle.step(BattleInput::default())?;
    }
    let error = battle
        .step(BattleInput::default())
        .err()
        .context("accepted an unbound model selector")?;
    assert!(
        error.to_string().contains("unprepared scene model"),
        "{error}"
    );
    assert!(battle.step(BattleInput::default()).is_err());
    Ok(())
}
