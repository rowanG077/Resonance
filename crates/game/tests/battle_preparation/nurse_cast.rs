use super::*;
use resonance_battle::{
    ActionPhase, Control, Cue, ModelDefinition, MotionBinding, Playback, PreparedBattle,
    SoundBinding, VoiceLine,
};
use resonance_content::{arte, battle_effect, battle_scene};

struct NurseResources<'a> {
    files: &'a Files,
    scene: Option<u16>,
}

impl BattleResources for NurseResources<'_> {
    fn casting(&mut self, path: &str) -> Result<battle::casting::CastingResource> {
        anyhow::ensure!(
            path == "battle/casting/raine/99",
            "unexpected casting resource"
        );
        Ok(battle::casting::CastingResource {
            character: 4,
            technique: 99,
            model: 3,
            stored_scene: self.scene,
        })
    }

    fn voice(&mut self, path: &str) -> Result<Vec<Option<VoiceLine>>> {
        // These opaque IDs represent prepared audio resources, as in the other
        // headless preparation tests. The selection itself uses verified tables.
        use battle::{
            model::ModelSource,
            voice::{Phase, Sound},
        };
        let actors = [
            ModelSource::Party(1),
            ModelSource::Party(2),
            ModelSource::Party(4),
            ModelSource::Enemy(36),
        ];
        let resolve = |sound| {
            Ok(match sound {
                Sound::Cue(index) => SoundBinding { resource: 1, index },
                Sound::Stream(index) => SoundBinding { resource: 2, index },
            })
        };
        match path {
            "battle/voices/relative/42" => {
                battle::voice::relative(self.files, &actors, 42, resolve)
            }
            "battle/voices/techniques/raine/237/release" => {
                battle::voice::technique(self.files, &actors, 237, Phase::Release, resolve)
            }
            "battle/voices/techniques/raine/237/chant" => {
                battle::voice::technique(self.files, &actors, 237, Phase::Chant, resolve)
            }
            "battle/voices/techniques/raine/237/fallback" => {
                battle::voice::technique(self.files, &actors, 237, Phase::Fallback, resolve)
            }
            "battle/voices/techniques/raine/237/self" => {
                battle::voice::technique(self.files, &actors, 237, Phase::SelfChant, resolve)
            }
            _ => bail!("unexpected voice resource {path}"),
        }
    }

    fn sound(&mut self, path: &str) -> Result<SoundBinding> {
        Ok(SoundBinding {
            resource: 1,
            index: path
                .strip_prefix("battle/sounds/common/")
                .context("unexpected sound resource")?
                .parse()?,
        })
    }

    fn effect(&mut self, path: &str) -> Result<EffectResource> {
        match path {
            "battle/effects/common" => Ok(EffectResource {
                models: Default::default(),
                source: battle_effect::COMMON_PATH.into(),
                resource: 37,
                members: vec![4, 8, 37],
                scene: None,
            }),
            "battle/scenes/237.json" => {
                let scene: battle_scene::Scene = self.files.json(path)?;
                Ok(EffectResource {
                    models: Default::default(),
                    source: scene.effects,
                    resource: 237,
                    members: vec![1, 2, 3, 4, 5, 6],
                    scene: Some(battle::SceneResources {
                        technique: 237,
                        models: scene
                            .models
                            .keys()
                            .map(|&slot| (slot, 700 + u32::from(slot)))
                            .collect(),
                    }),
                })
            }
            _ => bail!("unexpected effect resource {path}"),
        }
    }

    fn motion(&mut self, path: &str) -> Result<MotionBinding> {
        anyhow::ensure!(path == "battle/motions/raine/13", "unexpected entry motion");
        Ok(MotionBinding { model: 3, clip: 13 })
    }

    fn spell(&mut self, path: &str) -> Result<u16> {
        anyhow::ensure!(path == "battle/spells/nurse", "unexpected resident spell");
        Ok(237)
    }

    fn particle(&mut self, _: &str) -> Result<Arc<resonance_battle::ParticleDefinition>> {
        bail!("unexpected particle template")
    }
    fn projectile(&mut self, _: &str) -> Result<ProjectileResource> {
        bail!("unexpected projectile")
    }
    fn melee(&mut self, _: &str) -> Result<MeleeResource> {
        bail!("unexpected melee contact")
    }
}

