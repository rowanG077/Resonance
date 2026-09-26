//! Long-lived authored decisions share the action VM without occupying an action.
use crate::{ActionPhase, ActorId, Battle, Cue, PreparedBattle, Side};
use anyhow::{Context, Result, ensure};

#[cfg(test)]
mod entry_voice_tests;

#[derive(Debug, Clone, Copy, Default)]
pub struct EntryTimers {
    pub idle_variation: u16,
    pub fidget_ticks: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct EntryChoice {
    pub actor: ActorId,
    pub strategy: u8,
    /// Enemy setup chooser, using the same authored selection as ordinary AI.
    pub action: Option<u16>,
    pub idle_ticks: u16,
    pub idle_variation: u16,
}

/// Observations used by the authored 10A8 selector after actor initialization.
#[derive(Debug, Clone, Copy)]
pub struct EntryVoiceDefinition {
    pub action: u16,
    pub repeated_formation: bool,
    pub major_enemy: bool,
    pub enemy_count: u8,
    pub level_difference: i8,
}

#[derive(Debug, Clone, Copy)]
pub struct DecisionDefinition {
    pub actor: ActorId,
    pub target: ActorId,
    /// Prepared ActionPhase::Decision task.
    pub action: u16,
    pub idle_ticks: u16,
    pub idle_variation: u16,
    pub fidget_ticks: u16,
    pub idle_motion: Option<crate::MotionBinding>,
}

#[derive(Debug, Clone)]
pub(crate) struct Decision {
    pub definition: DecisionDefinition,
    pub started: bool,
}

impl Decision {
    pub fn new(definition: DecisionDefinition) -> Self {
        Self {
            definition,
            started: false,
        }
    }
}

impl PreparedBattle {
    pub fn with_entry_voice(mut self, definition: EntryVoiceDefinition) -> Result<Self> {
        ensure!(
            (1..=8).contains(&definition.enemy_count)
                && usize::from(definition.enemy_count)
                    == self
                        .actors
                        .iter()
                        .filter(|actor| actor.side == Side::Enemy)
                        .count()
                && (-8..=8).contains(&definition.level_difference)
                && self
                    .actions
                    .iter()
                    .any(|action| action.id == definition.action
                        && action.phase == ActionPhase::Decision
                        && action.tp_cost == 0),
            "invalid entry voice definition"
        );
        self.entry_voice = Some(definition);
        Ok(self)
    }

    pub fn initial_targets(&self) -> &[ActorId] {
        &self.targets
    }
    pub fn initial_actors(&self) -> &[crate::Actor] {
        &self.actors
    }

