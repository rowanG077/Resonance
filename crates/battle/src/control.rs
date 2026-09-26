//! Ordinary player requests and normal chaining (32738, 63D84, 636A8).
//! Selection tables and movement parameters are verified before activation;
//! the selected attack itself remains an authored battle sequence.
use crate::{
    ActionId, ActionPhase, ActionRequest, Activity, ActorId, Battle, Control, Cue, MotionBinding,
    PreparedBattle,
};
use anyhow::{Result, ensure};

/// Mapped buttons for one simulation update, after the user's button mapping.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonInput {
    pub held: bool,
    pub pressed: bool,
    pub released: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ControlInput {
    pub actor: ActorId,
    /// Original signed stick units. Positive X is screen right, positive Y up.
    pub stick: [i8; 2],
    /// Fresh strict +/-48 direction edge from the global input visit: -1/0/1.
    pub horizontal_pressed: i8,
    pub attack: ButtonInput,
    pub technique: ButtonInput,
    pub guard: ButtonInput,
    pub target: ButtonInput,
    /// One mapped navigation repeat while the target selector is open: -1/0/1.
    pub target_step: i8,
}

impl ControlInput {
    pub fn neutral(actor: ActorId) -> Self {
        Self {
            actor,
            stick: [0; 2],
            horizontal_pressed: 0,
            attack: Default::default(),
            technique: Default::default(),
            guard: Default::default(),
            target: Default::default(),
            target_step: 0,
        }
    }
}

/// Original normal descriptor and selector data, referring to this generation's
/// authored actions rather than original pointers or character-specific code.
#[derive(Debug, Clone, Copy)]
pub struct NormalControl {
    pub action: u16,
    pub allowed_directions: u8,
    pub fallback: Option<u8>,
    pub reach: f32,
    pub minimum_reach: f32,
    pub combo_at: [u16; 2],
    pub buffer_until: u8,
}

/// Learned player shortcut resolved to an authored action before activation.
#[derive(Debug, Clone, Copy)]
pub struct TechniqueControl {
    pub action: u16,
    pub minimum: f32,
    pub maximum: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct ControlMotions {
    pub idle: MotionBinding,
    pub walk: MotionBinding,
    pub run: MotionBinding,
    pub stop: MotionBinding,
    pub landing: MotionBinding,
}

#[derive(Debug, Clone)]
pub struct ControlDefinition {
    pub actor: ActorId,
    pub target: ActorId,
    pub normals: [NormalControl; 7],
    pub shortcuts: [Option<TechniqueControl>; 4],
    pub combo_limit: u8,
    pub walk_speed: f32,
    pub run_speed: f32,
    pub turn_ticks: u8,
    pub motions: Option<ControlMotions>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Locomotion {
    #[default]
    Idle,
    Walk,
    Run,
    Stop,
    Action,
}

#[derive(Debug, Clone)]
pub(crate) struct Controller {
    pub target: ActorId,
    pub combo: u8,
    pub(crate) confirmed_contact: bool,
    pub(crate) companion_retry: u8,
    pub(crate) chained_technique: Option<u16>,
    entry_facing: bool,
    normal: Option<(ActionId, u8)>,
    pub(crate) attack_target: ActorId,
    buffered: Option<u8>,
    pub(crate) jump_charge: crate::mobility::JumpCharge,
    pub(crate) mobility: Option<crate::mobility::Mobility>,
    pub(crate) hold_movement: bool,
    pub(crate) locomotion: Locomotion,
    previous_stick: i8,
    pub(crate) run_ticks: u8,
    target_ticks: u16,
    running_left: bool,
    landed: bool,
}

impl Controller {
    pub fn new(definition: &ControlDefinition) -> Self {
        Self {
            target: definition.target,
            combo: 0,
            confirmed_contact: false,
            companion_retry: 0,
            chained_technique: None,
            entry_facing: false,
            normal: None,
            attack_target: definition.target,
            buffered: None,
            jump_charge: Default::default(),
            mobility: None,
            hold_movement: false,
            locomotion: Locomotion::Idle,
            previous_stick: 0,
            run_ticks: 0,
            target_ticks: 0,
            running_left: false,
            landed: false,
        }
    }
}

impl PreparedBattle {
    pub fn with_controls(mut self, definitions: Vec<ControlDefinition>) -> Result<Self> {
        let mut controls = vec![None; self.actors.len()];
        for definition in definitions {
            let index = definition.actor.index();
            ensure!(
                index < self.actors.len() && definition.target.index() < self.actors.len(),
                "invalid prepared control actor"
            );
            ensure!(controls[index].is_none(), "duplicate battle control actor");
            let actor = &self.actors[index];
            ensure!(
                matches!(
                    actor.control,
                    Control::Manual | Control::SemiAuto | Control::Auto
                ),
                "normal controls require a party actor"
            );
            ensure!(
                actor.side != self.actors[definition.target.index()].side,
                "control target must be an opponent"
            );
            ensure!(
                (1..=7).contains(&definition.combo_limit)
                    && definition.turn_ticks != 0
                    && definition.walk_speed.is_finite()
                    && definition.walk_speed > 0.
                    && definition.run_speed.is_finite()
                    && definition.run_speed >= definition.walk_speed,
                "invalid prepared control movement"
            );
            for normal in definition.normals {
                ensure!(
                    normal.fallback.is_none_or(|v| v < 7)
                        && normal.allowed_directions & 0x80 == 0
                        && normal.reach.is_finite()
                        && normal.reach > 0.
                        && normal.minimum_reach.is_finite()
                        && normal.minimum_reach >= 0.
                        && normal.minimum_reach < normal.reach
                        && normal.combo_at.iter().all(|&v| v <= i16::MAX as u16),
                    "invalid prepared normal selector"
                );
                ensure!(
                    self.actions.iter().any(|a| a.id == normal.action
                        && a.phase == ActionPhase::Actor
                        && a.tp_cost == 0),
                    "normal selector needs a prepared actor action"
                );
            }
            for shortcut in definition.shortcuts.into_iter().flatten() {
                ensure!(
                    shortcut.minimum.is_finite()
                        && shortcut.maximum.is_finite()
                        && shortcut.minimum >= 0.
                        && shortcut.maximum > shortcut.minimum,
                    "invalid prepared technique range"
                );
                ensure!(
                    self.actions
                        .iter()
                        .any(|action| action.id == shortcut.action
                            && matches!(action.phase, ActionPhase::Actor | ActionPhase::Casting)),
                    "technique shortcut needs a prepared actor action"
                );
            }
            if let Some(motions) = definition.motions {
                let model = self.models[index]
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("control motions need an actor model"))?;
                for binding in [
                    motions.idle,
                    motions.walk,
                    motions.run,
                    motions.stop,
                    motions.landing,
                ] {
                    model.duration(binding)?;
                }
            }
            self.actors[index].movement.braking = 0.55;
            self.actors[index].movement.gravity = if self.actors[index].movement.flying {
                0.
            } else {
                -1.
            };
            controls[index] = Some(definition);
        }
        self.controls = controls;
        Ok(self)
    }
}

/// 268C4 uses strict directional thresholds, and gives vertical input priority.
pub(crate) fn normal_direction([x, y]: [i8; 2], height: f32) -> u8 {
    if height > 0.1 {
        return if y < -48 { 6 } else { 5 };
    }
    if y > 48 {
        1
    } else if y < -48 {
        2
    } else if !(-48..=48).contains(&x) {
        3
    } else {
        0
    }
}

impl Battle {
    /// Read held-target admission counts in actor order without consuming input.
    pub fn target_hold_counts(&self) -> impl Iterator<Item = Option<u16>> + '_ {
        self.controls
            .iter()
            .map(|control| control.as_ref().map(|control| control.target_ticks))
    }