fn prepare(
    files: &Files,
    actors: Vec<Actor>,
    models: Vec<Option<Arc<ModelDefinition>>>,
    resources: &mut NurseResources<'_>,
    cache: &mut PreparationCache,
) -> Result<Arc<PreparedBattle>> {
    let techniques: arte::Catalogue = files.json(arte::PATH)?;
    let prepared = battle::prepare(
        cache,
        files,
        &[
            ActionBinding {
                id: 99,
                phase: ActionPhase::Casting,
                module: "battle::raine_nurse".into(),
                entry: "run".into(),
                duration: 0,
                tp_cost: u16::from(techniques.definition(99)?.tp_cost),
            },
            ActionBinding {
                id: 237,
                phase: ActionPhase::Resident,
                module: "battle::nurse".into(),
                entry: "run".into(),
                // 60694/37B10 retain 250 updates, independently of the stored
                // source descriptor's duration of 240.
                duration: 250,
                tp_cost: 0,
            },
        ],
        actors,
        1,
        resources,
        models,
    )?;
    let ids: Vec<_> = prepared.actor_ids().collect();
    Ok(Arc::new(
        Arc::try_unwrap(prepared)
            .expect("unshared candidate")
            .with_camera(resonance_battle::CameraDefinition {
                leader: ids[0],
                target: ids[3],
                stage_pitch: -1.,
                adaptive: true,
                initial: resonance_battle::CameraPose {
                    eye: [0., 490., 2050.],
                    focus: [0., 120., 0.],
                    pitch: 7.,
                    yaw: 90.,
                    radius: 2050.,
                },
            })?
            .with_ambient_color([64; 3])
            .with_stage_colors(
                [64, 64, 64, 255],
                [Some([64, 64, 64, 255]), None, None, None],
            ),
    ))
}