    /// 40C8 links one actor at a time, runs its initial selector, then sets its
    /// facing. Unlinked allies have null targets and must not claim a candidate.
    pub fn with_entry_choices(self, choices: Vec<EntryChoice>) -> Result<Self> {
        ensure!(
            choices.len() == self.actors.len(),
            "entry choice/actor count differs"
        );
        let mut seen = std::collections::BTreeSet::new();
        for choice in &choices {
            ensure!(
                choice.actor.index() < self.actors.len() && seen.insert(choice.actor),
                "invalid entry choice actor"
            );
            ensure!(
                matches!(choice.strategy, 1 | 3 | 4 | 9),
                "unsupported initial target strategy"
            );
            if let Some(action) = choice.action {
                ensure!(
                    self.actions.iter().any(|a| a.id == action
                        && a.phase == ActionPhase::Decision
                        && a.tp_cost == 0),
                    "unprepared entry choice task"
                );
            }
        }
        let mut battle = Battle::new(std::sync::Arc::new(self));
        battle.entry_linked = Some(vec![false; battle.actors.len()]);
        for side in [Side::Party, Side::Enemy] {
            for index in 0..battle.actors.len() {
                if battle.actors[index].side != side {
                    continue;
                }
                let actor = ActorId(index as u8);
                let choice = choices.iter().find(|c| c.actor == actor).unwrap();
                let opposing_leader = battle
                    .actors
                    .iter()
                    .position(|a| a.side != side)
                    .context("initial targeting needs both sides")?;
                battle.targets[index] = ActorId(opposing_leader as u8);
                battle.entry_linked.as_mut().unwrap()[index] = true;
                let target = battle.decision_target(actor, choice.strategy)?;
                battle.set_decision_target(actor, target)?;
                if let Some(action) = choice.action {
                    let definition = battle
                        .prepared
                        .actions
                        .iter()
                        .position(|a| a.id == action)
                        .unwrap();
                    let (id, mut sequence) = battle.allocate_sequence(definition, actor, target)?;
                    let mut cues = Vec::new();
                    crate::script::step_sequence(&mut battle, id, &mut sequence, &mut cues)?;
                    ensure!(
                        sequence.tasks_complete() && cues.is_empty(),
                        "entry chooser must finish without admitting an action"
                    );
                } else if battle.actors[index].control == crate::Control::Manual {
                    let extra = if choice.idle_variation == 0 {
                        0
                    } else {
                        battle.random.next() % choice.idle_variation
                    };
                    battle.idle_timers[index] = choice.idle_ticks.wrapping_add(extra) as i16;
                }
                let target = battle.actors[battle.targets[index].index()].position;
                let owner = &mut battle.actors[index];
                let direction = crate::distance::normalize([
                    target[0] - owner.position[0],
                    target[1] - owner.position[1],
                    target[2] - owner.position[2],
                ]);
                owner.movement.direction = direction;
                owner.facing_direction = direction;
                crate::control::face(owner, direction, 0.);
            }
        }
        let leader = battle
            .actors
            .iter()
            .position(|a| a.side == Side::Party)
            .context("entry needs party leader")?;
        let target = battle.decision_target(ActorId(leader as u8), 1)?;
        battle.set_decision_target(ActorId(leader as u8), target)?;
        let target_position = battle.actors[target.index()].position;
        let owner = &mut battle.actors[leader];
        let direction = crate::distance::normalize([
            target_position[0] - owner.position[0],
            target_position[1] - owner.position[1],
            target_position[2] - owner.position[2],
        ]);
        owner.movement.direction = direction;
        owner.facing_direction = direction;
        crate::control::face(owner, direction, 0.);
        for (model, actor) in battle.models.iter_mut().zip(&mut battle.actors) {
            if let Some(model) = model {
                model.initialize_entry_placement(actor)?;
            }
        }
        // 40C8's tail runs 10A8 after every actor's initial target/action choice.
        // Use the same prepared action host and retain its pending voice request.
        if let Some(voice) = battle.prepared.entry_voice {
            let definition = battle
                .prepared
                .actions
                .iter()
                .position(|a| a.id == voice.action)
                .unwrap();
            let actor = ActorId(leader as u8);
            let (id, mut sequence) =
                battle.allocate_sequence(definition, actor, battle.targets[leader])?;
            let mut cues = Vec::new();
            crate::script::step_sequence(&mut battle, id, &mut sequence, &mut cues)?;
            ensure!(
                sequence.tasks_complete() && cues.is_empty(),
                "entry voice must finish without advancing actors"
            );
        }
        let mut prepared = std::sync::Arc::try_unwrap(battle.prepared)
            .map_err(|_| anyhow::anyhow!("entry preparation was retained"))?;
        prepared.actors = battle.actors;
        prepared.models = battle.models;
        prepared.targets = battle.targets;
        prepared.enemy_selected = battle.enemy_selected;
        prepared.voices = battle.voices;
        prepared.random_seed = battle.random.state();
        for (index, definition) in prepared.decisions.iter_mut().enumerate() {
            if let Some(definition) = definition {
                definition.target = prepared.targets[index];
            }
        }
        for (index, definition) in prepared.controls.iter_mut().enumerate() {
            if let Some(definition) = definition {
                definition.target = prepared.targets[index];
            }
        }
        Ok(prepared)
    }
    pub fn with_initial_targets(mut self, targets: Vec<usize>) -> Result<Self> {
        ensure!(
            targets.len() == self.actors.len() && targets.iter().all(|&i| i < self.actors.len()),
            "invalid initial target roster"
        );
        self.targets = targets.into_iter().map(|i| ActorId(i as u8)).collect();
        Ok(self)
    }