    pub fn target(&self, actor: ActorId) -> Option<ActorId> {
        self.controls
            .get(actor.index())
            .and_then(Option::as_ref)
            .map(|c| c.target)
            .or_else(|| self.targets.get(actor.index()).copied())
    }

    pub(crate) fn validate_controls(&self, inputs: &[ControlInput]) -> Result<()> {
        for (i, input) in inputs.iter().enumerate() {
            ensure!(
                // Result construction retires the mutable combat controller;
                // the prepared input binding remains valid through that handoff.
                self.prepared
                    .controls
                    .get(input.actor.index())
                    .is_some_and(Option::is_some),
                "input requires a prepared player controller"
            );
            ensure!(
                !inputs[..i]
                    .iter()
                    .any(|previous| previous.actor == input.actor),
                "duplicate battle controller input"
            );
            ensure!(
                (-1..=1).contains(&input.target_step)
                    && (-1..=1).contains(&input.horizontal_pressed),
                "invalid battle direction input"
            );
        }
        Ok(())
    }

    pub(crate) fn control_actor(
        &mut self,
        actor: ActorId,
        input: ControlInput,
        action_callback_ready: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if self.phase() != crate::BattlePhase::Combat {
            return Ok(());
        }
        let index = actor.index();
        let Some(mut control) = self.controls[index].take() else {
            return Ok(());
        };
        let definition = self.prepared.controls[index].as_ref().unwrap().clone();
        control.hold_movement = false;
        if control.mobility.is_some_and(|mobility| {
            !mobility.matches(self.actors[index].activity)
                || mobility == crate::mobility::Mobility::Finished
        }) {
            control.mobility = None;
        }
        if !self.actors[index].available() {
            control.normal = None;
            control.buffered = None;
            control.chained_technique = None;
            control.mobility = None;
            self.controls[index] = Some(control);
            return Ok(());
        }
        if !self.target_available(actor, control.target)
            && let Some(target) = self.nearest_target(actor, None)
        {
            control.target = target;
            self.actors[index].hud.target_highlight = 120;
        }
        // 63D84 is called before the model-ready/local-hit-stop gate in 2C5B4.
        if action_callback_ready && let Some((id, selection)) = control.normal {
            if let Some(sequence) = self.sequences.get(&id) {
                let normal = definition.normals[usize::from(selection)];
                if input.attack.pressed
                    && !control.landed
                    && sequence.recovery.is_none()
                    && sequence.age <= u32::from(normal.buffer_until)
                    && control.combo < definition.combo_limit - 1
                    && selection < 5
                {
                    let direction = normal_direction(input.stick, self.actors[index].position[1]);
                    let next = if normal.allowed_directions & (1 << direction) != 0 {
                        Some(direction)
                    } else {
                        normal.fallback
                    };
                    if let Some(next) = next {
                        control.buffered = Some(next);
                        control.chained_technique = None;
                    }
                } else if !input.attack.pressed
                    && input.technique.pressed
                    && !control.landed
                    && sequence.recovery.is_none()
                    && sequence.age <= u32::from(normal.buffer_until)
                    && let Some(shortcut) =
                        definition.shortcuts[usize::from(normal_direction(input.stick, 0.))]
                {
                    // 63D84 buffers before hit-stop/model gates. 636A8 checks
                    // TP/technique admission only when the combo window opens.
                    control.chained_technique = Some(shortcut.action);
                    control.buffered = None;
                }
            } else {
                control.normal = None;
                control.buffered = None;
                control.chained_technique = None;
                control.combo = 0;
            }
        }
        if self.actors[index].control == Control::Auto || self.idle_initializing(actor) {
            self.controls[index] = Some(control);
            return Ok(());
        }
        if control.normal.is_none() {
            self.player_control(&definition, &mut control, input, cues)?;
        }
        // 346C8 follows the player's command selection. Tap selection occurs on
        // release; a held button opens the selector on its tenth actor visit.
        if input.target.pressed {
            control.target_ticks = 0;
        }
        if input.target.held {
            if control.target_ticks > 8 {
                self.target_selector = Some(actor);
            }
            control.target_ticks = control.target_ticks.saturating_add(1);
        }
        if input.target.released {
            // 36B1C refreshes even when the request retains the same target.
            self.actors[index].hud.target_highlight = 120;
            if let Some(target) = self.nearest_target(actor, Some(control.target)) {
                control.target = target;
            }
            control.target_ticks = 0;
        }
        if let Some(camera) = &mut self.camera
            && camera.definition.leader == actor
        {
            camera.definition.target = control.target;
        }
        control.previous_stick = input.stick[0];
        self.controls[index] = Some(control);
        Ok(())
    }