#[test]
#[ignore = "requires current Nurse casting, voices, party models and scene publications; no devices"]
fn cold_nurse_cast_runs_verified_models_transition_recovery_and_independent_scene() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        files,
        &[
            battle::model::ModelSource::Party(1),
            battle::model::ModelSource::Party(2),
            battle::model::ModelSource::Party(4),
            battle::model::ModelSource::Enemy(36),
            battle::model::ModelSource::Scene(237),
        ],
        &mut cache,
        || false,
    )?;
    for (name, source) in [
        (
            "raine_nurse",
            include_bytes!("../../../../scripts/battle/raine_nurse.sym").as_slice(),
        ),
        (
            "casting",
            include_bytes!("../../../../scripts/battle/casting.sym").as_slice(),
        ),
        (
            "nurse",
            include_bytes!("../../../../scripts/battle/nurse.sym").as_slice(),
        ),
    ] {
        assert_eq!(
            files.read(&format!("scripts/battle/{name}.sym"))?.as_ref(),
            source
        );
    }
    let mut actors = Vec::new();
    let mut models = Vec::new();
    for (slot, (character, hp, max_hp)) in [(1, 100, 328), (2, 50, 172), (4, 150, 413)]
        .into_iter()
        .enumerate()
    {
        let mut session = actor();
        session.hp = hp;
        session.max_hp = max_hp;
        session.tp = 90;
        session.max_tp = 90;
        session.control = if slot == 0 {
            Control::Manual
        } else {
            Control::Auto
        };
        session.position = [slot as f32 * 150., 0., 0.];
        let (session, model) = battle::model::party(
            &files,
            character,
            session,
            battle::model::ModelSetup {
                resource: slot as u32 + 1,
                initial: Playback {
                    clip: 0,
                    frame: 0.,
                    rate: 0.5,
                    repeat: true,
                },
                suppress_root_translation: [false; 3],
                stun: None,
            },
        )?;
        actors.push(session);
        models.push(Some(model));
    }
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    let (mut enemy, model) = battle::model::enemy(
        &files,
        &menu.monsters.records[36],
        1,
        battle::model::ModelSetup {
            resource: 4,
            initial: Playback {
                clip: 0,
                frame: 0.,
                rate: 0.5,
                repeat: true,
            },
            suppress_root_translation: [false; 3],
            stun: None,
        },
    )?;
    enemy.position = [600., 0., 0.];
    actors.push(enemy);
    models.push(Some(model));
    let mut compiler = PreparationCache::default();
    let mut resources = NurseResources {
        files: &files,
        scene: Some(237),
    };
    let prepared = prepare(
        &files,
        actors.clone(),
        models.clone(),
        &mut resources,
        &mut compiler,
    )?;
    let ids: Vec<_> = prepared.actor_ids().collect();
    let request = ActionRequest {
        actor: ids[2],
        target: ids[0],
        action: 99,
    };
    let mut active = Battle::new(prepared);
    let first = active.step(BattleInput {
        actions: vec![request],
        ..Default::default()
    })?;
    assert_eq!(first.actors[2].tp, 90);

    // Failed candidates leave the active casting generation and its party intact.
    for scene in [None, Some(216)] {
        resources.scene = scene;
        assert!(
            prepare(
                &files,
                actors.clone(),
                models.clone(),
                &mut resources,
                &mut compiler
            )
            .is_err()
        );
    }
    resources.scene = Some(237);
    let mut missing_entry = models.clone();
    Arc::make_mut(missing_entry[2].as_mut().unwrap())
        .motions
        .remove(&13);
    assert!(
        prepare(
            &files,
            actors.clone(),
            missing_entry,
            &mut resources,
            &mut compiler
        )
        .is_err()
    );
    let mut missing_scene = files.clone();
    missing_scene.bytes.remove(&battle_scene::path(237));
    assert!(
        prepare(
            &missing_scene,
            actors.clone(),
            models.clone(),
            &mut resources,
            &mut compiler
        )
        .is_err()
    );

    // One fixed registration: original initialization is combat tick 5.
    // Real clips, tables, common effects and all scene members run together.
    let mut released = None;
    let mut healed = Vec::new();
    let mut voices = Vec::new();
    for tick in 6..=630 {
        let frame = active.step(BattleInput::default())?;
        assert_eq!(
            frame.actors[2].tp,
            if tick < 316 { 90 } else { 62 },
            "tick {tick}"
        );
        for cue in &frame.cues {
            match cue {
                Cue::Released { action, .. } => {
                    assert_eq!(tick, 376);
                    assert!(released.replace(*action).is_none());
                }
                Cue::Recovered { actor, .. } => healed.push((tick, actor.index())),
                Cue::Voice { actor, sound, .. } => voices.push((tick, actor.index(), sound.index)),
                _ => {}
            }
        }
        assert_eq!(
            frame.scenes.len(),
            usize::from((316..=628).contains(&tick)),
            "tick {tick}"
        );
        assert!(
            frame
                .models
                .iter()
                .all(|model| model.visible == !(377..=628).contains(&tick))
        );
        if tick == 498 {
            assert_eq!(
                frame.actors[..3]
                    .iter()
                    .map(|actor| actor.hp)
                    .collect::<Vec<_>>(),
                [231, 118, 315]
            );
            assert_eq!(frame.actors[3].hp, actors[3].hp);
        }
        if tick == 629 {
            assert!(frame.cues.contains(&Cue::Completed {
                action: released.context("missing resident")?
            }));
            assert!(
                frame
                    .particles
                    .iter()
                    .all(|particle| particle.resource != 237)
            );
        }
    }
    assert_eq!(healed, [(498, 0), (498, 1), (498, 2)]);
    assert_eq!(
        voices,
        [(234, 2, 458), (316, 2, 423), (499, 0, 544), (499, 1, 664)]
    );

    for interruption in [10, 400] {
        let prepared = prepare(
            &files,
            actors.clone(),
            models.clone(),
            &mut resources,
            &mut compiler,
        )?;
        let mut interrupted = Battle::new(prepared);
        let first = interrupted.step(BattleInput {
            actions: vec![request],
            ..Default::default()
        })?;
        let casting = first.actions[0].0;
        let mut healed = Vec::new();
        let mut frame = first;
        for tick in 6..=630 {
            frame = interrupted.step(BattleInput {
                interrupt: if tick == interruption {
                    vec![casting]
                } else {
                    vec![]
                },
                ..Default::default()
            })?;
            healed.extend(frame.cues.iter().filter_map(|cue| match cue {
                Cue::Recovered { actor, .. } => Some((tick, actor.index())),
                _ => None,
            }));
        }
        assert_eq!(frame.actors[2].tp, if interruption == 10 { 90 } else { 62 });
        assert_eq!(healed.len(), if interruption == 10 { 0 } else { 3 });
        assert!(frame.scenes.is_empty());
    }
    // 39974 substitutes the generic streamed chant when Raine selected herself.
    // It has its own original duration; the release clock stays unchanged.
    let prepared = prepare(
        &files,
        actors.clone(),
        models.clone(),
        &mut resources,
        &mut compiler,
    )?;
    let mut self_cast = Battle::new(prepared);
    self_cast.step(BattleInput {
        actions: vec![ActionRequest {
            target: ids[2],
            ..request
        }],
        ..Default::default()
    })?;
    let mut self_voices = Vec::new();
    for tick in 6..=316 {
        self_voices.extend(
            voice_cues(&self_cast.step(BattleInput::default())?)
                .into_iter()
                .map(|(line, _)| (tick, line)),
        );
    }
    assert_eq!(self_voices, [(255, 369), (316, 423)]);

    actors[2].tp = 27;
    let prepared = prepare(&files, actors, models, &mut resources, &mut compiler)?;
    let mut poor = Battle::new(prepared);
    let frame = poor.step(BattleInput {
        actions: vec![request],
        ..Default::default()
    })?;
    assert_eq!(frame.actors[2].tp, 27);
    assert!(frame.actions.is_empty() && frame.scenes.is_empty());
    assert!(matches!(frame.cues.as_slice(), [Cue::Rejected { .. }]));
    Ok(())
}