    pub fn with_entry_timers(mut self, timers: Vec<EntryTimers>) -> Result<Self> {
        ensure!(
            timers.len() == self.actors.len(),
            "entry timer/actor count differs"
        );
        ensure!(
            timers
                .iter()
                .all(|t| t.fidget_ticks <= i16::MAX as u16 && t.idle_variation <= i16::MAX as u16),
            "entry timer exceeds source clock"
        );
        self.entry_timers = timers;
        Ok(self)
    }

    pub fn with_decisions(mut self, definitions: Vec<DecisionDefinition>) -> Result<Self> {
        for definition in definitions {
            let index = definition.actor.index();
            ensure!(
                index < self.actors.len() && definition.target.index() < self.actors.len(),
                "invalid decision actor"
            );
            ensure!(self.decisions[index].is_none(), "duplicate decision actor");
            ensure!(
                definition.idle_ticks <= i16::MAX as u16
                    && definition.idle_variation <= i16::MAX as u16
                    && definition.fidget_ticks <= i16::MAX as u16,
                "decision timer exceeds source clock"
            );
            ensure!(
                self.actions.iter().any(|a| a.id == definition.action
                    && a.phase == ActionPhase::Decision
                    && a.tp_cost == 0),
                "unprepared decision task"
            );
            if let Some(motion) = definition.idle_motion {
                self.models[index]
                    .as_ref()
                    .context("idle motion needs actor model")?
                    .duration(motion)?;
            }
            self.targets[index] = definition.target;
            self.entry_timers[index] = EntryTimers {
                idle_variation: definition.idle_variation,
                fidget_ticks: definition.fidget_ticks,
            };
            self.decisions[index] = Some(definition);
        }
        Ok(self)
    }
}

impl Battle {
    pub fn fidget_clock(&self, actor: ActorId) -> Option<u16> {
        self.fidget_timers
            .get(actor.index())
            .map(|&clock| clock as u16)
    }
    /// Source 3EA4 overwrites, rather than adds, the initial delay. The entry
    /// preparation chooser has already consumed its own random draws.
    pub(crate) fn initialize_entry_delays(&mut self) {
        for side in [Side::Party, Side::Enemy] {
            for index in 0..self.actors.len() {
                if self.actors[index].side != side {
                    continue;
                }
                let variation = self.prepared.entry_timers[index].idle_variation;
                let delay = if variation == 0 {
                    0
                } else {
                    self.random.next() % variation
                };
                self.idle_timers[index] = delay as i16;
            }
        }
    }

    pub(crate) fn decision_mut(&mut self, actor: ActorId) -> Result<&mut Decision> {
        self.decisions[actor.index()]
            .as_mut()
            .context("actor has no authored decision")
    }

    pub(crate) fn initialize_decision(&mut self, actor: ActorId) -> Result<()> {
        let Some(decision) = &self.decisions[actor.index()] else {
            return Ok(());
        };
        if decision.started || !self.actors[actor.index()].available() {
            return Ok(());
        }
        let definition = self
            .prepared
            .actions
            .iter()
            .position(|a| a.id == decision.definition.action)
            .context("missing decision task")?;
        let (id, sequence) =
            self.allocate_sequence(definition, actor, self.targets[actor.index()])?;
        self.sequences.insert(id, sequence);
        self.decision_mut(actor)?.started = true;
        Ok(())
    }

    pub(crate) fn decision_ready(&self, actor: ActorId) -> bool {
        self.actors[actor.index()].available()
            && self.actors[actor.index()].activity == crate::Activity::Idle
            && self.actors[actor.index()]
                .reaction
                .idle_initialization
                .is_none()
            && !self
                .sequences
                .values()
                .any(|s| s.actor == actor && s.definition.phase.is_actor())
            && self.transition_owner().is_none()
            && self.terminal.result.is_none()
    }

    pub(crate) fn idle_initializing(&self, actor: ActorId) -> bool {
        let actor = &self.actors[actor.index()];
        actor.available()
            && actor.activity == crate::Activity::Idle
            && actor.reaction.idle_initialization.is_some()
    }