    /// 30B4C faces before normal3E37C or martial3A1E8 dispatch, including
    /// their initializers. This precedes model/local-hit-stop gates; a failed
    /// turn skips the complete callback and its movement.
    pub(crate) fn action_callback_ready(
        &mut self,
        actor: ActorId,
    ) -> (bool, Option<crate::weapon_flight::ReturnSteering>) {
        let index = actor.index();
        if !self.actors[index].available()
            || !matches!(self.actors[index].activity, Activity::Action { .. })
        {
            return (true, None);
        }
        let Some(sequence) = self.sequences.values().find(|sequence| {
            sequence.actor == actor
                && sequence.definition.phase == ActionPhase::Actor
                && sequence.recovery.is_none()
        }) else {
            return (true, None);
        };
        let action = sequence.definition.id;
        let turn_ticks = self.prepared.controls[index]
            .as_ref()
            .filter(|definition| {
                definition
                    .normals
                    .iter()
                    .any(|normal| normal.action == action)
                    || definition
                        .shortcuts
                        .iter()
                        .flatten()
                        .any(|technique| technique.action == action)
                    || self.prepared.companions[index]
                        .as_ref()
                        .is_some_and(|companion| {
                            companion
                                .techniques
                                .iter()
                                .any(|technique| technique.action == action)
                        })
            })
            .map(|definition| definition.turn_ticks)
            .or_else(|| {
                self.prepared.enemy_decisions[index]
                    .as_ref()
                    .filter(|definition| {
                        definition
                            .choices
                            .iter()
                            .any(|choice| choice.action == action)
                    })
                    .map(|definition| definition.turn_ticks)
            });
        let Some(turn_ticks) = turn_ticks else {
            return (true, None);
        };
        let actor = &mut self.actors[index];
        if actor.movement.turning_disabled
            || actor.side == crate::Side::Party && actor.position[1] > 0.1
        {
            return (true, None);
        }
        let direction = actor.movement.direction;
        face_cached_with_desired(actor, direction, 180. / f32::from(turn_ticks))
    }

    pub(crate) fn normal_visit(&self, index: usize) -> (bool, bool) {
        self.controls[index]
            .as_ref()
            .map_or((false, false), |control| {
                (
                    control.normal.is_some(),
                    control.normal.is_some() && control.entry_facing,
                )
            })
    }

