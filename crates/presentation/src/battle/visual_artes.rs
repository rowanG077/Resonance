//! GPU observations of catalogue actions through the ordinary battle scene.
//! Capture/admission receipts deliberately make no visual-review judgement.
//!
//! Run the ignored `render_catalogue_artes_to_png` test with
//! `RESONANCE_ARTE_CATALOGUE`, `RESONANCE_ARTE_EDITION` and
//! `RESONANCE_ARTE_CAPTURE_OUTPUT`. `RESONANCE_ARTE_CASES` accepts comma-separated
//! inventory IDs or `all`; its default is the edition's Lloyd Demon Fang.
//! Optional `RESONANCE_ARTE_FIXTURE` supplies an observed initial physical state
//! and observer camera, never an actor pose sequence.
//! `RESONANCE_ARTE_UNISON_PLAN` selects one validated canonical assignment per
//! recipe while retaining its unexecuted alternatives as metadata.
use super::*;
mod assignments;
mod enemy_fixture;
use anyhow::bail;
use bevy::{app::PluginsState, time::TimeUpdateStrategy};
use resonance_battle::{ActionKey, ActionRequest, BattleInput, BattlePhase, Cue};
use resonance_content::{diagnostics::Diagnostics, library::Library};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

struct CaptureInputs {
    library: Library,
    data: Arc<SessionData>,
    menus: Arc<MenuData>,
    rules: resonance_content::battle_rules::Captured<resonance_content::arte::Catalogue>,
}

fn fixture_actor(row: &Value, slot: usize, side: Side) -> Option<&Value> {
    let actors = row["fixture"]["ready"]["actors"].as_array()?;
    let side = if side == Side::Party { 0 } else { 1 };
    let same_side = |source: &&Value| source["side"].as_u64() == Some(side);
    actors
        .iter()
        .filter(same_side)
        .find(|source| source["slot"].as_u64() == Some(slot as u64))
        .or_else(|| {
            let mut matches = actors.iter().filter(same_side);
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        })
}