    /// 31B68 runs after the control callback, so the initialization visit cannot
    /// also choose an attack. Guard recovery enters state 3 and never comes here.
    pub(crate) fn initialize_idle(&mut self, actor: ActorId) -> Result<()> {
        ensure!(
            self.idle_initializing(actor),
            "actor is not initializing idle"
        );
        let index = actor.index();
        let hurt = self.actors[index]
            .reaction
            .idle_initialization
            .take()
            .unwrap();
        if matches!(
            self.actors[index].control,
            crate::Control::Auto | crate::Control::Enemy
        ) && let Some(decision) = &self.decisions[index]
        {
            let definition = decision.definition;
            let extra = if definition.idle_variation == 0 {
                0
            } else {
                self.random.next() % definition.idle_variation
            };
            let mut delay = definition.idle_ticks.wrapping_add(extra) as i16;
            if hurt {
                delay = (delay >> 3).max(15);
            }
            self.idle_timers[index] = delay;
        }
        self.actors[index].movement.forward = 0.;
        Ok(())
    }

    /// An explicit Recover completes through 301A4 reason 5. Bare task
    /// completion has no source recovery transition or guard random draw.
    pub(crate) fn complete_ordinary_action(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        // 301A4 copies18E4 back to18F0 before choosing its completion branch.
        self.actors[actor.index()].movement.direction = self.actors[actor.index()].facing_direction;
        if self.begin_recovery_return(actor, cues)? {
            return Ok(());
        }
        // 295B8 reason 5 keeps the recovery motion already bound by the action.
        self.reset_ordinary_actor(actor);
        Ok(())
    }

    pub(crate) fn reset_ordinary_actor(&mut self, actor: ActorId) {
        let index = actor.index();
        crate::reaction::recover(&mut self.actors[index], &mut self.random, &mut self.ledger);
        self.idle_timers[index] = self.actors[index].reaction.remaining;
        self.restore_idle_expression(index);
    }

    pub(crate) fn initialize_unhandled_idle(&mut self, actor: ActorId) -> Result<()> {
        if !self.idle_initializing(actor) {
            return Ok(());
        }
        self.initialize_idle(actor)?;
        let index = actor.index();
        if let Some(motions) = self.prepared.controls[index]
            .as_ref()
            .and_then(|d| d.motions)
            && let Some(model) = &mut self.models[index]
            && (model.finished()
                || model.is_playing(motions.walk)?
                || model.is_playing(motions.stop)?)
        {
            self.play_idle_pose(actor, 8)?;
        }
        Ok(())
    }

    pub(crate) fn decision_target(&mut self, owner: ActorId, policy: u8) -> Result<ActorId> {
        let actors: Vec<_> = self
            .actors
            .iter()
            .enumerate()
            .map(|(index, a)| crate::TargetActor {
                side: a.side,
                position: self.target_positions[index],
                available: a.available(),
                hidden: false,
                dying: a.activity == crate::Activity::Defeated,
                target: self
                    .entry_linked
                    .as_ref()
                    .is_none_or(|linked| linked[index])
                    .then(|| {
                        self.target(ActorId(index as u8))
                            .unwrap_or(self.targets[index])
                            .index()
                    }),
            })
            .collect();
        let target = ActorId(
            crate::select_target(&actors, owner.index(), policy, &mut || self.random.next())? as u8,
        );
        // 36A94 refreshes the indicator only when selection changes.
        if self.target(owner) != Some(target) {
            self.actors[owner.index()].hud.target_highlight = 120;
        }
        Ok(target)
    }

    pub(crate) fn set_decision_target(&mut self, owner: ActorId, target: ActorId) -> Result<()> {
        ensure!(
            target.index() < self.actors.len(),
            "invalid decision target"
        );
        self.targets[owner.index()] = target;
        if let Some(control) = &mut self.controls[owner.index()] {
            control.target = target;
        }
        Ok(())
    }

