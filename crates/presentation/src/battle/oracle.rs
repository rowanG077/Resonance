//! Catalogue execution observations through real preparation, models and effects.
//! Comparison with independently observed source traces belongs to `compare`.
use super::feedback::Feedback;
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActionId, ActionRequest, Activity, ActorId, BattleInput, BattlePhase, ContactSource, Cue,
    EffectFollow, Effects, ParticleId, ProjectileId, Side,
};
use resonance_content::{
    arte::{Catalogue, RegalArteFamily, TechniqueTarget},
    battle_action::{EnemyEvent, EnemyExecution, TargetPolicy},
    battle_enemy,
    diagnostics::Diagnostics,
    menu_data::MenuData,
    prepared::{Cache, Files},
    session::SessionData,
};
use resonance_events::{
    battle::{DefeatPolicy, Encounter, Setup},
    party::Party,
};
use resonance_game::battle::encounter::{Inputs, PrepareOptions};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
mod compare;
mod tests;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "side", rename_all = "snake_case")]
pub(super) enum Binding {
    Party { character: u8, arte: u16 },
    Enemy { enemy: u8, row: usize },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContextKind {
    Attack,
    Aerial,
    Support,
    Revival,
    Counter,
    Capture,
    Departure,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Case {
    pub id: String,
    pub binding: Binding,
    pub context: ContextKind,
}
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Trace {
    pub case: String,
    pub setup: Value,
    pub frames: Vec<Value>,
    pub events: Vec<Value>,
    pub assessment: Value,
    pub incomplete_observations: Vec<String>,
}

pub(super) struct Oracle {
    root: PathBuf,
    cache: Cache,
    retained: Files,
    menus: MenuData,
    session: SessionData,
    catalogue: Catalogue,
}
impl Oracle {
    pub fn load(root: &Path) -> Result<Self> {
        let mut cache = Cache::default();
        let retained = Files::load_with_diagnostics(
            root,
            &["fields/map-332.preload.json"],
            &mut cache,
            || false,
            Diagnostics::new(true),
        )?;
        let menus: MenuData = retained.json("game/menu-data.json")?;
        let mut session: SessionData = retained.json("game/session-data.json")?;
        session.rules = Some(Arc::new(menus.clone()));
        Ok(Self {
            root: root.into(),
            cache,
            menus,
            session,
            catalogue: retained.json(resonance_content::arte::PATH)?,
            retained,
        })
    }

    /// Enumerate authored entries, never the subset the native dispatcher accepts.
    pub fn cases(&self) -> Result<Vec<Case>> {
        let mut cases = Vec::new();
        for (index, entries) in self
            .catalogue
            .learning
            .iter()
            .take(self.session.characters.len())
            .enumerate()
        {
            let character = u8::try_from(index + 1)?;
            for &id in entries {
                if id == 0 {
                    continue;
                }
                let arte = u16::from(id);
                let capability = self.catalogue.definition(usize::from(arte))?.capabilities;
                let context = if capability.revives {
                    ContextKind::Revival
                } else if capability.healing
                    || matches!(
                        capability.target,
                        TechniqueTarget::Ally | TechniqueTarget::SelfTarget
                    )
                {
                    ContextKind::Support
                } else if capability.regal_family == Some(RegalArteFamily::Aerial) {
                    ContextKind::Aerial
                } else if !capability.offensive && capability.target == TechniqueTarget::Enemy {
                    ContextKind::Counter
                } else {
                    ContextKind::Attack
                };
                cases.push(Case {
                    id: format!("party:{character}:{arte}"),
                    binding: Binding::Party { character, arte },
                    context,
                });
            }
        }
        let mut enemies = Vec::new();
        for entry in fs::read_dir(self.root.join("battle/enemies"))? {
            let entry = entry?;
            if let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u8>().ok())
            {
                if entry.path().join("definition.json").is_file() {
                    enemies.push(id);
                }
            }
        }
        enemies.sort_unstable();
        for enemy in enemies {
            let definition: battle_enemy::Definition = self.read(&battle_enemy::path(enemy))?;
            for (row, action) in definition.actions.rows.iter().enumerate() {
                let attack = match &action.execution {
                    Some(
                        EnemyExecution::Attack(attack) | EnemyExecution::Physical { attack, .. },
                    ) => Some(attack),
                    _ => None,
                };
                let context = if matches!(action.execution, Some(EnemyExecution::LeaveArena)) {
                    ContextKind::Departure
                } else if action.target_policy == Some(TargetPolicy::NearestIncapacitated) {
                    ContextKind::Revival
                } else if attack.is_some_and(|attack| {
                    attack
                        .events
                        .iter()
                        .any(|(_, event)| matches!(event, EnemyEvent::ReleaseCaptured { .. }))
                }) {
                    ContextKind::Capture
                } else if action.target_allies
                    || action.target_policy == Some(TargetPolicy::SelfTarget)
                    || attack.is_some_and(|attack| {
                        attack.events.iter().any(|(_, event)| {
                            matches!(
                                event,
                                EnemyEvent::StatUp { .. } | EnemyEvent::UseItem { .. }
                            )
                        })
                    })
                {
                    ContextKind::Support
                } else {
                    ContextKind::Attack
                };
                cases.push(Case {
                    id: format!("enemy:{enemy}:{row}"),
                    binding: Binding::Enemy { enemy, row },
                    context,
                });
            }
        }
        Ok(cases)
    }

    fn read<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        Ok(serde_json::from_slice(
            &fs::read(self.root.join(path)).with_context(|| path.to_owned())?,
        )?)
    }