    pub(crate) fn face_normal_entry(&mut self, id: ActionId, actor: ActorId) -> Result<()> {
        let Some(control) = self.controls[actor.index()].as_ref() else {
            return Ok(());
        };
        if !control.entry_facing || control.normal.is_none_or(|(active, _)| active != id) {
            return Ok(());
        }
        let mut control = self.controls[actor.index()].take().unwrap();
        let definition = self.prepared.controls[actor.index()].as_ref().unwrap();
        // 3DF34/1B48C preserve the original attack target across a chain and
        // only turn a grounded actor while that target remains selected.
        let index = actor.index();
        if !self.actors[index].movement.turning_disabled
            && self.actors[index].position[1] <= 0.1
            && control.target == control.attack_target
        {
            let direction = self.actors[index].movement.target_direction;
            if self.actors[index].control != Control::Manual
                || crate::distance::dot(self.actors[index].facing_direction, direction) >= 0.
            {
                self.actors[index].movement.direction = direction;
                self.actors[index].facing_direction = direction;
                face(
                    &mut self.actors[index],
                    direction,
                    180. / f32::from(definition.turn_ticks),
                );
            }
        }
        control.entry_facing = false;
        control.confirmed_contact = false;
        control.hold_movement = true;
        let selection = control.normal.unwrap().1;
        self.controls[actor.index()] = Some(control);
        self.record_normal_title(actor, selection);
        Ok(())
    }

    pub(crate) fn start_control_normal(
        &mut self,
        definition: &ControlDefinition,
        control: &mut Controller,
        selection: u8,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let id = ActionId(self.next_action);
        self.start(
            ActionRequest {
                actor: definition.actor,
                action: definition.normals[usize::from(selection)].action,
                target: control.target,
            },
            cues,
        )?;
        if self.sequences.contains_key(&id) {
            control.normal = Some((id, selection));
            control.confirmed_contact = false;
            control.entry_facing = true;
            control.locomotion = Locomotion::Action;
            control.run_ticks = 0;
            control.landed = false;
            if selection >= 5 {
                self.actors[definition.actor.index()]
                    .movement
                    .airborne_action = true;
            }
        }
        Ok(())
    }

    /// 636A8 follows this visit's commands, hit submission and animation rows.
    /// Replacing a sequence drops its owned tasks; the next initializer runs on
    /// the following actor visit, and already-submitted contacts remain valid.
    pub(crate) fn chain_normal(
        &mut self,
        id: ActionId,
        sequence: &crate::script::Sequence,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let index = sequence.actor.index();
        if self.actors[index].control == Control::Enemy {
            if let Some(choice) =
                self.prepared.enemy_decisions[index]
                    .as_ref()
                    .and_then(|definition| {
                        definition
                            .choices
                            .iter()
                            .find(|choice| choice.action == sequence.definition.id)
                    })
                && sequence.age == u32::from(choice.combo_at)
            {
                // 636A8 rolls before comparing the descriptor's chance.
                // Prepared rows have nonpositive chances but still consume it.
                // This boundary follows ready commands/hits and precedes the
                // attached effect and outer age increment; blends/hit-stop hold it.
                self.random.next();
            }
            return Ok(false);
        }
        if self.actors[index].control == Control::Auto {
            self.visit_companion_combo(id, sequence, cues)?;
        }
        let Some(control) = &self.controls[index] else {
            return Ok(false);
        };
        let Some((active, selection)) = control.normal else {
            return Ok(false);
        };
        if active != id {
            return Ok(false);
        }
        let normal =
            self.prepared.controls[index].as_ref().unwrap().normals[usize::from(selection)];
        let [first, second] = normal.combo_at.map(u32::from);
        if sequence.age < first || sequence.age > first && second != 0 && sequence.age < second {
            return Ok(false);
        }
        if let Some(action) = control.chained_technique {
            let mut control = self.controls[index].take().unwrap();
            control.chained_technique = None;
            let cost = self
                .prepared
                .actions
                .iter()
                .find(|a| a.id == action)
                .ok_or_else(|| anyhow::anyhow!("unprepared chained technique"))?
                .tp_cost;
            if self.actors[index].tp < cost || self.actors[index].position[1] > 0.1 {
                if self.actors[index].tp < cost {
                    cues.push(Cue::Rejected {
                        actor: sequence.actor,
                        reason: crate::Rejection::InsufficientTp,
                    });
                }
                self.controls[index] = Some(control);
                return Ok(false);
            }
            control.normal = None;
            control.buffered = None;
            control.confirmed_contact = false;
            cues.push(Cue::Completed { action: id });
            self.record_technique_title(sequence.actor);
            self.start(
                ActionRequest {
                    actor: sequence.actor,
                    target: control.target,
                    action,
                },
                cues,
            )?;
            self.controls[index] = Some(control);
            return Ok(true);
        }
        let Some(next) = control.buffered else {
            return Ok(false);
        };
        let mut control = self.controls[index].take().unwrap();
        control.buffered = None;
        control.combo += 1;
        self.actors[index].attack_power =
            self.actors[index].attack_power.saturating_sub(15).max(10);
        let definition = self.prepared.controls[index].as_ref().unwrap().clone();
        cues.push(Cue::Completed { action: id });
        self.start_control_normal(&definition, &mut control, next, cues)?;
        self.controls[index] = Some(control);
        Ok(true)
    }

    pub(crate) fn confirm_actor_contact(&mut self, owner: ActorId) {
        if let Some(control) = &mut self.controls[owner.index()] {
            control.confirmed_contact = true;
        }
    }