fn package(
    inputs: &CaptureInputs,
    row: &Value,
) -> Result<(
    Package,
    ActorId,
    ActorId,
    ActionKey,
    Option<enemy_fixture::Plan>,
)> {
    let library = &inputs.library;
    let diagnostics = Diagnostics::new(true);
    let data = inputs.data.clone();
    let menus = inputs.menus.clone();
    let rules = &inputs.rules;
    let binding = &row["binding"];
    let enemy = binding["enemy"].as_u64().map(|v| v as u16);
    let members: Vec<(u8, u16)> = row
        .get("assignment")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    let character = members
        .first()
        .map_or(binding["character"].as_u64().unwrap_or(1) as u8, |member| {
            member.0
        });
    let arte = members
        .first()
        .map_or(binding["arte"].as_u64().unwrap_or(1) as u16, |member| {
            member.1
        });
    let capabilities = if enemy.is_none() && members.is_empty() {
        rules.techniques.definition(usize::from(arte))?.capabilities
    } else {
        Default::default()
    };
    let cure = enemy.is_none()
        && matches!(
            menus
                .techniques
                .get(usize::from(arte))
                .and_then(|technique| technique.field_use),
            Some(resonance_content::menu_data::TechniqueUse::Cure { .. })
        );
    let mut party = Party::new(&data, Default::default())?;
    party.formation = if members.is_empty() {
        vec![character]
    } else if members.len() < 4 || members.iter().any(|member| member.0 == 1) {
        vec![1]
    } else {
        Vec::new()
    };
    for &(character, _) in &members {
        if !party.formation.contains(&character) {
            party.formation.push(character);
        }
    }
    if members.is_empty() && capabilities.target == resonance_content::arte::TechniqueTarget::Ally {
        let ally = (1..=9).find(|other| *other != character).unwrap();
        party.formation.push(ally);
    }
    party.field_leader = party.formation[0];
    party.settings.battle_controls = [1; 4];
    for &character in &party.formation.clone() {
        party
            .raise_level(&data, usize::from(character - 1), 99, None, || 0)
            .map_err(anyhow::Error::msg)?;
    }
    for &character in &party.formation.clone() {
        let member = &mut party.members[usize::from(character - 1)];
        member.techniques.clear();
        member.disabled_techniques.clear();
        member.shortcuts = [0; 4];
        member.assist_shortcuts = [None; 2];
        member.unison_arte = None;
    }
    // The original capture's ordinary Colette preset uses post-seal state1
    // and SET_CHARACTER_COSTUME(2,3), not a quest progression simulation.
    // See tools/oracle/battle_artes.py and arte_equivalence.py::party_case.
    let colette_state = if party.formation.contains(&2) {
        party.members[1].costume = 3;
        1
    } else {
        0
    };
    if !members.is_empty() {
        party.unison_gauge = 200;
        party.settings.preferences.learn_elemental_artes = true;
        for &(character, arte) in &members {
            let member = &mut party.members[usize::from(character - 1)];
            member.techniques = [arte].into();
            member.disabled_techniques.clear();
            member.shortcuts = [arte; 4];
            member.unison_arte = Some(arte);
        }
    } else if enemy.is_none() {
        let member = &mut party.members[usize::from(character - 1)];
        member.techniques.insert(arte);
        member.disabled_techniques.remove(&arte);
    }
    party.validate(&data)?;
    let mut snapshot = library.snapshot();
    let explicit_formation = row["fixture"]["formation"].as_u64().map(|id| id as u16);
    let enemy_plan = enemy
        .map(|enemy| {
            enemy_fixture::Plan::read(
                &mut snapshot,
                &rules.formations,
                enemy,
                binding["row"].as_u64().context("enemy row missing")? as usize,
                explicit_formation,
            )
        })
        .transpose()?;
    let formation = enemy_plan
        .as_ref()
        .map(|plan| plan.formation)
        .or(explicit_formation)
        .unwrap_or(2);
    let setup = resonance_events::battle::Setup {
        route: [0; 5],
        encounter: resonance_events::battle::Encounter::Formation(formation),
        arena: row["fixture"]["arena"].as_u64().unwrap_or(13) as u16,
        defeat: resonance_events::battle::DefeatPolicy::ResumeEvent,
        music: None,
    };
    let audio_catalogue =
        resonance_content::battle_audio::document::Catalogue::capture(&mut snapshot, &|| false)?;
    let mut inputs = encounter::Assets::capture_fixture(
        &mut snapshot,
        &menus,
        &data,
        &party,
        setup,
        diagnostics.clone(),
        &|| false,
    )?;
    let mut selected_audio = resonance_content::battle_audio::Selection::default();
    let mut selected = None;
    let mut prepared = inputs.prepare_fixture(
        &menus,
        encounter::PrepareOptions {
            hunting_symbol: None,
            event_flags: enemy_plan
                .as_ref()
                .map(|plan| plan.event_flags.clone())
                .unwrap_or_default(),
            devils_arms_unlocked: true,
            victory_story_flags: [false; 2],
            random_seed: 1,
            map: 332,
            world_music: 0,
            story: if members.is_empty() { 2500 } else { 1_403_000 },
            colette_state,
            overlimit_boost: false,
        },
        |sound| audio_catalogue.collect(&mut selected_audio, &diagnostics, sound),
        |participants| {
            let owner = if let Some(enemy) = enemy {
                participants
                    .iter()
                    .position(|(_, setup)| {
                        setup.enemy_decision.as_ref().is_some_and(|decision| {
                            decision.enemy_id == resonance_battle::EnemyId(enemy)
                        })
                    })
                    .context("requested enemy is not active")?
            } else {
                party
                    .formation
                    .iter()
                    .position(|owner| *owner == character)
                    .context("party owner absent")?
            };
            let side = participants[owner].0.side;
            let enemy_choice = enemy
                .and_then(|_| participants[owner].1.enemy_decision.as_ref())
                .and_then(|decision| decision.choices.get(binding["row"].as_u64()? as usize));
            let target_self = capabilities.target
                == resonance_content::arte::TechniqueTarget::SelfTarget
                || enemy_choice.is_some_and(|choice| {
                    choice.target_policy
                        == Some(resonance_content::battle_action::TargetPolicy::SelfTarget)
                });
            let target_allies = capabilities.target
                == resonance_content::arte::TechniqueTarget::Ally
                || enemy_choice.is_some_and(|choice| choice.target_allies);
            let revive = capabilities.revives
                || enemy_choice.is_some_and(|choice| {
                    choice.target_policy
                        == Some(
                            resonance_content::battle_action::TargetPolicy::NearestIncapacitated,
                        )
                });
            let healing = capabilities.healing
                || enemy_choice.is_some_and(|choice| {
                    choice.target_allies
                        && choice.target_policy
                            == Some(resonance_content::battle_action::TargetPolicy::LowestHp)
                });
            let target = if let Some(character) = row["fixture"]["target_character"].as_u64() {
                party
                    .formation
                    .iter()
                    .position(|&owner| u64::from(owner) == character)
                    .context("observed fixture target character is absent")?
            } else if target_self {
                owner
            } else if target_allies {
                participants
                    .iter()
                    .enumerate()
                    .position(|(index, (actor, _))| index != owner && actor.side == side)
                    .or_else(|| (!revive).then_some(owner))
                    .context("support action has no other allied recipient")?
            } else {
                participants
                    .iter()
                    .position(|(actor, _)| actor.side != side)
                    .context("capture has no opposing target")?
            };
            let action = if enemy.is_some() {
                let index = binding["row"].as_u64().context("enemy row is missing")? as usize;
                participants[owner]
                    .1
                    .enemy_decision
                    .as_ref()
                    .unwrap()
                    .choices
                    .get(index)
                    .context("enemy row has no prepared action")?
                    .action
            } else {
                participants[owner]
                    .1
                    .techniques
                    .iter()
                    .find(|technique| technique.catalogue == resonance_battle::TechniqueId(arte))
                    .context("party arte has no prepared action")?
                    .action
            };
            for (index, (actor, setup)) in participants.iter_mut().enumerate() {
                if actor.side == Side::Party {
                    actor.control = resonance_battle::Control::Manual;
                }
                actor.equipment.max_hp = 1_000_000;
                actor.hp = 1_000_000;
                actor.equipment.max_tp = 9999;
                actor.tp = 9999;
                actor.position = [
                    if actor.side == Side::Party {
                        -180. + index as f32 * 120.
                    } else {
                        180. + index as f32 * 40.
                    },
                    0.,
                    0.,
                ];
                actor.heading = if actor.side == Side::Party { 90. } else { 270. };
                if let Some(observed) = fixture_actor(row, index, actor.side) {
                    actor.position = serde_json::from_value(observed["position"].clone())?;
                    actor.heading = observed["heading"]
                        .as_f64()
                        .context("fixture heading missing")?
                        as f32;
                }
                actor.facing_direction = resonance_battle::direction_from_heading(actor.heading);
                actor.movement.direction = actor.facing_direction;
                actor.movement.target_direction = actor.facing_direction;
                setup.decision = None;
                setup.companion = None;
            }
            if let Some(plan) = &enemy_plan {
                participants[owner].0.hp =
                    participants[owner].0.equipment.max_hp * i32::from(plan.owner_hp_percent) / 100;
            }
            if healing {
                participants[target].0.hp = participants[target].0.equipment.max_hp / 4;
            }
            if revive {
                ensure!(target != owner, "revival needs a noncaster ally");
                participants[target].0.hp = 0;
                participants[target].0.availability = resonance_battle::ActorAvailability::Dead;
            }
            if cure {
                let mut layers = participants[target].0.conditions.layers();
                layers.base = resonance_battle::conditions::ConditionSet::of(&[
                    resonance_battle::conditions::Condition::PoisonMild,
                ]);
                participants[target].0.conditions.reload_layers(layers);
            }
            selected = Some((
                ActorId::from_index(owner)?,
                ActorId::from_index(target)?,
                action,
            ));
            Ok(())
        },
    )?;
    prepared.core.suppress_automatic_decisions();
    selected_audio.music.insert(prepared.music);
    let audio = crate::battle_audio::Assets::capture_named(
        &audio_catalogue,
        &selected_audio,
        &mut snapshot,
        diagnostics.clone(),
        &mut Default::default(),
        &|| false,
    )?;
    // The ordinary field route already retains this global render dependency.
    // This standalone battle fixture must capture it before adopting its reader.
    snapshot.read(
        resonance_content::model_preview::document::TOON_RAMP,
        resonance_content::preparation::ASSET_LIMIT,
    )?;
    inputs.files = Files::from_snapshot(snapshot, || false, diagnostics)?;
    let scenery_models = resonance_game::battle::stage::Model::load(&inputs.files, &inputs.stage)?;
    let catalogue = inputs.catalogue.clone();
    let (owner, target, action) = selected.context("capture fixture was not configured")?;
    Ok((
        Package {
            assets: Arc::new(inputs),
            scenery_models,
            prepared,
            audio,
            party,
            menus,
            data,
            catalogue,
            gameplay_random: resonance_events::GameplayRandom::new(1),
            entry_seed: 1,
        },
        owner,
        target,
        action,
        enemy_plan,
    ))
}