    pub fn run(&mut self, case: &Case, source: &Trace, max_updates: u32) -> Result<Trace> {
        ensure!(max_updates > 0, "empty observation budget");
        ensure!(
            source.case == case.id,
            "source trace belongs to another catalogue entry"
        );
        ensure!(
            !source.frames.is_empty(),
            "source trace has no execution window"
        );
        let setup = &source.setup;
        let formation: u16 = field(setup, "formation")?;
        let arena: u16 = field(setup, "arena")?;
        let seed: u64 = field(setup, "seed")?;
        let request_delay: u32 = field(&setup["input"], "bound_admission_after_updates")?;
        ensure!(
            request_delay > 0,
            "source request must follow its ready snapshot"
        );
        let mut party = Party::new(&self.session, Default::default())?;
        party.formation = field(setup, "party")?;
        party.field_leader = *party.formation.first().context("source party is empty")?;
        party.settings.battle_controls = [0; 4];
        party.settings.preferences.battle_rank = field(setup, "battle_rank")?;
        for member in &mut party.members {
            member.techniques.clear();
            member.disabled_techniques.clear();
            member.shortcuts = [0; 4];
            member.assist_shortcuts = [None; 2];
        }
        for member in setup["party_members"]
            .as_array()
            .context("source party equipment/progression is missing")?
        {
            let character: u8 = field(member, "character")?;
            let prepared = party
                .members
                .get_mut(usize::from(
                    character
                        .checked_sub(1)
                        .context("invalid source character")?,
                ))
                .context("invalid source character")?;
            prepared.equipment = field(member, "equipment")?;
            prepared.level = field(member, "level")?;
            prepared.base_stats = field(member, "base_stats")?;
            prepared.luck = field(member, "luck")?;
            prepared.technique_balance = field(member, "technique_balance")?;
            prepared.ex_skills = field(member, "ex_skills")?;
            prepared.ex_gems = field(member, "ex_gems")?;
            prepared.compound_ex_skills = field(member, "compound_ex_skills")?;
            prepared.techniques = field(member, "techniques")?;
            prepared.recent_compound_ex_skills.clear();
            prepared.hp = prepared.base_stats[0];
            prepared.tp = prepared.base_stats[1];
        }
        if let Binding::Party { character, arte } = case.binding {
            ensure!(
                party.members[usize::from(character - 1)]
                    .techniques
                    .contains(&arte),
                "source owner has not learned the requested arte"
            );
        }
        party.validate(&self.session)?;
        let inputs = Inputs::load(
            &self.root,
            &self.retained,
            &self.menus,
            &self.session,
            &party,
            Setup {
                route: [0; 5],
                encounter: Encounter::Formation(formation),
                arena,
                defeat: DefeatPolicy::ResumeEvent,
                music: None,
            },
            &mut self.cache,
            || false,
        )?;
        let mut selected = None;
        let prepared = inputs
            .prepare_fixture(
                &self.menus,
                PrepareOptions {
                    event_flags: field(setup, "event_flags")?,
                    devils_arms_unlocked: field(setup, "devils_arms_unlocked")?,
                    random_seed: seed,
                    map: field(setup, "map")?,
                    world_music: field(setup, "world_music")?,
                    story: field(setup, "story")?,
                    colette_state: field(setup, "colette_state")?,
                    victory_story_flags: field(setup, "victory_story_flags")?,
                    overlimit_boost: field(setup, "overlimit_boost")?,
                },
                |sound| Ok(Some(sound)),
                |participants| {
                    let owner: usize = field(setup, "owner")?;
                    let target: usize = field(setup, "target")?;
                    ensure!(
                        owner < participants.len() && target < participants.len(),
                        "source actor mapping is outside the prepared roster"
                    );
                    match case.binding {
                        Binding::Party { character, .. } => ensure!(
                            party.formation.get(owner) == Some(&character),
                            "source owner is not the selected character"
                        ),
                        Binding::Enemy { enemy, .. } => ensure!(
                            participants[owner]
                                .1
                                .enemy_decision
                                .as_ref()
                                .is_some_and(|decision| decision.enemy_id == enemy),
                            "source owner is not the selected enemy"
                        ),
                    }
                    let action = match case.binding {
                        Binding::Party { arte, .. } => participants[owner]
                            .1
                            .techniques
                            .iter()
                            .find(|technique| technique.catalogue == arte)
                            .map(|technique| technique.action)
                            .context("catalogue arte has no prepared action")?,
                        Binding::Enemy { row, .. } => {
                            participants[owner]
                                .1
                                .enemy_decision
                                .as_ref()
                                .unwrap()
                                .choices
                                .get(row)
                                .context("authored enemy row has no prepared action")?
                                .action
                        }
                    };
                    let side = participants[owner].0.side;
                    let opponent = participants
                        .iter()
                        .position(|(actor, _)| actor.side != side)
                        .context("fixture has no opposing actor")?;
                    let incoming = participants[opponent]
                        .1
                        .control
                        .as_ref()
                        .and_then(|control| control.normals.first().map(|normal| normal.action))
                        .or_else(|| {
                            participants[opponent]
                                .1
                                .enemy_decision
                                .as_ref()
                                .and_then(|decision| decision.choices.first())
                                .map(|choice| choice.action)
                        });
                    let initial = setup["actors"]
                        .as_array()
                        .context("source initial actors are missing")?;
                    ensure!(
                        initial.len() == participants.len(),
                        "source and native actor counts differ"
                    );
                    for (index, (actor, actor_setup)) in participants.iter_mut().enumerate() {
                        let observed = initial
                            .iter()
                            .find(|row| row["actor"].as_u64() == Some(index as u64))
                            .context("source actor mapping is incomplete")?;
                        actor.equipment.max_hp = field(observed, "max_hp")?;
                        actor.hp = field(observed, "hp")?;
                        actor.equipment.max_tp = field(observed, "max_tp")?;
                        actor.tp = field(observed, "tp")?;
                        actor.position = field(observed, "position")?;
                        actor.heading = field(observed, "heading")?;
                        actor.facing_direction =
                            resonance_battle::direction_from_heading(actor.heading);
                        actor.movement.direction = actor.facing_direction;
                        actor.movement.target_direction = actor.facing_direction;
                        let stats = &observed["stats"];
                        actor.equipment.stats = resonance_battle::CombatStats {
                            slash: field(stats, "slash")?,
                            thrust: field(stats, "thrust")?,
                            defense: field(stats, "defense")?,
                            intelligence: field(stats, "intelligence")?,
                            accuracy: field(stats, "accuracy")?,
                            evasion: field(stats, "evasion")?,
                            level: field(stats, "level")?,
                        };
                        if actor.side == Side::Party {
                            actor.equipment.contact.technique_balance =
                                field(stats, "technique_balance")?;
                        }
                        let mut layers = actor.conditions.layers();
                        layers.base = field(observed, "conditions")?;
                        actor.conditions.reload_layers(layers);
                        if actor.side == Side::Party {
                            let character = party.formation[index];
                            ensure!(
                                actor.equipment.stats.level
                                    == party.members[usize::from(character - 1)].level,
                                "source actor level differs from its party progression"
                            );
                        }
                        if index != owner {
                            let control = &setup["opponent_control"];
                            actor.guard.auto_disabled = field(control, "automatic_guard_disabled")?;
                            let mode: String = field(control, "mode")?;
                            ensure!(
                                mode == "manual",
                                "source opponent is not a neutral manual recipient"
                            );
                            // Enemy recovery owns a prepared common controller.
                            // Keep it valid while the disabled choice list below
                            // suppresses all ordinary action submissions.
                            if actor.control != resonance_battle::Control::Enemy {
                                actor.control = resonance_battle::Control::Manual;
                            }
                            actor.control_slot = field(control, "port")?;
                            actor_setup.recovery_return = None;
                        }
                        actor.availability = if actor.hp <= 0 {
                            resonance_battle::ActorAvailability::Dead
                        } else {
                            resonance_battle::ActorAvailability::Active
                        };
                        actor_setup.decision = None;
                        actor_setup.companion = None;
                        if let Some(decision) = &mut actor_setup.enemy_decision {
                            decision.disabled_choices = (0..decision.choices.len())
                                .map(u8::try_from)
                                .collect::<std::result::Result<_, _>>()?;
                        }
                    }
                    selected = Some((
                        ActorId::from_index(owner)?,
                        ActorId::from_index(target)?,
                        action,
                        ActorId::from_index(opponent)?,
                        incoming,
                    ));
                    Ok(())
                },
            )
            .with_context(|| format!("prepare {}", case.id))?;
        ensure!(
            !prepared.lifecycle.has_events(),
            "story-owned encounter requires an explicit event prelude"
        );
        let (owner, target, action, opponent, incoming) =
            selected.context("fixture setup did not run")?;
        let banks: BTreeMap<_, _> = prepared
            .effects
            .iter()
            .map(|effect| (effect.resource, effect.source.source_sha256.clone()))
            .collect();
        let common_effect_bank = banks
            .get(&prepared.poison_effect.resource)
            .context("prepared common effect bank is missing")?
            .clone();
        let mut feedback = Feedback::new(prepared.feedback, !seed);
        let diagnostics = Diagnostics::new(true);
        let mut effects = Effects::new(
            prepared.effect_banks,
            Some(prepared.poison_effect),
            !seed,
            diagnostics.clone(),
        )?
        .with_ambient(prepared.ambient_effect)?
        .with_passive(prepared.passive_effects)?;
        let mut candidate = resonance_game::battle::results::Candidate::new(
            prepared.results,
            prepared.core.actors(),
            party,
            Default::default(),
            Arc::new(self.session.clone()),
            Arc::new(self.menus.clone()),
            Arc::new(self.catalogue.clone()),
        )?;
        let mut lifecycle = prepared.lifecycle;
        let mut display = Display;
        let mut core = prepared.core;
        core.set_diagnostics(diagnostics.clone());
        let mut observed_setup = setup.clone();
        observed_setup["actors"] = setup_actors(core.actors());
        observed_setup["initial_action_power"] = json!(core.actors().iter().enumerate().map(|(actor, state)|
            json!({"actor":actor,"attack_power":state.attack_power,"casting_strength":state.casting_state.power_percent})).collect::<Vec<_>>());
        observed_setup["effect_banks"] = json!(banks);
        observed_setup["common_effect_bank"] = json!(common_effect_bank);
        observed_setup["native_opponent_controls"] = json!(
            core.actors()
                .iter()
                .enumerate()
                .filter(|(id, _)| *id != owner.index())
                .map(
                    |(id, actor)| json!({"actor":id,"controller":format!("{:?}", actor.control),
                "port":actor.control_slot,"ordinary_action_choices_disabled":true})
                )
                .collect::<Vec<_>>()
        );
        observed_setup["scope"] = json!({"execution":"explicit_action_request", "natural_selection":false,"audio_output":false,"rendered_pixels":false});
        let mut trace = Trace {
            case: case.id.clone(),
            setup: observed_setup,
            frames: Vec::new(),
            events: Vec::new(),
            assessment: Value::Null,
            incomplete_observations: Vec::new(),
        };
        let mut actions = BTreeMap::<ActionId, usize>::new();
        let mut projectiles = BTreeMap::<ProjectileId, usize>::new();
        let mut particles = BTreeMap::<ParticleId, usize>::new();
        let mut completed = BTreeSet::new();
        let mut expired = BTreeSet::new();
        let mut onset = None;
        let mut onset_combat = None;
        let mut raw_events = Vec::new();
        let mut model_ordinals = BTreeMap::new();
        let mut model_counts = BTreeMap::<(ActorId, u32, u16), usize>::new();
        let mut recovering = false;
        let mut idle_observed = false;
        let mut requested = false;
        let mut jumped = false;
        let mut running = false;
        let mut request_at = None;
        let mut incoming_sent = false;
        let mut damage = 0_i64;
        let mut recovery = 0_i64;
        let mut finished = false;
        let mut previous_conditions = core.actors()[target.index()].conditions.base();
        let mut projectile_actions = BTreeMap::new();
        let initial_conditions = core.actors()[target.index()].conditions.clone();
        let recipient = &core.actors()[target.index()];
        let regeneration = recipient.conditions.gear_regeneration();
        let mut support_attribution = recipient.conditions.periodic_effects().is_empty()
            && recipient
                .conditions
                .active_effects()
                .iter()
                .all(|effect| effect.remaining.is_none())
            && regeneration.hp_percent == 0
            && regeneration.tp_percent == 0
            && recipient.equipment.recovery.common == Default::default();
        for _visit in 0..max_updates {
            let mut input = BattleInput::default();
            if running
                && request_at.is_some_and(|at| core.ledger().combat_ticks.saturating_add(1) >= at)
                && onset.is_none()
                && core.phase() == BattlePhase::Combat
                && core.activity(owner) == Activity::Idle
            {
                if case.context == ContextKind::Aerial && !jumped {
                    let mut control = resonance_battle::ControlInput::neutral(owner);
                    control.stick = [0, 127];
                    control.vertical_pressed = 1;
                    input.controllers.push(control);
                } else if case.context != ContextKind::Aerial
                    || (core.activity(owner) == Activity::Jumping
                        && core.actors()[owner.index()].position[1] > 0.)
                {
                    input.actions.push(ActionRequest {
                        actor: owner,
                        action,
                        target,
                    });
                    requested = true;
                }
            }
            if running
                && request_at.is_some_and(|at| core.ledger().combat_ticks.saturating_add(1) >= at)
                && case.context == ContextKind::Aerial
                && onset.is_none()
                && (core.activity(owner) == Activity::Jumping
                    && core.actors()[owner.index()].position[1] > 0.)
            {
                input.actions.push(ActionRequest {
                    actor: owner,
                    action,
                    target,
                });
                requested = true;
            }
            if running && case.context == ContextKind::Counter && onset.is_some() && !incoming_sent
            {
                input.actions.push(ActionRequest {
                    actor: opponent,
                    action: incoming.context("counter lacks a prepared incoming action")?,
                    target: owner,
                });
            }
            let previous_tp = core.actors()[owner.index()].tp;
            let mut frame = lifecycle.step(
                &mut core,
                resonance_game::battle::lifecycle::Input {
                    battle: input,
                    ..Default::default()
                },
                &mut candidate,
                &mut display,
            )?;
            feedback.resolve(&mut frame);
            let clock = frame.clock;
            running = clock == resonance_battle::BattleClock::Running;
            let presentation = core.ledger().elapsed_ticks;
            let combat = core.ledger().combat_ticks;
            if request_at.is_none()
                && running
                && core.phase() == BattlePhase::Combat
                && core.activity(owner) == Activity::Idle
            {
                let poses = setup["ready_models"]
                    .as_array()
                    .context("source ready body poses are missing")?;
                ensure!(
                    poses.len() == frame.actors.len(),
                    "source ready pose count differs"
                );
                for (index, pose) in poses.iter().enumerate() {
                    ensure!(
                        field::<usize>(pose, "actor")? == index,
                        "source ready pose order differs"
                    );
                    let actor = ActorId::from_index(index)?;
                    let model = core
                        .models()
                        .and_then(|models| models.definition(actor))
                        .context("source ready pose has no prepared body")?;
                    frame
                        .model_requests
                        .push(resonance_battle::ModelRequest::Play {
                            actor,
                            motion: resonance_battle::MotionBinding {
                                model: model.resource,
                                clip: field(pose, "clip")?,
                            },
                            pose: resonance_battle::Pose {
                                frame: field(pose, "frame")?,
                                rate: field(pose, "rate")?,
                                repeat: field(pose, "repeat")?,
                                blend: 0,
                                ..Default::default()
                            },
                        });
                    frame
                        .model_requests
                        .push(resonance_battle::ModelRequest::LoopStart {
                            actor,
                            frame: field(pose, "loop_start")?,
                        });
                }
                request_at = Some(
                    combat
                        .checked_add(request_delay)
                        .context("source input delay overflow")?,
                );
                trace.setup["native_ready"] = json!({"presentation_tick":presentation,"combat_tick":combat,
                    "bound_admission_after_updates":request_delay});
            }
            core.advance_models(
                &mut frame,
                clock,
                clock == resonance_battle::BattleClock::Running
                    && core.phase() == BattlePhase::Combat,
            )?;
            feedback.resolve_model_effects(&mut frame)?;
            let effect_cues = effects.advance(&frame, clock.feedback_paused())?;
            frame.cues.extend(effect_cues);
            feedback.advance(&mut frame, clock.feedback_paused());
            for cue in &frame.cues {
                if matches!(cue, Cue::Jumped { actor, .. } if *actor == owner) {
                    jumped = true;
                }
                if matches!(cue, Cue::Started { actor, definition, .. } if *actor == opponent && *definition == incoming)
                {
                    incoming_sent = true;
                }
                if matches!(cue, Cue::Started { actor, .. } | Cue::Released { actor, .. } if *actor != owner)
                {
                    support_attribution = false;
                }
                let event = match cue {
                    Cue::Started {
                        definition: Some(key),
                        action: id,
                        actor,
                    } if *actor == owner && *key == action => {
                        onset.get_or_insert(presentation);
                        onset_combat.get_or_insert(combat);
                        let next = actions.len() + 1;
                        actions.entry(*id).or_insert(next);
                        Some(json!({"kind":"started","owner":owner.index(),"action":actions[id]}))
                    }
                    Cue::Released {
                        action: id,
                        parent: Some(parent),
                        actor,
                        ..
                    } if actions.contains_key(parent) => {
                        let next = actions.len() + 1;
                        actions.entry(*id).or_insert(next);
                        Some(
                            json!({"kind":"released","owner":actor.index(),"action":actions[id],"parent":actions[parent]}),
                        )
                    }
                    Cue::ProjectileStarted {
                        projectile,
                        action: id,
                    } if actions.contains_key(id) => {
                        let next = projectiles.len() + 1;
                        projectiles.entry(*projectile).or_insert(next);
                        projectile_actions.insert(*projectile, *id);
                        Some(
                            json!({"kind":"projectile_started","owner":owner.index(),"target":frame.projectiles.iter().find(|shot|shot.id==*projectile).map(|shot|shot.target.index()),"action":actions[id],"projectile":projectiles[projectile]}),
                        )
                    }
                    Cue::Hit {
                        source,
                        owner: actor,
                        actor: recipient,
                        result,
                        ..
                    } if *actor == owner && linked(*source, &actions, &projectiles) => {
                        damage += i64::from((-result.hp_change).max(0));
                        Some(
                            json!({"kind":"hit","owner":actor.index(),"target":recipient.index(),"action":contact_action(*source, &actions, &projectile_actions),"projectile":match source { ContactSource::Projectile(id)=>Some(projectiles[id]), _=>None },"hp_change":result.hp_change,"critical":result.critical}),
                        )
                    }
                    Cue::Recovered { actor, applied, .. }
                        if onset.is_some()
                            && support_attribution
                            && *actor == target
                            && matches!(
                                case.context,
                                ContextKind::Support | ContextKind::Revival
                            ) =>
                    {
                        recovery += i64::from(*applied);
                        Some(
                            json!({"kind":"recovered","owner":owner.index(),"target":actor.index(),"action":1,"amount":applied}),
                        )
                    }
                    Cue::Completed { action: id } if actions.contains_key(id) => {
                        completed.insert(*id);
                        Some(json!({"kind":"completed","action":actions[id]}))
                    }
                    Cue::ProjectileExpired { projectile }
                        if projectiles.contains_key(projectile) =>
                    {
                        expired.insert(*projectile);
                        Some(
                            json!({"kind":"projectile_expired","projectile":projectiles[projectile]}),
                        )
                    }
                    Cue::Rejected { actor, reason } if *actor == owner => Some(
                        json!({"kind":"rejected","owner":actor.index(),"reason":format!("{reason:?}")}),
                    ),
                    _ => None,
                };
                if let Some(mut event) = event {
                    event["presentation_tick"] = json!(presentation);
                    event["combat_tick"] = json!(combat);
                    if matches!(
                        event["kind"].as_str(),
                        Some("released" | "completed" | "rejected")
                    ) {
                        raw_events.push(event);
                    } else {
                        if event.get("action").is_some() {
                            event["action"] = json!(1);
                        }
                        trace.events.push(event);
                    }
                }
            }
            let current_conditions = core.actors()[target.index()].conditions.base();
            if onset.is_some() && support_attribution && current_conditions != previous_conditions {
                let event = json!({"kind":"condition_changed","owner":owner.index(),"target":target.index(),"action":1,
                    "before":previous_conditions,"after":current_conditions,"presentation_tick":presentation,"combat_tick":combat});
                trace.events.push(event);
            }
            previous_conditions = current_conditions;
            if onset.is_some() {
                let tp = core.actors()[owner.index()].tp;
                if tp < previous_tp {
                    trace.events.push(
                        json!({"kind":"payment","owner":owner.index(),"amount":previous_tp-tp,
                        "presentation_tick":presentation,"combat_tick":combat}),
                    );
                }
                if !recovering && core.activity(owner) == Activity::Recovering {
                    recovering = true;
                    trace.events.push(
                        json!({"kind":"recovery_started","owner":owner.index(),"action":1,
                        "presentation_tick":presentation,"combat_tick":combat}),
                    );
                }
                if !idle_observed && core.activity(owner) == Activity::Idle {
                    idle_observed = true;
                    trace
                        .events
                        .push(json!({"kind":"returned","owner":owner.index(),
                        "presentation_tick":presentation,"combat_tick":combat}));
                }
            }
            if onset.is_some() {
                for request in effects.started_programs() {
                    trace.events.push(json!({"kind":"program_started","owner":request.owner.index(),
                        "bank":banks.get(&request.appearance.resource),"program":request.appearance.member,
                        "presentation_tick":presentation,"combat_tick":combat}));
                }
            }
            let shown = effects.frames();
            if onset.is_some() && trace.frames.is_empty() {
                model_ordinals.clear();
                model_counts.clear();
            }
            let mut births = BTreeSet::new();
            for particle in &shown {
                let born = !particles.contains_key(&particle.id);
                if born {
                    let next = particles.len() + 1;
                    particles.insert(particle.id, next);
                }
                if particle.model.is_some() && !model_ordinals.contains_key(&particle.id) {
                    let next = model_counts
                        .entry((particle.owner, particle.resource, particle.member))
                        .or_default();
                    *next += 1;
                    model_ordinals.insert(particle.id, *next);
                }
                if onset.is_some()
                    && born
                    && births.insert((particle.owner, particle.resource, particle.member))
                {
                    trace.events.push(
                        json!({"kind":"declaration_started","owner":particle.owner.index(),
                        "bank":banks.get(&particle.resource),"member":particle.member,
                        "presentation_tick":presentation,"combat_tick":combat}),
                    );
                }
            }
            if onset.is_some() {
                trace.frames.push(json!({"presentation_tick":presentation,"combat_tick":combat,
                    "action_age":actions.keys().find_map(|id| core.action_age(*id)),
                    "actors":actors(frame.actors.iter().map(|actor|&actor.state)),
                    "models":frame.models.iter().map(|model|
                        json!({"actor":model.actor.index(),"clip":model.clip,"frame":model.frame,"world":affine(resonance_content::animation::multiply(model.world, model.bones[0]))})).collect::<Vec<_>>(),
                    "projectiles":frame.projectiles.iter().filter(|shot|projectiles.contains_key(&shot.id)).map(|shot|
                        json!({"id":projectiles[&shot.id],"owner":shot.owner.index(),"target":shot.target.index(),"position":shot.position,"velocity":shot.velocity,"age":shot.age,"contact_active":shot.contact_active,"disarmed":shot.disarmed})).collect::<Vec<_>>(),
                    "effect_models":shown.iter().filter_map(|particle|particle.model.as_ref().map(|model|
                        json!({"owner":particle.owner.index(),"bank":banks.get(&particle.resource),"member":particle.member,
                            "ordinal":model_ordinals[&particle.id],"attachment":attachment(effects.particle_attachment(particle.id),particle.owner_bone,particle.owner,&projectiles),
                            "model":{"clip":model.clip,"frame":model.clip.map(|_|model.frame),"world":affine(model.world)}}))).collect::<Vec<_>>(),
                    "particles":shown.iter().map(|particle|
                        json!({"id":particles[&particle.id],"owner":particle.owner.index(),"bank":banks.get(&particle.resource),"member":particle.member,"origin":particle.origin,"offset":particle.state.offset,"owner_bone":particle.owner_bone,
                            "model":particle.model.as_ref().map(|model|json!({"clip":model.clip,"frame":model.clip.map(|_|model.frame),"world":affine(model.world)}))})).collect::<Vec<_>>() }));
            }
            let returned = onset.is_some() && core.activity(owner) == Activity::Idle;
            if trace.frames.len() == source.frames.len() {
                finished = returned
                    && !actions.is_empty()
                    && actions.keys().all(|id| completed.contains(id))
                    && projectiles.keys().all(|id| expired.contains(id));
                break;
            }
            if frame.outcome.is_some() {
                break;
            }
        }
        let condition_changed = core.actors()[target.index()].conditions != initial_conditions;
        if let (Some(presentation), Some(combat)) = (onset, onset_combat) {
            for event in &mut trace.events {
                let object = event.as_object_mut().unwrap();
                let p = object
                    .remove("presentation_tick")
                    .unwrap()
                    .as_u64()
                    .unwrap();
                let c = object.remove("combat_tick").unwrap().as_u64().unwrap();
                object.insert(
                    "presentation_clock".into(),
                    json!(p - u64::from(presentation)),
                );
                object.insert("clock".into(), json!(c - u64::from(combat)));
            }
        }
        trace.assessment = json!({
            "outcome":case.context,
            "diagnostics":diagnostics.entries(),"preparation_diagnostics":inputs.files.diagnostics().entries(),
            "raw_events":raw_events,"onset":onset,"requested":requested,"damage":damage,"recovery":recovery,
            "action_count":actions.len(),"completed_actions":completed.len(),
            "projectile_count":projectiles.len(),"expired_projectiles":expired.len(),"particle_count":particles.len()});
        if matches!(case.context, ContextKind::Support | ContextKind::Revival)
            && !support_attribution
        {
            trace
                .incomplete_observations
                .push("support_attribution_has_competing_action_or_passive_timer".into());
        }
        if onset.is_none() {
            trace
                .incomplete_observations
                .push("selected_action_never_admitted".into());
        } else if !finished {
            trace
                .incomplete_observations
                .push("execution_or_cleanup_not_completed_within_budget".into());
        }
        if matches!(
            case.context,
            ContextKind::Attack | ContextKind::Aerial | ContextKind::Capture
        ) && damage == 0
        {
            trace
                .incomplete_observations
                .push("selected_action_did_not_damage_an_opponent".into());
        }
        if matches!(case.context, ContextKind::Support | ContextKind::Revival)
            && recovery == 0
            && !condition_changed
        {
            trace
                .incomplete_observations
                .push("support_benefit_not_observed".into());
        }
        if matches!(
            case.context,
            ContextKind::Counter
                | ContextKind::Capture
                | ContextKind::Revival
                | ContextKind::Departure
        ) {
            trace
                .incomplete_observations
                .push("special_outcome_observer_not_prepared".into());
        }
        Ok(trace)
    }
}
fn attachment(
    follow: Option<EffectFollow>,
    bone: Option<u8>,
    owner: ActorId,
    projectiles: &BTreeMap<ProjectileId, usize>,
) -> Value {
    if let Some(bone) = bone {
        return json!({"kind":"bone","actor":owner.index(),"bone":bone});
    }
    match follow {
        None => Value::Null,
        Some(EffectFollow::Actor(actor)) => json!({"kind":"actor","actor":actor.index()}),
        Some(EffectFollow::Center(actor)) => json!({"kind":"center","actor":actor.index()}),
        Some(EffectFollow::Bone { actor, bone }) => {
            json!({"kind":"bone","actor":actor.index(),"bone":bone})
        }
        Some(EffectFollow::Projectile(id)) => {
            json!({"kind":"projectile","projectile":projectiles.get(&id)})
        }
        Some(EffectFollow::Spell(_)) => json!({"kind":"spell","action":1}),
    }
}
fn linked(
    source: ContactSource,
    actions: &BTreeMap<ActionId, usize>,
    projectiles: &BTreeMap<ProjectileId, usize>,
) -> bool {
    match source {
        ContactSource::Melee { action, .. } | ContactSource::Weapon { action, .. } => {
            actions.contains_key(&action)
        }
        ContactSource::Projectile(id) => projectiles.contains_key(&id),
    }
}
fn contact_action(
    source: ContactSource,
    actions: &BTreeMap<ActionId, usize>,
    projectiles: &BTreeMap<ProjectileId, ActionId>,
) -> usize {
    let id = match source {
        ContactSource::Melee { action, .. } | ContactSource::Weapon { action, .. } => action,
        ContactSource::Projectile(id) => projectiles[&id],
    };
    actions[&id]
}
fn actors<'a>(actors: impl IntoIterator<Item = &'a resonance_battle::Actor>) -> Value {
    json!(
        actors
            .into_iter()
            .enumerate()
            .map(|(index, actor)| json!({"actor":index,
        "side":if actor.side==Side::Party {0}else{1},"hp":actor.hp,"tp":actor.tp,
        "position":actor.position,"heading":actor.heading}))
            .collect::<Vec<_>>()
    )
}
fn setup_actors(source: &[resonance_battle::Actor]) -> Value {
    let mut result = actors(source);
    for (value, actor) in result.as_array_mut().unwrap().iter_mut().zip(source) {
        value["max_hp"] = json!(actor.equipment.max_hp);
        value["max_tp"] = json!(actor.equipment.max_tp);
        value["conditions"] = json!(actor.conditions.base());
        value["stats"] = json!({"slash":actor.equipment.stats.slash,"thrust":actor.equipment.stats.thrust,
            "defense":actor.equipment.stats.defense,"intelligence":actor.equipment.stats.intelligence,
            "accuracy":actor.equipment.stats.accuracy,"evasion":actor.equipment.stats.evasion,"level":actor.equipment.stats.level,
            "technique_balance":(actor.side==Side::Party).then_some(actor.equipment.contact.technique_balance)});
    }
    result
}
fn affine(matrix: resonance_content::animation::Matrix) -> [f32; 12] {
    std::array::from_fn(|index| matrix[index % 4][index / 4])
}
fn field<T: serde::de::DeserializeOwned>(value: &Value, key: &str) -> Result<T> {
    serde_json::from_value(
        value
            .get(key)
            .with_context(|| format!("source setup lacks {key}"))?
            .clone(),
    )
    .with_context(|| format!("invalid source setup field {key}"))
}
struct Display;
impl resonance_game::battle::results::Presentation for Display {
    fn request(
        &mut self,
        _: resonance_game::battle::lifecycle::Request,
        _: Option<&resonance_game::battle::results::Results>,
        _: &resonance_battle::Battle,
    ) -> Result<()> {
        Ok(())
    }
}