    fn visit_companion_combo(
        &mut self,
        id: ActionId,
        sequence: &crate::script::Sequence,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = sequence.actor.index();
        if self.prepared.companions[index].is_none() {
            return Ok(());
        }
        let Some(control) = &self.controls[index] else {
            return Ok(());
        };
        let Some((active, selection)) = control.normal else {
            return Ok(());
        };
        let normal =
            self.prepared.controls[index].as_ref().unwrap().normals[usize::from(selection)];
        if active != id
            || !control.confirmed_contact
            || control.landed
            || !(sequence.age == u32::from(normal.combo_at[0])
                || normal.combo_at[1] != 0 && sequence.age == u32::from(normal.combo_at[1]))
        {
            return Ok(());
        }
        let decisions: Vec<_> = self
            .sequences
            .iter()
            .filter(|(_, s)| {
                s.actor == sequence.actor && s.definition.phase == ActionPhase::Decision
            })
            .map(|(&id, _)| id)
            .collect();
        for decision in decisions {
            let mut policy = self.sequences.remove(&decision).unwrap();
            crate::script::step_combo_visit(self, decision, &mut policy, cues)?;
            self.sequences.insert(decision, policy);
        }
        Ok(())
    }

    pub(crate) fn companion_combo(&self, actor: ActorId) -> Result<Vec<i32>> {
        let control = self.controls[actor.index()]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("companion has no normal controller"))?;
        let (_, selection) = control
            .normal
            .ok_or_else(|| anyhow::anyhow!("companion has no active normal"))?;
        Ok(vec![
            selection.into(),
            control.combo.into(),
            self.prepared.controls[actor.index()]
                .as_ref()
                .unwrap()
                .combo_limit
                .into(),
        ])
    }

    pub(crate) fn queue_companion_chain(&mut self, actor: ActorId, action: u16) -> Result<bool> {
        let index = actor.index();
        ensure!(
            self.prepared.companions[index].is_some(),
            "chain needs a companion policy"
        );
        let definition = self.prepared.controls[index].as_ref().unwrap();
        let control = self.controls[index].as_mut().unwrap();
        if let Some(selection) = definition.normals.iter().position(|n| n.action == action) {
            control.buffered = Some(selection as u8);
            return Ok(true);
        }
        let technique = self.prepared.companions[index]
            .as_ref()
            .unwrap()
            .techniques
            .iter()
            .find(|t| t.action == action)
            .ok_or_else(|| anyhow::anyhow!("unprepared companion chain"))?;
        if !technique.enabled
            || self.actors[index].tp < technique.cost
            || self.actors[index].position[1] > 0.1
        {
            return Ok(false);
        }
        control.chained_technique = Some(action);
        Ok(true)
    }

    pub(crate) fn move_control_actor(
        &mut self,
        index: usize,
        definition: &ControlDefinition,
        control: &mut Controller,
        stick: i8,
        walk_only: bool,
    ) -> Result<()> {
        let target = self.actors[control.target.index()].position;
        let direction = crate::distance::planar_direction(
            target,
            self.actors[index].position,
            self.actors[index].movement.direction,
        );
        let current = self.actors[index].position;
        let projected = |point: [f32; 3]| {
            self.camera.as_ref().map_or(point[0], |camera| {
                project_screen_x(camera.pose, [point[0], 0., point[2]])
            })
        };
        let screen_delta = projected(target) - projected(current);
        let mut movement = self.actors[index].movement.direction;
        let locomotion;
        let running = !walk_only && control.locomotion == Locomotion::Run;
        let continuing = if control.running_left {
            stick <= -30
        } else {
            stick >= 30
        };
        if control.locomotion == Locomotion::Stop {
            // 32738 excludes stop-state movement without EX41; 2F788 keeps
            // braking until both the speed and the stop motion have finished.
            locomotion = Locomotion::Stop;
        } else if running && !continuing {
            // Releasing or reversing a run first enters walk/stop; reversal
            // never changes the movement vector on this visit.
            if control.run_ticks > 20 {
                locomotion = Locomotion::Stop;
            } else {
                locomotion = Locomotion::Walk;
                self.actors[index].movement.forward =
                    definition.walk_speed * self.actors[index].body.scale;
            }
            control.run_ticks = 0;
        } else if stick >= 30 || stick <= -30 {
            // 32738 measures pixels, using 4 on admission and 8 while running.
            if screen_delta.abs() > if running { 8. } else { 4. } {
                let sign =
                    if screen_delta < 0. { -1. } else { 1. } * if stick < 0 { -1. } else { 1. };
                movement = direction.map(|v| v * sign);
            }
            let difference = (i16::from(control.previous_stick) - i16::from(stick)).abs();
            let running = !walk_only && (running || difference >= 15);
            locomotion = if running {
                Locomotion::Run
            } else {
                Locomotion::Walk
            };
            self.actors[index].movement.direction = movement;
            // 32738 copies18F0 into18E4. Manual/Semi31290's enabled1 turn
            // changes heading only; it does not rebuild this copied vector.
            self.actors[index].facing_direction = movement;
            if running {
                if control.run_ticks == 0 {
                    self.actors[index].movement.forward = definition.walk_speed;
                }
                self.actors[index].movement.forward =
                    (self.actors[index].movement.forward + 0.5).min(definition.run_speed);
                control.run_ticks += 1;
                if control.run_ticks > 15 {
                    // 31290 assigns 200 to a seven-bit field: the stored
                    // value is 72, and remains 72 on subsequent run visits.
                    control.run_ticks = 200 & 0x7f;
                }
                control.running_left = stick < 0;
            } else {
                self.actors[index].movement.forward =
                    definition.walk_speed * self.actors[index].body.scale;
                control.run_ticks = 0;
            }
            face(
                &mut self.actors[index],
                movement,
                180. / f32::from(definition.turn_ticks),
            );
        } else {
            locomotion = Locomotion::Idle;
            self.actors[index].movement.forward = 0.;
            control.run_ticks = 0;
        }
        self.control_motion(
            index,
            definition,
            control,
            locomotion,
            if locomotion == Locomotion::Walk { 3 } else { 4 },
        )
    }

    pub(crate) fn control_motion(
        &mut self,
        index: usize,
        definition: &ControlDefinition,
        control: &mut Controller,
        locomotion: Locomotion,
        blend: u8,
    ) -> Result<()> {
        if control.locomotion != locomotion {
            if let Some(motions) = definition.motions {
                if locomotion == Locomotion::Idle {
                    self.play_idle_pose(ActorId(index as u8), 8)?;
                } else {
                    let motion = match locomotion {
                        Locomotion::Idle => motions.idle,
                        Locomotion::Walk => motions.walk,
                        Locomotion::Run => motions.run,
                        Locomotion::Stop => motions.stop,
                        Locomotion::Action => unreachable!(),
                    };
                    self.models[index].as_mut().unwrap().play(
                        motion,
                        0.,
                        0.5,
                        locomotion != Locomotion::Stop,
                        if matches!(locomotion, Locomotion::Idle | Locomotion::Stop) {
                            8
                        } else {
                            blend
                        },
                    )?;
                }
            }
            control.locomotion = locomotion;
        }
        Ok(())
    }

    fn target_available(&self, actor: ActorId, target: ActorId) -> bool {
        let target = &self.actors[target.index()];
        target.side != self.actors[actor.index()].side && target.available()
    }

    pub(crate) fn brake_control(&mut self, index: usize) -> Result<()> {
        if let Some(control) = &mut self.controls[index]
            && let Some((id, _)) = control.normal
            && let Some(sequence) = self.sequences.get_mut(&id)
            && sequence.recovery.is_none()
        {
            let actor = &mut self.actors[index];
            if actor.movement.vertical > 0. && actor.position[1] > 0.1 {
                actor.movement.airborne_action = true;
            }
            if actor.movement.airborne_action && actor.position[1] <= 0.1 {
                actor.movement.airborne_action = false;
                control.landed = true;
                control.buffered = None;
                // 3DA00's post-integration landing branch leaves the animation
                // stream clock intact, while remaining command rows become due.
                sequence.age = u32::from(sequence.definition.duration.saturating_sub(16));
                sequence.command_age = 0x400;
                sequence.hit_age = 0x400;
                if let Activity::Action { clock, .. } = &mut actor.activity {
                    *clock = sequence.age as i16;
                }
                if let Some(motions) = self.prepared.controls[index].as_ref().unwrap().motions {
                    self.models[index].as_mut().unwrap().play(
                        motions.landing,
                        0.,
                        0.5,
                        false,
                        6,
                    )?;
                }
            }
        }
        if self.controls[index]
            .as_ref()
            .is_some_and(|control| control.locomotion == Locomotion::Stop)
        {
            let unfinished = self.models[index]
                .as_ref()
                .is_some_and(|model| !model.finished());
            let actor = &mut self.actors[index];
            if actor
                .movement
                .brake(actor.position[1], actor.activity, false, unfinished)
            {
                actor.guard.reset_auto_chance(75, &mut self.random);
                actor.reset_hud_targets();
                let definition = self.prepared.controls[index].as_ref().unwrap().clone();
                let mut control = self.controls[index].take().unwrap();
                self.control_motion(index, &definition, &mut control, Locomotion::Idle, 8)?;
                self.controls[index] = Some(control);
            }
        }
        Ok(())
    }

    pub(crate) fn control_holds_movement(&self, index: usize) -> bool {
        self.approach_holds_movement(index)
            || self.controls[index]
                .as_ref()
                .is_some_and(|control| control.hold_movement)
    }

    pub(crate) fn start_automatic_normal(
        &mut self,
        actor: ActorId,
        action: u16,
        target: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let index = actor.index();
        let Some(definition) = self.prepared.controls[index].clone() else {
            return Ok(false);
        };
        let Some(selection) = definition
            .normals
            .iter()
            .position(|normal| normal.action == action)
        else {
            return Ok(false);
        };
        let mut control = self.controls[index].take().unwrap();
        control.target = target;
        control.attack_target = target;
        control.combo = 0;
        self.start_control_normal(&definition, &mut control, selection as u8, cues)?;
        self.controls[index] = Some(control);
        Ok(true)
    }

    fn nearest_target(&self, actor: ActorId, avoid: Option<ActorId>) -> Option<ActorId> {
        let mut best = None;
        let mut distance = f32::MAX;
        let count = self
            .actors
            .iter()
            .enumerate()
            .filter(|(i, _)| self.target_available(actor, ActorId(*i as u8)))
            .count();
        for (index, target) in self.actors.iter().enumerate() {
            let id = ActorId(index as u8);
            if !self.target_available(actor, id) || count > 1 && Some(id) == avoid {
                continue;
            }
            let origin = self.actors[actor.index()].position;
            let value = crate::distance::length([
                target.position[0] - origin[0],
                0.,
                target.position[2] - origin[2],
            ]);
            if value < distance {
                distance = value;
                best = Some(id);
            }
        }
        best
    }

    pub(crate) fn control_target_selector(&mut self, inputs: &[ControlInput]) -> Result<()> {
        let actor = self.target_selector.unwrap();
        let input = inputs
            .iter()
            .find(|input| input.actor == actor)
            .copied()
            .unwrap_or_else(|| ControlInput::neutral(actor));
        let previous = self.controls[actor.index()].as_ref().unwrap().target;
        if input.target_step != 0 {
            self.actors[actor.index()].hud.target_highlight = 120;
            let projected = |index: ActorId| {
                let actor = &self.actors[index.index()];
                self.camera
                    .as_ref()
                    .map_or(actor.body.target_center[0], |camera| {
                        project_screen_x(camera.pose, actor.body.target_center)
                    })
            };
            let current = projected(previous);
            let mut near = previous;
            let mut far = previous;
            let mut near_distance = f32::MAX;
            let mut far_distance = 0.;
            for index in 0..self.actors.len() {
                let target = ActorId(index as u8);
                if target == previous || !self.target_available(actor, target) {
                    continue;
                }
                let delta = (projected(target) - current) * f32::from(input.target_step);
                if delta > 0.
                    && (delta < near_distance || input.target_step > 0 && delta == near_distance)
                {
                    near = target;
                    near_distance = delta;
                }
                if delta < 0. && -delta > far_distance {
                    far = target;
                    far_distance = -delta;
                }
            }
            self.controls[actor.index()].as_mut().unwrap().target =
                if near == previous { far } else { near };
        }
        let target = self.controls[actor.index()].as_ref().unwrap().target;
        if let Some(camera) = &mut self.camera {
            if camera.definition.leader == actor {
                camera.definition.target = target;
            }
            camera.step_target_selector(&self.actors)?;
        }
        // 3648 selects first, then FE9C/53300/1C40 draw and follow the new
        // target. 5200C bit4 composes retained models before its appearance
        // override; 51914/155CC then copy that tint to carried weapons.
        let ambient = self.prepared.ambient_color;
        for (index, (model, actor)) in self.models.iter_mut().zip(&mut self.actors).enumerate() {
            if let Some(model) = model {
                model.step(actor, false, crate::model::PlacementUpdate::Held)?;
                let mut tint = model.shown.tint;
                self.contact_feedback[index].appearance(&mut tint, Some(ambient));
                // Original107D low lifecycle states0..2 skip selector tint.
                if actor.available() {
                    let rgb = if index == target.index() {
                        self.contact_feedback[index].clear_flash();
                        ambient
                    } else {
                        ambient.map(|value| value >> 1)
                    };
                    tint[..3].copy_from_slice(&rgb);
                }
                model.sample_tint(tint);
            }
        }
        self.advance_hud(false);
        // Release clears bit4 only after this held visit's model and HUD work.
        if !input.target.held {
            self.target_selector = None;
            self.controls[actor.index()].as_mut().unwrap().target_ticks = 0;
        }
        Ok(())
    }
}