fn step(scene: &mut Scene, input: BattleInput) -> Result<Vec<Cue>> {
    let mut frame = scene.core.step(input)?;
    let clock = frame.clock;
    let paused = clock.feedback_paused();
    let settle = matches!(
        clock,
        resonance_battle::BattleClock::Running | resonance_battle::BattleClock::EntrySettle
    ) && matches!(scene.core.phase(), BattlePhase::Entry | BattlePhase::Combat);
    let before = frame.cues.len();
    scene.feedback.resolve(&mut frame);
    scene.core.record_voice_requests(&frame.cues[before..]);
    scene.core.advance_models(&mut frame, clock, settle)?;
    scene.feedback.resolve_model_effects(&mut frame)?;
    frame.cues.extend(scene.effects.advance(&frame, paused)?);
    scene.effects.write_frames(&mut scene.particles);
    update_hud(
        &mut scene.hud,
        &frame,
        &scene.action_names,
        &scene.feedback.resources,
        &scene.diagnostics,
        paused,
    )?;
    scene.view.advance_feedback(&frame, paused)?;
    scene
        .feedback
        .advance(&mut frame, paused, scene.effects.actor_effect_ticks());
    let cues = frame.cues.clone();
    scene.frame = Some(frame);
    scene.published_tick = None;
    Ok(cues)
}

fn scene_mut(app: &mut App) -> &mut Scene {
    let owner = app
        .world_mut()
        .get_resource_mut::<Owner>()
        .unwrap()
        .into_inner();
    let Phase::Scene(scene) = &mut owner.phase else {
        panic!("capture scene is missing")
    };
    scene
}