fn chant_sequence(body: &str, fallback_duration: u16) -> Result<(Battle, ActionRequest)> {
    let compiled = symphonia_script_compiler::compile(
        "test",
        &std::collections::BTreeMap::from([
            (
                "test".into(),
                format!(
                    r#"
                script battle; use battle; use battle::casting;
                asset chant: battle::Voice = "test/chant";
                asset fallback: battle::Voice = "test/fallback";
                asset release: battle::Voice = "test/release";
                pub task run() {{
                    let voices = casting::Voices {{ chant: chant, fallback: fallback, release: release }};
                    let mut last = ticks(0);
                    {body}
                    battle::finish();
                }}
            "#
                ),
            ),
            (
                "battle::casting".into(),
                include_str!("../../../../scripts/battle/casting.sym").into(),
            ),
        ]),
        &resonance_battle::native_declarations(),
    )?;
    let resources = compiled
        .assets
        .iter()
        .map(|asset| {
            let (index, duration) = match asset.path.as_str() {
                "test/chant" => (1, 62),
                "test/fallback" => (2, fallback_duration),
                "test/release" => (3, 0),
                _ => unreachable!(),
            };
            resonance_battle::ResourceBinding::Voice(vec![Some(VoiceLine {
                sound: SoundBinding { resource: 1, index },
                duration,
            })])
        })
        .collect();
    let entry = compiled
        .program
        .authored()
        .unwrap()
        .functions
        .iter()
        .find(|function| function.name == "test::run")
        .context("missing chant fixture entry")?
        .entry;
    let prepared = Arc::new(PreparedBattle::new(
        vec![actor()],
        vec![resonance_battle::ActionDefinition {
            id: 1,
            phase: ActionPhase::Casting,
            program: Arc::new(compiled.program),
            entry,
            duration: 100,
            tp_cost: 0,
            resources,
        }],
        1,
        vec![],
        vec![],
    )?);
    let actor = prepared.actor_ids().next().unwrap();
    Ok((
        Battle::new(prepared),
        ActionRequest {
            actor,
            target: actor,
            action: 1,
        },
    ))
}