/// Ordinary 53484/53300 camera projection followed by SDK GXProject. Keep
/// look-at translation and vertex multiplication separate, with their original
/// scalar operation order; 32738's four/eight-pixel boundaries depend on depth.
pub fn project_screen_x(camera: crate::CameraPose, point: [f32; 3]) -> f32 {
    project_screen_point(camera, point)[0]
}

/// Source viewport coordinates, before presentation's 448-to-480 layout mapping.
pub fn project_screen_point(camera: crate::CameraPose, point: [f32; 3]) -> [f32; 2] {
    let [eye_x, eye_y, eye_z] = project_view(camera, point);
    let angle = 0.017_453_292_f32 * (0.5_f32 * 13.33_f32);
    let cotangent = 1. / angle.tan();
    let projection = cotangent / (4_f32 / 3.);
    let clip_x = eye_x * projection + eye_z * 0.;
    let clip_y = eye_y * cotangent + eye_z * 0.;
    [
        320. + ((1. / -eye_z) * (clip_x * 640. / 2.)),
        224. + ((1. / -eye_z) * (-clip_y * 448. / 2.)),
    ]
}

/// GXProject depth used by 495E4's actor ordering, before raster depth testing.
pub fn project_depth(camera: crate::CameraPose, point: [f32; 3]) -> f32 {
    let [_, _, eye_z] = project_view(camera, point);
    let range = 1_f32 / (21288_f32 - 50_f32);
    let clip_z = range * -(21288_f32 * 50_f32) + eye_z * (-50_f32 * range);
    1. + (1. / -eye_z) * clip_z
}