fn warm(app: &mut App) -> Result<()> {
    let began = Instant::now();
    loop {
        ensure!(
            began.elapsed() < Duration::from_secs(120),
            "battle GPU warmup timed out"
        );
        app.update();
        crate::playthrough::check_exit(app)?;
        let scene = scene_mut(app);
        ensure!(
            scene.failure.is_none(),
            "battle GPU preparation failed: {:?}",
            scene.failure
        );
        if scene.state == SceneState::Active {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn fixture_camera(scene: &mut Scene, row: &Value, age: u32) -> Result<()> {
    let fixture = &row["fixture"];
    let camera = fixture["samples"]
        .as_array()
        .and_then(|samples| {
            samples
                .iter()
                .find(|sample| sample["age"].as_u64() == Some(u64::from(age)))
        })
        .map_or(&fixture["ready"]["camera"], |sample| &sample["camera"]);
    if camera.is_null() {
        return Ok(());
    }
    let eye: [f32; 3] = serde_json::from_value(camera["eye"].clone())?;
    let focus: [f32; 3] = serde_json::from_value(camera["target"].clone())?;
    let delta = Vec3::from_array(eye) - Vec3::from_array(focus);
    if let Some(frame) = &mut scene.frame {
        frame.camera = Some(resonance_battle::CameraPose {
            eye,
            focus,
            radius: delta.length(),
            yaw: delta.x.atan2(delta.z).to_degrees(),
            pitch: delta
                .y
                .atan2(Vec2::new(delta.x, delta.z).length())
                .to_degrees(),
        });
    }
    Ok(())
}

fn fixture_motion(scene: &mut Scene, row: &Value) -> Result<()> {
    if !row["fixture"]["ready"]["actors"].is_array() {
        return Ok(());
    }
    for id in scene.core.actor_ids().collect::<Vec<_>>() {
        let actor = &scene.core.actors()[id.index()];
        let Some(source) = fixture_actor(row, id.index(), actor.side) else {
            continue;
        };
        let mut motion = serde_json::to_value(resonance_battle::FixtureMotion::observe(actor))?;
        motion["position"] = source["position"].clone();
        motion["heading"] = source["heading"].clone();
        let direction = resonance_battle::direction_from_heading(
            source["heading"]
                .as_f64()
                .context("fixture heading missing")? as f32,
        );
        for key in ["facing_direction", "movement_direction", "target_direction"] {
            motion[key] = json!(direction);
        }
        if source["position"][1].as_f64().unwrap_or(0.).abs() > 0.01 {
            ensure!(
                source["motion"]["vertical"].is_number() && source["motion"]["gravity"].is_number(),
                "airborne source fixture requires observed initial velocity and gravity"
            );
        }
        if let Some(observed) = source["motion"].as_object() {
            for (key, value) in observed {
                motion[key] = value.clone();
            }
        }
        scene
            .core
            .restore_fixture_motion(id, serde_json::from_value(motion)?)?;
    }
    for _ in 0..row["fixture"]["source_action_start_frame"]
        .as_u64()
        .unwrap_or(0)
    {
        step(scene, BattleInput::default())?;
    }
    Ok(())
}

fn screenshot(app: &mut App, path: PathBuf, row: &Value, age: u32) -> Result<()> {
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    app.update();
    crate::playthrough::check_exit(app)?;
    let held = app.world().resource::<Owner>().diagnostic();
    let particle_states = match &app.world().resource::<Owner>().phase {
        Phase::Scene(scene) => scene
            .particles
            .iter()
            .map(|particle| {
                json!({
                    "handle":format!("{:?}",particle.id),"actor":particle.owner.index(),
                    "resource":particle.resource.0,"member":particle.member,
                    "origin":particle.origin,"owner_bone":particle.owner_bone,
                    "palettes":particle.state.palettes,"uv":particle.state.uv,
                    "colors":particle.state.colors,"state":particle.state,
                })
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let metadata = json!({"source_kind":"resonance", "case":row["id"], "age":age,
        "state_tick_locked":true, "audio_device":false, "battle":held,
        "particle_states":particle_states,
        "output_stage":app.world().resource::<crate::display::OutputStage>()});
    let complete = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let done = complete.clone();
    let failure = failed.clone();
    let target = app.world().resource::<crate::Framebuffer>().0.clone();
    app.world_mut()
        .spawn(Screenshot(target))
        .observe(move |event: On<ScreenshotCaptured>| {
            if let Err(error) = crate::screenshot::write(&event.image, &path, Some(&metadata)) {
                error!("Arte screenshot failed: {error:#}");
                failure.store(true, Ordering::Release);
            }
            done.store(true, Ordering::Release);
        });
    let began = Instant::now();
    while !complete.load(Ordering::Acquire) {
        ensure!(
            began.elapsed() < Duration::from_secs(15),
            "arte GPU readback timed out"
        );
        app.update();
        crate::playthrough::check_exit(app)?;
        ensure!(
            app.world().resource::<Owner>().diagnostic() == held,
            "battle advanced during held GPU readback"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    ensure!(
        !failed.load(Ordering::Acquire),
        "arte GPU screenshot write failed"
    );
    Ok(())
}

fn capture_case(
    app: &mut App,
    inputs: &CaptureInputs,
    row: &Value,
    output: &Path,
    progress: &mut Value,
) -> Result<Value> {
    let (package, actor, target, action, enemy_plan) = package(inputs, row)?;
    let is_unison = row["category"] == "unison";
    let members: Vec<(u8, u16)> = row
        .get("assignment")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    let slots = members
        .iter()
        .map(|member| {
            package
                .party
                .formation
                .iter()
                .position(|owner| *owner == member.0)
                .context("Unison owner absent from formation")
        })
        .collect::<Result<Vec<_>>>()?;
    let setup = package.assets.setup.clone();
    progress["assistance"] = json!({"target":target.index(),"formation":setup.formation().ok(),"arena":setup.arena,"story":if members.is_empty() {2500} else {1_403_000},
        "policy":"manual neutral actors with catalogue-declared target and benefit context",
        "participants":package.prepared.core.actors().iter().map(|actor|json!({"side":format!("{:?}",actor.side),"hp":actor.hp,"maximum_hp":actor.equipment.max_hp,"tp":actor.tp,"conditions":actor.conditions.base()})).collect::<Vec<_>>()});
    if package.party.formation.contains(&2) {
        progress["assistance"]["colette"] = json!({"colette_state":1,"costume":3,
            "source":"tools/oracle/arte_equivalence.py::party_case and battle_artes.py::prepare ordinary post-seal preset"});
    }
    if let Some(plan) = &enemy_plan {
        progress["assistance"]["enemy_context"] = serde_json::to_value(plan)?;
    }
    let entry_budget = enemy_plan.as_ref().map_or(600, |plan| plan.entry_budget);
    let mut world = resonance_events::GameWorld::default();
    world.party = Some(package.party.clone());
    let request = world.request_battle(setup).map_err(anyhow::Error::msg)?;
    *app.world()
        .resource::<crate::loading::Resident>()
        .files
        .write()
        .unwrap() = Some(Arc::new(package.assets.files.clone()));
    let toon = app
        .world()
        .resource::<AssetServer>()
        .load_builder()
        .with_settings(|settings: &mut bevy::image::ImageLoaderSettings| {
            settings.is_srgb = false;
            settings.sampler = bevy::image::ImageSampler::linear();
        })
        .load(resonance_content::model_preview::document::TOON_RAMP);
    // Use the same source target and final output pass as production battles.
    let render_target = RenderTarget::Image(
        app.world()
            .resource::<crate::display::Targets>()
            .source
            .clone()
            .into(),
    );
    let mut scene = construct_scene(app.world_mut(), package, toon, render_target)?;
    for _ in 0..entry_budget {
        step(&mut scene, BattleInput::default())?;
        if scene.core.phase() == BattlePhase::Combat
            && !scene.core.is_paused()
            && scene
                .frame
                .as_ref()
                .is_some_and(|frame| !frame.clock.paused())
        {
            break;
        }
    }
    ensure!(
        scene.core.phase() == BattlePhase::Combat,
        "battle entry never reached combat"
    );
    app.world_mut().insert_resource(Owner {
        request,
        phase: Phase::Scene(Box::new(scene)),
        input_tick: 0,
        publish_field: false,
    });
    warm(app)?;
    fixture_motion(scene_mut(app), row)?;
    if enemy_plan.is_some() {
        // Ordinary enemy selection waits for its flight entry to settle. This
        // explicit request fixture must retain that same physical prerequisite.
        let mut hover_wait = 0;
        while !scene_mut(app).core.actors()[actor.index()]
            .movement
            .hover_ready()
            && hover_wait < entry_budget
        {
            step(scene_mut(app), BattleInput::default())?;
            hover_wait += 1;
        }
        progress["assistance"]["hover_settling_ticks"] = json!(hover_wait);
        ensure!(
            scene_mut(app).core.actors()[actor.index()]
                .movement
                .hover_ready(),
            "enemy flight entry did not settle within its entry budget"
        );
        enemy_fixture::selected_action(
            &mut scene_mut(app).core,
            actor,
            row["binding"]["row"]
                .as_u64()
                .context("enemy row missing")? as usize,
            action,
        )?;
    }
    if is_unison {
        let core = &mut scene_mut(app).core;
        for id in core.actor_ids().collect::<Vec<_>>() {
            if core.actors()[id.index()].side == Side::Enemy {
                core.set_fixture_neutral_control(id)?;
            }
        }
        let [x, _, z] = core.actors()[target.index()].position;
        core.restore_fixture_placement(ActorId::from_index(0)?, [x - 100., 0., z], Some(90.))?;
        core.restore_fixture_target(ActorId::from_index(0)?, target)?;
    }
    if !is_unison {
        let capabilities = scene_mut(app)
            .core
            .prepared_techniques(actor)
            .iter()
            .find(|technique| technique.action == action)
            .map(|technique| technique.capabilities);
        if capabilities.is_some_and(|capabilities| capabilities.summon) {
            scene_mut(app).core.restore_fixture_overlimit(actor, 1000)?;
            progress["assistance"]["overlimit"] =
                json!("native active duration, required by summon capability");
        }
        if capabilities.is_some_and(|capabilities| {
            capabilities.regal_family == Some(resonance_content::arte::RegalArteFamily::Aerial)
        }) {
            for _ in 0..90 {
                let mut input = resonance_battle::ControlInput::neutral(actor);
                input.stick = [0, 127];
                step(scene_mut(app), input.into())?;
                if scene_mut(app).core.actors()[actor.index()].airborne() {
                    break;
                }
            }
            ensure!(
                scene_mut(app).core.actors()[actor.index()].airborne(),
                "native jump did not establish aerial arte context"
            );
            progress["assistance"]["aerial"] = json!("ordinary manual jump input");
        }
    }
    let mut selected_slots = 0;
    let mut admitted = None;
    let mut invocation = None;
    let mut rejections = Vec::new();
    let mut captures = Vec::new();
    let mut ages = vec![0, 4, 8, 16, 24, 40, 64, 96, 160];
    ages.extend(
        row["fixture"]["samples"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|sample| sample["age"].as_u64().map(|age| age as u32)),
    );
    let last_age = ages.iter().copied().max().unwrap_or(160);
    let mut action_completed = false;
    let mut action_interrupted = false;
    let mut termination_reason = Value::Null;
    let mut released_actions = Vec::new();
    let mut released = false;
    let mut released_at = None;
    let mut active_scene_at = None;
    let mut impacted = false;
    let mut cleaned = false;
    let baseline_stage_color = scene_mut(app).view.stage_color();
    let mut stage_changed = false;
    let mut stage_restored_at = None;
    let mut completion_at = None;
    let mut observation_end_at = None;
    let case_name = row["id"]
        .as_str()
        .context("case id missing")?
        .replace(':', "-");
    for tick in 0..3600u32 {
        let input = if is_unison {
            let phase = scene_mut(app)
                .frame
                .as_ref()
                .and_then(|frame| frame.unison.as_ref())
                .map(|run| run.phase);
            let unison = if admitted.is_none() && phase.is_none() {
                Some(resonance_battle::UnisonInput::Activate(
                    ActorId::from_index(0)?,
                ))
            } else if matches!(
                phase,
                Some(
                    resonance_battle::UnisonPhase::Selecting
                        | resonance_battle::UnisonPhase::Executing
                )
            ) && selected_slots < slots.len()
            {
                let slot = slots[selected_slots];
                selected_slots += 1;
                Some(resonance_battle::UnisonInput::Select {
                    pressed_slots: 1 << slot,
                })
            } else {
                None
            };
            BattleInput {
                unison,
                ..Default::default()
            }
        } else if admitted.is_none() {
            BattleInput::from(ActionRequest {
                actor,
                action,
                target,
            })
        } else {
            BattleInput::default()
        };
        let cues = step(scene_mut(app), input)?;
        for cue in &cues {
            if matches!(cue, Cue::Rejected { .. }) && rejections.len() < 8 {
                rejections.push(format!("{cue:?}"));
            }
        }
        if is_unison && admitted.is_none() {
            if let Some(combination) = scene_mut(app)
                .frame
                .as_ref()
                .and_then(|frame| frame.unison.as_ref())
                .filter(|run| run.phase == resonance_battle::UnisonPhase::Combined)
                .and_then(|run| run.combination.as_ref())
            {
                ensure!(
                    u64::from(combination.id.0)
                        == row["binding"]["combination"]
                            .as_u64()
                            .context("Unison binding missing")?,
                    "a different Unison combination was admitted"
                );
                admitted = Some(tick);
                progress["admitted"] = json!(true);
                progress["admission_tick"] = json!(tick);
            }
        }
        if !is_unison && admitted.is_none() {
            if let Some(id) = cues.iter().find_map(|cue| match cue {
                Cue::Started {
                    actor: owner,
                    definition: Some(key),
                    action: id,
                } if *owner == actor && *key == action => Some(*id),
                _ => None,
            }) {
                admitted = Some(tick);
                invocation = Some(id);
                progress["admitted"] = json!(true);
                progress["admission_tick"] = json!(tick);
            }
        }
        let Some(start) = admitted else {
            ensure!(
                tick < if is_unison { 3000 } else { 60 },
                "requested action was not admitted: {rejections:?}"
            );
            continue;
        };
        let age = tick - start;
        let scene_active = active_scene_at.is_none()
            && cues.iter().any(|cue| {
                matches!(cue,
                Cue::Casting { actor: owner, action: key,
                    phase: resonance_battle::CastPhase::Released }
                    if *owner == actor && *key == action)
            });
        if scene_active {
            active_scene_at = Some(age);
            progress["active_scene_age"] = json!(age);
        }
        let release = !released
            && cues.iter().any(|cue| {
                matches!(
                    cue,
                    Cue::Released { .. }
                        | Cue::Casting {
                            phase: resonance_battle::CastPhase::Released,
                            ..
                        }
                )
            });
        if release {
            released_at = Some(age);
            progress["release_age"] = json!(age);
        }
        let impact = !impacted && cues.iter().any(|cue| matches!(cue, Cue::Hit { .. }));
        released |= release;
        impacted |= impact;
        let completed_before = action_completed;
        action_completed |= cues
            .iter()
            .any(|cue| matches!(cue, Cue::Completed { action } if Some(*action) == invocation));
        for cue in &cues {
            if let Cue::Released {
                action: child,
                parent,
                actor: owner,
                ..
            } = cue
            {
                if is_unison
                    || *owner == actor
                    || Some(*child) == invocation
                    || parent.is_some_and(|parent| {
                        Some(parent) == invocation || released_actions.contains(&parent)
                    })
                {
                    if !released_actions.contains(child) {
                        released_actions.push(*child);
                    }
                }
            }
            if matches!(cue, Cue::Interrupted { action } if Some(*action) == invocation) {
                action_interrupted = true;
                let owner = &scene_mut(app).core.actors()[actor.index()];
                termination_reason = json!({"kind":"interrupted","age":age,
                    "hp":owner.hp,"availability":format!("{:?}",owner.availability)});
                progress["termination_reason"] = termination_reason.clone();
            }
        }
        if is_unison {
            action_completed |= admitted.is_some()
                && scene_mut(app)
                    .frame
                    .as_ref()
                    .is_some_and(|frame| frame.unison.is_none());
        }
        let completion = !completed_before && action_completed;
        if completion {
            completion_at = Some(age);
            progress["completion_age"] = json!(age);
        }
        let stage_color = scene_mut(app).view.stage_color();
        let clock = scene_mut(app).frame.as_ref().map(|frame| frame.clock);
        let stage_at_baseline = stage_color == baseline_stage_color;
        stage_changed |= !stage_at_baseline;
        let stage_restored = stage_changed
            && stage_at_baseline
            && stage_restored_at.is_none()
            && clock.is_some_and(|clock| !clock.feedback_paused());
        if stage_restored {
            stage_restored_at = Some(age);
        }
        progress["stage_observation"] = json!({"baseline":baseline_stage_color,
            "current":stage_color,"changed":stage_changed,"restored_age":stage_restored_at,
            "clock":format!("{clock:?}")});
        progress["action_completed"] = json!(action_completed);
        progress["action_interrupted"] = json!(action_interrupted);
        progress["action_terminated"] = json!(action_completed || action_interrupted);
        let live_action = invocation.is_some_and(|id| scene_mut(app).core.action_age(id).is_some())
            || released_actions
                .iter()
                .any(|&id| scene_mut(app).core.action_age(id).is_some());
        let visual_empty = scene_mut(app).particles.is_empty()
            && scene_mut(app).effects.live_particles().next().is_none()
            && scene_mut(app)
                .frame
                .as_ref()
                .is_some_and(|frame| frame.projectiles.is_empty())
            && scene_mut(app).effects.active_programs().next().is_none();
        let cleanup = (action_completed || action_interrupted)
            && !cleaned
            && !live_action
            && visual_empty
            && stage_at_baseline;
        cleaned |= cleanup;
        let observation_complete = age >= last_age
            && stage_at_baseline
            && (cleaned
                || (!live_action
                    && visual_empty
                    && age >= released_at.map_or(1800, |start| start + 900)));
        let observation_end = tick == 3599 && !observation_complete;
        if observation_end {
            observation_end_at = Some(age);
            progress["observation_end_age"] = json!(age);
        }
        let interesting = scene_active
            || release
            || impact
            || completion
            || stage_restored
            || cleanup
            || observation_end;
        let after_release = released_at.is_some_and(|start| {
            [4, 8, 16, 24, 40, 64, 96, 160, 240, 360].contains(&(age - start))
        });
        let after_active = active_scene_at.is_some_and(|start| {
            [4, 8, 16, 24, 40, 64, 96, 160, 240, 360].contains(&(age - start))
        });
        if ages.contains(&age) || interesting || after_release || after_active {
            app.world_mut().resource_mut::<Owner>().input_tick = tick + 1;
            let path = PathBuf::from(&case_name).join(format!("age-{age:04}.png"));
            fs::create_dir_all(output.join(&case_name))?;
            fixture_camera(scene_mut(app), row, age)?;
            screenshot(app, output.join(&path), row, age)?;
            captures.push(json!({"age":age,"path":path,
                "moment":if observation_end {"observation_end"} else if cleanup {"cleanup"}
                    else if stage_restored {"stage_restored"} else if completion {"action_completed"}
                    else if scene_active {"scene_active"} else if release {"release"}
                    else if impact {"impact"} else {"fixed_age"},
                "action_completed_now":completion,"stage_restored_now":stage_restored}));
            progress["images"] = json!(captures);
        }
        if observation_complete {
            break;
        }
    }
    ensure!(
        admitted.is_some() && !captures.is_empty(),
        "no admitted action was rendered"
    );
    if is_unison {
        ensure!(
            matches!(scene_mut(app).core.last_unison_completion(),
            Some(resonance_battle::UnisonCompletion::CombinationExecuted { id, .. })
                if u64::from(id.0) == row["binding"]["combination"].as_u64().unwrap()),
            "admitted Unison combination did not complete: {:?}",
            scene_mut(app).core.last_unison_completion()
        );
    }
    Ok(json!({"id":row["id"], "status":"captured", "admitted":true,
        "action_key":action.0,"assignment":row.get("assignment"),"assistance":progress.get("assistance"),"images":captures,"released":released,"release_age":released_at,"active_scene_age":active_scene_at,"impacted":impacted,
        "action_completed":action_completed,"action_interrupted":action_interrupted,
        "action_terminated":action_completed||action_interrupted,"termination_reason":termination_reason,
        "cleanup_seen":cleaned,"completion_age":completion_at,
        "stage_observation":progress.get("stage_observation"),
        "observation_end_age":observation_end_at,
        "observation_budget_exhausted":observation_end_at.is_some(),
        "fixture":row.get("fixture"),"review":{"status":"pending"}}))
}

fn write_report(output: &Path, edition: &str, root: &Path, cases: &[Value]) -> Result<()> {
    let temporary = output.join("capture-report.json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(&json!({
            "source_kind":"resonance","edition":edition,"library":root,"cases":cases,
        }))?,
    )?;
    fs::rename(temporary, output.join("capture-report.json"))?;
    Ok(())
}

fn retire_case(app: &mut App) {
    if let Some(owner) = app.world_mut().remove_resource::<Owner>() {
        dispose(app.world_mut(), owner, None);
    }
    app.world()
        .resource::<crate::loading::Resident>()
        .battle
        .store(true, Ordering::Release);
    app.world_mut().resource_mut::<Messages<AppExit>>().clear();
}

fn failure_diagnostic(app: &App, row: &Value, output: &Path, receipt: &mut Value) -> Result<()> {
    let Some(owner) = app.world().get_resource::<Owner>() else {
        return Ok(());
    };
    let path = PathBuf::from(
        row["id"]
            .as_str()
            .context("failed case ID missing")?
            .replace(':', "-"),
    )
    .join("failure.json");
    fs::create_dir_all(output.join(&path).parent().unwrap())?;
    fs::write(
        output.join(&path),
        serde_json::to_vec_pretty(&json!({
            "source_kind":"resonance","case":row["id"],"battle":owner.diagnostic(),
        }))?,
    )?;
    receipt["failure_diagnostic"] = json!(path);
    Ok(())
}

fn capture_unison(
    app: &mut App,
    inputs: &CaptureInputs,
    row: &Value,
    output: &Path,
    progress: &mut Value,
) -> Result<Value> {
    let data = &inputs.data;
    let menus = &inputs.menus;
    let catalogue = &inputs.rules.unison;
    let combination = row["binding"]["combination"]
        .as_u64()
        .context("Unison combination missing")? as u8;
    if catalogue
        .combinations
        .iter()
        .find(|candidate| candidate.id == combination)
        .is_some_and(|definition| definition.recipes.is_empty())
    {
        progress["error"] = json!("source combination has no selectable recipes");
        progress["pending_evidence"] = json!("native recipe list is empty");
        return Ok(progress.clone());
    }
    let selected = row["binding"]["recipe"]
        .as_u64()
        .map(|index| index as usize);
    let plan = row.get("unison_plan");
    if let Some(plan) = plan {
        ensure!(
            plan["binding"] == row["binding"],
            "Unison plan binding differs from inventory"
        );
        if plan["planning_status"] == "pending" {
            progress["error"] = plan["pending_reason"].clone();
            progress["pending_evidence"] = plan.clone();
            return Ok(progress.clone());
        }
    }
    let all_assignments = assignments::enumerate(&catalogue, &data, menus, combination, selected)?;
    let assignments = if let Some(plan) = plan {
        let requested: Vec<(u8, u16)> = serde_json::from_value(plan["assignment"].clone())?;
        let matching = all_assignments
            .iter()
            .find(|assignment| assignment.members == requested)
            .context("canonical Unison plan assignment is not legal in current cooked rules")?;
        vec![matching.clone()]
    } else {
        all_assignments.clone()
    };
    let mut receipts = Vec::new();
    let mut images = Vec::new();
    for assignment in assignments {
        let mut selected = row.clone();
        selected["binding"]["recipe"] = json!(assignment.recipe);
        selected["assignment"] = json!(assignment.members);
        selected["parent_id"] = row["id"].clone();
        selected["id"] = json!(format!(
            "{}:recipe-{}:assignment-{}",
            row["id"].as_str().unwrap(),
            assignment.recipe,
            assignment
                .members
                .iter()
                .map(|(character, arte)| format!("{character}-{arte}"))
                .collect::<Vec<_>>()
                .join("_")
        ));
        let mut receipt = json!({"id":selected["id"],"status":"pending","admitted":false,"images":[],"review":{"status":"pending"}});
        match capture_case(app, inputs, &selected, output, &mut receipt) {
            Ok(success) => receipt = success,
            Err(error) => {
                receipt["status"] = json!("failed");
                receipt["error"] = json!(format!("{error:#}"));
                failure_diagnostic(app, &selected, output, &mut receipt)?;
            }
        }
        retire_case(app);
        for image in receipt["images"].as_array().into_iter().flatten() {
            let mut image = image.clone();
            image["assignment"] = selected["assignment"].clone();
            images.push(image);
        }
        receipts.push(receipt);
    }
    let failed = receipts
        .iter()
        .any(|receipt| receipt["status"] != "captured");
    Ok(
        json!({"id":row["id"],"status":if failed {"failed"} else {"captured"},
        "admitted":receipts.iter().any(|receipt| receipt["admitted"] == true),
        "images":images,"assignments":receipts,
        "candidate_assignments":all_assignments.iter().map(|assignment|json!({"recipe":assignment.recipe,"members":assignment.members})).collect::<Vec<_>>(),
        "unison_plan":plan,"review":{"status":"pending"}}),
    )
}

#[test]
#[ignore = "requires current catalogue/libraries and native GPU; captures real action PNGs without asserting visual fidelity"]
fn render_catalogue_artes_to_png() -> Result<()> {
    let catalogue_path =
        std::env::var_os("RESONANCE_ARTE_CATALOGUE").context("set RESONANCE_ARTE_CATALOGUE")?;
    let catalogue: Value = serde_json::from_slice(&fs::read(catalogue_path)?)?;
    let source_slots: Value = serde_json::from_slice(&fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/arte-visuals/source_slots.json"),
    )?)?;
    let unison_plan: Option<Value> = std::env::var_os("RESONANCE_ARTE_UNISON_PLAN")
        .map(|path| -> Result<_> { Ok(serde_json::from_slice(&fs::read(path)?)?) })
        .transpose()?;
    let edition = std::env::var("RESONANCE_ARTE_EDITION").context("set RESONANCE_ARTE_EDITION")?;
    let root = PathBuf::from(
        catalogue["libraries"][&edition]
            .as_str()
            .context("edition library missing")?,
    );
    let output = PathBuf::from(
        std::env::var_os("RESONANCE_ARTE_CAPTURE_OUTPUT")
            .context("set RESONANCE_ARTE_CAPTURE_OUTPUT")?,
    );
    fs::create_dir_all(&output)?;
    let selection =
        std::env::var("RESONANCE_ARTE_CASES").unwrap_or_else(|_| format!("{edition}:party:1:1"));
    let library = Library::open(&root, &[])?;
    let mut options = crate::test_support::run_options(&root);
    options.capture = Some(output.join("unused.png"));
    let (mut app, _) = crate::build_app_with_display(options, crate::Resolution::default())?;
    while app.plugins_state() == PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    let began = Instant::now();
    while !app.world().resource::<crate::timing::Ready>().0 {
        ensure!(
            began.elapsed() < Duration::from_secs(120),
            "startup GPU warmup timed out"
        );
        app.update();
        crate::playthrough::check_exit(&app)?;
        std::thread::sleep(Duration::from_millis(1));
    }
    // Keep the normal caller contract without constructing a field GPU owner.
    // The battle latch prevents field adoption while its snapshot is retained.
    app.world()
        .resource::<crate::loading::Resident>()
        .battle
        .store(true, Ordering::Release);
    let field =
        crate::new_game::FieldPackage::prepare(&root, 332, &mut Default::default(), || false)?;
    let checkpoint = crate::test_support::field_checkpoint(&field.files)?;
    let retained =
        crate::new_game::Session::load_prepared(Arc::new(field), Some(checkpoint), None)?;
    crate::new_game::activate(app.world_mut(), retained);
    let data = app
        .world()
        .resource::<crate::new_game::Session>()
        .data()
        .clone();
    let menus = data.rules.clone().context("missing retained menu rules")?;
    let rules = resonance_content::battle_rules::capture(&mut library.snapshot(), &|| false)?;
    let inputs = CaptureInputs {
        library,
        data,
        menus,
        rules,
    };
    let mut rows = Vec::new();
    for row in catalogue["cases"]
        .as_array()
        .context("catalogue cases missing")?
    {
        if row["edition"] != edition {
            continue;
        }
        rows.push(row.clone());
        for variant in row["recipe_variants"].as_array().into_iter().flatten() {
            let mut variant = variant.clone();
            variant["category"] = json!("unison");
            rows.push(variant);
        }
    }
    if selection != "all" {
        for requested in selection.split(',') {
            ensure!(
                rows.iter().any(|row| row["id"] == requested),
                "requested case {requested} is absent from the {edition} catalogue"
            );
        }
    }
    let fixture: Option<Value> = std::env::var_os("RESONANCE_ARTE_FIXTURE")
        .map(|path| -> Result<_> { Ok(serde_json::from_slice(&fs::read(path)?)?) })
        .transpose()?;
    let mut receipts = rows.iter().map(|row| json!({"id":row["id"],"status":"pending","error":"not requested","images":[],"review":{"status":"pending"}})).collect::<Vec<_>>();
    write_report(&output, &edition, &root, &receipts)?;
    let recipe_children = rows
        .iter()
        .filter_map(|row| {
            let children = row["recipe_variants"].as_array()?;
            Some((
                row["id"].as_str()?.to_owned(),
                children
                    .iter()
                    .filter_map(|child| child["id"].as_str().map(str::to_owned))
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut completed = 0;
    for (index, mut row) in rows.into_iter().enumerate() {
        let id = row["id"].as_str().context("case id missing")?.to_owned();
        let selected = selection == "all" || selection.split(',').any(|wanted| wanted == id);
        if !selected {
            continue;
        }
        if row["category"] == "unison" && row["binding"]["recipe"].is_u64() {
            if let Some(plan) = &unison_plan {
                let request = plan["requests"]
                    .as_array()
                    .context("Unison plan requests missing")?
                    .iter()
                    .find(|request| request["id"] == row["id"])
                    .context("Unison recipe is absent from requested canonical plan")?;
                row["unison_plan"] = request.clone();
            }
        }
        if let Some(fixture) = &fixture {
            let source = fixture["source_case_id"]
                .as_str()
                .context("fixture source case missing")?;
            if source.split_once(':').map(|(_, case)| case)
                == id.split_once(':').map(|(_, case)| case)
            {
                row["fixture"] = fixture.clone();
            }
        }
        let mut receipt = json!({"id":id,"status":"pending","admitted":false,"images":[],"review":{"status":"pending"}});
        let unavailable = source_slots["entries"]
            .as_array()
            .context("source slot evidence entries missing")?
            .iter()
            .find(|entry| {
                entry["scope_status"] == "source_unimplemented_slot"
                    && entry["selector"].as_object().is_some_and(|selector| {
                        !selector.is_empty()
                            && selector
                                .iter()
                                .all(|(key, value)| row.get(key) == Some(value))
                    })
            });
        if let Some(evidence) = unavailable {
            receipt["availability"] = json!("unavailable");
            receipt["error"] = json!("original source dispatch slot has no executable arte");
            receipt["pending_evidence"] = json!({
                "document":"tools/arte-visuals/source_slots.json","entry":evidence["id"],
                "scope_status":evidence["scope_status"],"conclusion":evidence["conclusion"]});
        } else if selection == "all"
            && recipe_children
                .get(&id)
                .is_some_and(|children| !children.is_empty())
        {
            receipt["error"] = json!("combination evidence is aggregated from its recipe variants");
        } else if row["category"] == "unison" {
            match capture_unison(&mut app, &inputs, &row, &output, &mut receipt) {
                Ok(success) => receipt = success,
                Err(error) => {
                    receipt["status"] = json!("failed");
                    receipt["error"] = json!(format!("{error:#}"));
                    failure_diagnostic(&app, &row, &output, &mut receipt)?;
                }
            }
        } else {
            match capture_case(&mut app, &inputs, &row, &output, &mut receipt) {
                Ok(success) => receipt = success,
                Err(error) => {
                    receipt["status"] = json!("failed");
                    receipt["error"] = json!(format!("{error:#}"));
                    failure_diagnostic(&app, &row, &output, &mut receipt)?;
                }
            }
        }
        retire_case(&mut app);
        receipts[index] = receipt;
        completed += 1;
        if completed % 8 == 0 {
            write_report(&output, &edition, &root, &receipts)?;
        }
    }
    if selection == "all" {
        for (parent, children) in &recipe_children {
            if children.is_empty() {
                continue;
            }
            let evidence = receipts
                .iter()
                .filter(|receipt| children.iter().any(|id| receipt["id"] == *id))
                .cloned()
                .collect::<Vec<_>>();
            let failed = evidence.iter().any(|receipt| receipt["status"] == "failed");
            let captured = evidence.len() == children.len()
                && evidence
                    .iter()
                    .all(|receipt| receipt["status"] == "captured");
            if let Some(receipt) = receipts.iter_mut().find(|receipt| receipt["id"] == *parent) {
                receipt["status"] = json!(if failed {
                    "failed"
                } else if captured {
                    "captured"
                } else {
                    "pending"
                });
                receipt["admitted"] = json!(evidence.iter().any(|child| child["admitted"] == true));
                receipt["images"] = json!(
                    evidence
                        .iter()
                        .flat_map(|child| child["images"].as_array().into_iter().flatten())
                        .collect::<Vec<_>>()
                );
                receipt["recipe_evidence"] = json!(children);
                if captured {
                    receipt.as_object_mut().unwrap().remove("error");
                }
            }
        }
    }
    write_report(&output, &edition, &root, &receipts)?;
    let requested = receipts
        .iter()
        .filter(|row| selection == "all" || selection.split(',').any(|id| row["id"] == id))
        .collect::<Vec<_>>();
    ensure!(
        !requested.is_empty(),
        "no requested case exists in this catalogue"
    );
    if requested.iter().any(|row| row["status"] == "failed") {
        bail!("one or more requested captures failed; see capture-report.json")
    }
    Ok(())
}