    pub(crate) fn admit_decision(
        &mut self,
        actor: ActorId,
        target: ActorId,
        action: u16,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        ensure!(
            self.prepared
                .actions
                .iter()
                .any(|a| a.id == action
                    && matches!(a.phase, ActionPhase::Actor | ActionPhase::Casting)),
            "decision admission requires a prepared actor action"
        );
        if !self.decision_ready(actor) {
            return Ok(false);
        }
        let before = cues.len();
        self.start(
            crate::ActionRequest {
                actor,
                target,
                action,
            },
            cues,
        )?;
        Ok(cues[before..]
            .iter()
            .any(|cue| matches!(cue, Cue::Started { .. })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, sync::Arc};

    #[test]
    fn idle_target_facing_uses_retained_centers_before_live_positions() -> Result<()> {
        // Original route01 C298: Genis is stationary, Zombie moves later in
        // the actor list.31C88 samples retained C296 centers before32298 faces.
        let mut owner = crate::tests::actor(Side::Party);
        owner.control = crate::Control::Auto;
        owner.position = [-700., 0., 150.];
        owner.heading = f32::from_bits(0x42c8_927a); // Source C297.
        let mut target = crate::tests::actor(Side::Enemy);
        target.position = [0x4387_df5a, 0, 0xc1d5_9d88].map(f32::from_bits); // C297 root.
        let prepared = crate::tests::prepared(
            "pub task run() { while true { battle::ai_face_target(22.5); await battle::next_update(); } }",
            vec![owner, target],
            0,
        );
        let mut prepared = Arc::try_unwrap(prepared).unwrap();
        prepared.actions[0].phase = ActionPhase::Decision;
        prepared.actions[0].tp_cost = 0;
        let prepared = prepared.with_decisions(vec![DecisionDefinition {
            actor: ActorId(0),
            target: ActorId(1),
            action: 99,
            idle_ticks: 30,
            idle_variation: 0,
            fidget_ticks: 0,
            idle_motion: None,
        }])?;
        let mut battle = Battle::new(Arc::new(prepared));
        battle.actors[0].body.center = [-700., 65., 150.];
        battle.actors[1].body.center = [0x4388_8592, 0x42c3_8000, 0xc1d6_19bd].map(f32::from_bits); // C296 root.
        let random = battle.random_state();
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[0].heading.to_bits(), 0x42c8_9796); // Source C298.
        // The target's later visit has now refreshed its center to C297.
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.actors[0].heading.to_bits(), 0x42c8_9cb5); // Source C299.
        assert_eq!(battle.random_state(), random);
        Ok(())
    }

    #[test]
    fn camera_arrival_initializes_delay_once_then_visits_decision() -> Result<()> {
        let sources = BTreeMap::from([("decision".into(), "script battle; use battle; pub task decide() { while true { battle::ai_set_idle_timer(battle::ai_idle_timer() - 1); await battle::next_update(); } }".into())]);
        let compiled = symphonia_script_compiler::compile(
            "decision",
            &sources,
            &crate::native_declarations(),
        )?;
        let entry = compiled.program.authored().unwrap().functions[0].entry;
        let mut actors = vec![
            crate::tests::actor(Side::Party),
            crate::tests::actor(Side::Enemy),
        ];
        actors[0].position[0] = -300.;
        actors[1].position[0] = 300.;
        actors[1].control = crate::Control::Enemy;
        let prepared = PreparedBattle::new(
            actors,
            vec![crate::ActionDefinition {
                id: 0,
                phase: ActionPhase::Decision,
                program: Arc::new(compiled.program),
                entry,
                duration: 0,
                tp_cost: 0,
                resources: vec![],
            }],
            0,
            vec![],
            vec![],
        )?
        .with_entry_timers(vec![
            EntryTimers {
                idle_variation: 10,
                fidget_ticks: 17,
            },
            EntryTimers {
                idle_variation: 25,
                fidget_ticks: 99,
            },
        ])?
        .with_decisions(vec![DecisionDefinition {
            actor: ActorId(1),
            target: ActorId(0),
            action: 0,
            idle_ticks: 75,
            idle_variation: 25,
            fidget_ticks: 99,
            idle_motion: None,
        }])?
        .with_entry_camera(
            crate::CameraDefinition {
                leader: ActorId(0),
                target: ActorId(1),
                stage_pitch: -1.,
                adaptive: true,
                initial: crate::CameraPose {
                    eye: [0., 490., 2050.],
                    focus: [0., 120., 0.],
                    pitch: -1.,
                    yaw: 85.,
                    radius: 2050.,
                },
            },
            crate::EntryCamera {
                initial_yaw: 85.,
                radius: 4200.,
                focus_x: 700.,
                focus_speed_scale: 1. / 59.,
            },
        )?;
        let mut battle = Battle::new(Arc::new(prepared));
        let mut visits = 0;
        while battle.entry_pending() {
            battle.step(crate::BattleInput::default())?;
            visits += 1;
            if battle.entry_pending() {
                assert_eq!(battle.random_state(), 0);
                assert!(battle.sequences.is_empty());
            }
            assert!(visits <= 60);
        }
        assert_eq!(battle.random_state(), 0xf8df_5002);
        assert_eq!(battle.idle_timers, [8, 10]);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.random_state(), 0xf8df_5002);
        assert_eq!(battle.idle_timers, [8, 9]);
        Ok(())
    }