fn project_view(camera: crate::CameraPose, point: [f32; 3]) -> [f32; 3] {
    let forward =
        crate::distance::normalize(std::array::from_fn(|i| camera.eye[i] - camera.focus[i]));
    // PSMTXLookAt: up=(0,1,0), cross(up,forward), then SDK normalization.
    let right = crate::distance::normalize([forward[2], 0., -forward[0]]);
    let up = [
        forward[1] * right[2],
        forward[2] * right[0] - forward[0] * right[2],
        -forward[1] * right[0],
    ];
    let scalar_dot = |a: [f32; 3], b: [f32; 3]| (a[2] * b[2]) + ((a[0] * b[0]) + (a[1] * b[1]));
    let eye_x = -scalar_dot(camera.eye, right) + scalar_dot(point, right);
    let eye_z = -scalar_dot(camera.eye, forward) + scalar_dot(point, forward);
    let eye_y = -scalar_dot(camera.eye, up) + scalar_dot(point, up);
    [eye_x, eye_y, eye_z]
}

/// Source4DD2C: fmuls with the original degree factor, double sin/cos, then
/// separate frsp results. Host double trig still needs original bit comparison.
pub fn direction_from_heading(heading: f32) -> [f32; 3] {
    let radians = heading * f32::from_bits(0x3c8e_fa33);
    let radians = f64::from(radians);
    [radians.sin() as f32, 0., radians.cos() as f32]
}