fn voice_cues(frame: &resonance_battle::BattleFrame) -> Vec<(u16, resonance_battle::VoiceId)> {
    frame
        .cues
        .iter()
        .filter_map(|cue| match cue {
            Cue::Voice {
                sound, playback, ..
            } => Some((sound.index, *playback)),
            _ => None,
        })
        .collect()
}

#[test]
fn maintained_chant_latches_held_and_rejected_requests_and_prefers_specific_voice() -> Result<()> {
    let calls = r#"
        last = casting::chant_voice(voices, ticks(82), last);
        await battle::next_update();
        last = casting::chant_voice(voices, ticks(82), last);
        await battle::next_update();
        last = casting::chant_voice(voices, ticks(82), last);
    "#;
    // Equal thresholds choose the specific chant. Finishing its playback cannot
    // repeat either request while the casting countdown remains unchanged.
    let (mut active, request) = chant_sequence(calls, 62)?;
    let frame = active.step(BattleInput {
        actions: vec![request],
        ..Default::default()
    })?;
    let first = voice_cues(&frame);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, 1);
    let frame = active.step(BattleInput {
        voices_finished: vec![first[0].1],
        ..Default::default()
    })?;
    assert!(voice_cues(&frame).is_empty());
    assert!(voice_cues(&active.step(BattleInput::default())?).is_empty());

    // 3898C remembers the requested countdown even when 71E78 rejects its
    // priority. A later audio completion must not retry that held countdown.
    let body = format!(
        r#"
        battle::voice(battle::owner(), release, 3);
        await battle::next_update();
        {calls}
    "#
    );
    let (mut active, request) = chant_sequence(&body, 41)?;
    let frame = active.step(BattleInput {
        actions: vec![request],
        ..Default::default()
    })?;
    let first = voice_cues(&frame);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, 3);
    assert!(voice_cues(&active.step(BattleInput::default())?).is_empty());
    let frame = active.step(BattleInput {
        voices_finished: vec![first[0].1],
        ..Default::default()
    })?;
    assert!(voice_cues(&frame).is_empty());
    assert!(voice_cues(&active.step(BattleInput::default())?).is_empty());
    Ok(())
}

#[test]
fn maintained_chant_fallback_waits_for_actual_audio_completion() -> Result<()> {
    let (mut active, request) = chant_sequence(
        r#"
        last = casting::chant_voice(voices, ticks(82), last);
        await battle::next_update();
        last = casting::chant_voice(voices, ticks(61), last);
        await battle::next_update();
        last = casting::chant_voice(voices, ticks(61), last);
    "#,
        41,
    )?;
    let frame = active.step(BattleInput {
        actions: vec![request],
        ..Default::default()
    })?;
    let first = voice_cues(&frame);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, 1);
    assert!(voice_cues(&active.step(BattleInput::default())?).is_empty());
    let frame = active.step(BattleInput {
        voices_finished: vec![first[0].1],
        ..Default::default()
    })?;
    let fallback = voice_cues(&frame);
    assert_eq!(fallback.len(), 1);
    assert_eq!(fallback[0].0, 2);
    assert!(
        !frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::VoiceStopped { .. }))
    );
    Ok(())
}