    fn cadence_battle(activity: crate::Activity) -> Result<Battle> {
        let sources = BTreeMap::from([(
            "cadence".into(),
            r#"
            script battle; use battle;
            pub task attack() { await battle::recover(ticks(0)); battle::finish(); }
            pub task decide() {
                while true {
                    if battle::ai_idle_initializing() { battle::ai_reset_idle(); }
                    else if battle::ai_ready() {
                        if battle::ai_idle_timer() > 0 {
                            battle::ai_set_idle_timer(battle::ai_idle_timer() - 1);
                        } else { battle::ai_set_idle_timer(-100); }
                    }
                    await battle::next_update();
                }
            }
        "#
            .into(),
        )]);
        let compiled =
            symphonia_script_compiler::compile("cadence", &sources, &crate::native_declarations())?;
        let entry = |name: &str| {
            compiled
                .program
                .authored()
                .unwrap()
                .functions
                .iter()
                .find(|f| f.name == name || f.name.ends_with(&format!("::{name}")))
                .unwrap()
                .entry
        };
        let attack_entry = entry("attack");
        let decision_entry = entry("decide");
        let program = Arc::new(compiled.program);
        let actions = vec![
            crate::ActionDefinition {
                id: 7,
                phase: ActionPhase::Actor,
                program: Arc::clone(&program),
                entry: attack_entry,
                duration: 1,
                tp_cost: 0,
                resources: vec![],
            },
            crate::ActionDefinition {
                id: 8,
                phase: ActionPhase::Decision,
                program,
                entry: decision_entry,
                duration: 0,
                tp_cost: 0,
                resources: vec![],
            },
        ];
        let mut actors = vec![
            crate::tests::actor(Side::Party),
            crate::tests::actor(Side::Enemy),
        ];
        actors[1].control = crate::Control::Enemy;
        actors[1].activity = activity;
        let prepared =
            PreparedBattle::new(actors, actions, 0, vec![], vec![])?.with_decisions(vec![
                DecisionDefinition {
                    actor: ActorId(1),
                    target: ActorId(0),
                    action: 8,
                    idle_ticks: 75,
                    idle_variation: 25,
                    fidget_ticks: 0,
                    idle_motion: None,
                },
            ])?;
        Ok(Battle::new(Arc::new(prepared)))
    }