/// 24A8C with flags17: force an immediate heading, without replacing18E4.
pub(crate) fn snap_heading(actor: &mut crate::Actor, direction: [f32; 3]) {
    if actor.heading >= 180. {
        actor.heading -= 360.;
    }
    if actor.heading <= -180. {
        actor.heading += 360.;
    }
    if crate::distance::length(direction) >= 0.01 {
        actor.heading = (f64::from(direction[0]).atan2(f64::from(direction[2])) as f32) * 57.295_79;
    }
}

/// Active24D24 branch. The enabled1 Manual/Semi queued-turn path instead calls
/// the heading-only helper below and deliberately leaves18E4 unchanged.
pub(crate) fn face_cached(actor: &mut crate::Actor, direction: [f32; 3], step: f32) -> bool {
    face_cached_with_desired(actor, direction, step).0
}

fn face_cached_with_desired(
    actor: &mut crate::Actor,
    direction: [f32; 3],
    step: f32,
) -> (bool, Option<crate::weapon_flight::ReturnSteering>) {
    if actor.movement.turning_disabled || !actor.movement.hover_ready() {
        return (false, None);
    }
    // 24D24 wraps and rebuilds even when its input is too short to turn.
    // That branch does not establish the desired-angle operand.
    if crate::distance::length(direction) < 0.01 {
        if actor.heading >= 180. {
            actor.heading -= 360.;
        }
        if actor.heading <= -180. {
            actor.heading += 360.;
        }
        actor.facing_direction = direction_from_heading(actor.heading);
        return (false, None);
    }
    let result = face_with_desired(actor, direction, step);
    actor.facing_direction = direction_from_heading(actor.heading);
    result
}

pub(crate) fn face(actor: &mut crate::Actor, direction: [f32; 3], step: f32) -> bool {
    face_with_desired(actor, direction, step).0
}

fn face_with_desired(
    actor: &mut crate::Actor,
    direction: [f32; 3],
    step: f32,
) -> (bool, Option<crate::weapon_flight::ReturnSteering>) {
    if crate::distance::length(direction) < 0.01 {
        return (false, None);
    }
    let mut wanted = (f64::from(direction[0]).atan2(f64::from(direction[2])) as f32) * 57.295_79;
    if actor.heading >= 180. {
        actor.heading -= 360.;
    }
    if actor.heading <= -180. {
        actor.heading += 360.;
    }
    let mut wrapped = false;
    if actor.heading - wanted > 180. {
        wanted += 360.;
        wrapped = true;
    }
    if actor.heading - wanted < -180. {
        wanted -= 360.;
        wrapped = true;
    }
    // Capture the adjusted desired angle before a partial turn changes the
    // current heading. The latter is not the callback's retained operand.
    let desired = Some(crate::weapon_flight::ReturnSteering::facing(
        wanted, wrapped,
    ));
    if step == 0. {
        actor.heading = wanted;
        return (true, desired);
    }
    // Preserve the source subtract/add comparisons, not a rearranged delta.
    let ready = if actor.heading < wanted - step {
        actor.heading += step;
        false
    } else if actor.heading > wanted + step {
        actor.heading -= step;
        false
    } else {
        actor.heading = wanted;
        true
    };
    (ready, desired)
}

/// 1BAF4 measures the nearest pair of 0x40 body points, including radii.
pub(crate) fn body_gap(actor: &crate::Actor, target: &crate::Actor) -> f32 {
    let mut best = 10000.;
    for a in &actor.body.approach_points {
        for b in &target.body.approach_points {
            let distance =
                crate::distance::length([a.center[0] - b.center[0], 0., a.center[2] - b.center[2]])
                    - (a.radius * actor.body.scale + b.radius * target.body.scale);
            let distance = if distance < 0. { 0.01 } else { distance };
            if distance < best {
                best = distance;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests;