    #[test]
    fn ordinary_completion_resets_once_then_initializes_without_counting_down() -> Result<()> {
        let mut battle = cadence_battle(crate::Activity::Idle)?;
        battle.actors[1].movement.forward = 8.;
        battle.actors[1].movement.acceleration = 2.;
        battle.step(crate::BattleInput {
            actions: vec![crate::ActionRequest {
                actor: ActorId(1),
                target: ActorId(0),
                action: 7,
            }],
            ..Default::default()
        })?;
        assert_eq!(
            battle.random_state(),
            0,
            "recovery begins without an idle reset"
        );
        battle.step(crate::BattleInput::default())?;
        assert_eq!(
            battle.random_state(),
            0x0012_d687,
            "completion consumes guard reset on this visit"
        );
        assert_eq!(battle.actors[1].guard.auto_chance, 78);
        assert_eq!(battle.actors[1].movement.forward, 0.);
        assert_eq!(battle.actors[1].movement.acceleration, 0.);
        assert_eq!(battle.actors[1].reaction.idle_initialization, Some(false));
        assert_eq!(battle.idle_timers[1], 0);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.random_state(), 0xf8df_5002);
        assert_eq!(
            battle.idle_timers[1], 86,
            "75 + 63711 % 25; no decision on initializer visit"
        );
        assert_eq!(battle.actors[1].reaction.idle_initialization, None);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.idle_timers[1], 85);
        assert_eq!(
            battle.random_state(),
            0xf8df_5002,
            "no stale controller cleanup draw"
        );
        Ok(())
    }

    #[test]
    fn task_finish_and_expiry_do_not_enter_recovery_but_explicit_recover_does() -> Result<()> {
        for (body, recovery) in [
            ("battle::finish();", false),
            ("return;", false),
            ("await battle::recover(ticks(0)); battle::finish();", true),
        ] {
            let mut actor = crate::tests::actor(Side::Party);
            actor.guard.auto_chance = 41;
            actor.movement.forward = 3.;
            actor.movement.braking = 0.;
            // No controller/model binding: a real recovery must still perform
            // the unconditional source 2B18C guard draw and movement reset.
            let mut prepared = crate::tests::prepared(
                &format!("pub task run() {{ {body} }}"),
                vec![actor, crate::tests::actor(Side::Enemy)],
                0,
            );
            Arc::get_mut(&mut prepared).unwrap().actions[0].phase = ActionPhase::Actor;
            let seed = prepared.random_seed;
            let mut battle = Battle::new(prepared);
            let first = battle.step(crate::BattleInput {
                actions: vec![crate::ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(1),
                    action: 99,
                }],
                ..Default::default()
            })?;
            assert_eq!(battle.random_state(), seed);
            assert_eq!(battle.actors[0].guard.auto_chance, 41);
            assert_eq!(battle.actors[0].reaction.idle_initialization, None);
            if recovery {
                assert_eq!(battle.actors[0].activity, crate::Activity::Recovering);
                assert!(!first.actions.is_empty());
                let recovered = battle.step(crate::BattleInput::default())?;
                assert!(recovered.actions.is_empty());
                assert_eq!(
                    battle.random_state(),
                    seed.wrapping_mul(0x41c6_4e6d).wrapping_add(0x12d687)
                );
                assert_eq!(battle.actors[0].movement.forward, 0.);
                assert_eq!(battle.actors[0].reaction.idle_initialization, Some(false));
            } else {
                assert!(first.actions.is_empty());
                assert_eq!(battle.actors[0].activity, crate::Activity::Idle);
                assert_eq!(battle.actors[0].movement.forward, 3.);
                battle.step(crate::BattleInput::default())?;
                assert_eq!(battle.random_state(), seed);
                assert_eq!(battle.actors[0].reaction.idle_initialization, None);
            }
        }
        Ok(())
    }

    #[test]
    fn hurt_initializes_short_delay_but_guard_returns_directly_to_decisions() -> Result<()> {
        let mut hurt = cadence_battle(crate::Activity::Hurt)?;
        hurt.step(crate::BattleInput::default())?;
        assert_eq!(hurt.random_state(), 0x0012_d687);
        assert_eq!(hurt.actors[1].reaction.idle_initialization, Some(true));
        hurt.step(crate::BattleInput::default())?;
        assert_eq!(hurt.idle_timers[1], 15, "max((75 + 11) >> 3, 15)");
        assert_eq!(hurt.random_state(), 0xf8df_5002);
        hurt.step(crate::BattleInput::default())?;
        assert_eq!(hurt.idle_timers[1], 14);

        let mut guard = cadence_battle(crate::Activity::Guarding)?;
        guard.idle_timers[1] = 77;
        guard.step(crate::BattleInput::default())?;
        assert_eq!(guard.idle_timers[1], 0);
        assert_eq!(guard.actors[1].reaction.idle_initialization, None);
        guard.step(crate::BattleInput::default())?;
        assert_eq!(
            guard.idle_timers[1], -100,
            "guard is eligible on its first idle visit"
        );
        assert_eq!(
            guard.random_state(),
            0x0012_d687,
            "guard adds no idle-jitter draw"
        );
        Ok(())
    }
}
