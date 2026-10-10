//! Player input, action chaining and target selection.
use crate::action_selection::{BufferedAction, InputIntent};
use crate::state::ActorTask;
use crate::{
    ActionId, ActionRequest, Activity, ActorId, Battle, Control, Cue, MotionBinding, PreparedBattle,
};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

pub(crate) const MOVEMENT_STICK_THRESHOLD: u8 = 30;
pub(crate) const RUN_FLICK_THRESHOLD: i16 = 15;
pub(crate) const RUN_STOP_AFTER_TICKS: u8 = 20;
pub(crate) const WALK_FACING_MARGIN: f32 = 4.;
pub(crate) const RUN_FACING_MARGIN: f32 = 8.;
const MOVEMENT_PLAYBACK_RATE: f32 = 0.5;
const WALK_BLEND_TICKS: u8 = 3;
const RUN_BLEND_TICKS: u8 = 4;
const STOP_BLEND_TICKS: u8 = 8;
const AERIAL_COMBO_LIFT: f32 = 8.;

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
    pub stick: [i8; 2],
    /// Fresh horizontal direction edge: left -1, neutral 0, right 1.
    pub horizontal_pressed: i8,
    /// Fresh vertical direction edge: down -1, neutral 0, up 1.
    pub vertical_pressed: i8,
    pub attack: ButtonInput,
    pub technique: ButtonInput,
    /// Mapped logical action 4 (Delay Spell), sampled by the caster's
    /// controller slot even when that caster is Auto-controlled.
    pub delay_spell: ButtonInput,
    pub taunt: ButtonInput,
    pub guard: ButtonInput,
    pub target: ButtonInput,
    pub assist: [ButtonInput; 2],
    /// One mapped navigation repeat while the target selector is open: -1/0/1.
    pub target_step: i8,
}

impl ControlInput {
    pub fn neutral(actor: ActorId) -> Self {
        Self {
            actor,
            stick: [0; 2],
            horizontal_pressed: 0,
            vertical_pressed: 0,
            attack: Default::default(),
            technique: Default::default(),
            delay_spell: Default::default(),
            taunt: Default::default(),
            guard: Default::default(),
            target: Default::default(),
            assist: Default::default(),
            target_step: 0,
        }
    }
}

/// Native normal attacks, ordered only for their prepared action bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NormalAttack {
    Neutral,
    Rising,
    Thrust,
    Low,
    Finisher,
    AerialSlash,
    AerialThrust,
}

impl NormalAttack {
    pub const ALL: [Self; 7] = [
        Self::Neutral,
        Self::Rising,
        Self::Thrust,
        Self::Low,
        Self::Finisher,
        Self::AerialSlash,
        Self::AerialThrust,
    ];

    pub fn airborne(self) -> bool {
        matches!(self, Self::AerialSlash | Self::AerialThrust)
    }

    pub(crate) fn from_input([x, y]: [i8; 2], airborne: bool) -> Self {
        const DIRECTION_THRESHOLD: i8 = 48;
        if airborne {
            if y < -DIRECTION_THRESHOLD {
                Self::AerialThrust
            } else {
                Self::AerialSlash
            }
        } else if y > DIRECTION_THRESHOLD {
            Self::Rising
        } else if y < -DIRECTION_THRESHOLD {
            Self::Thrust
        } else if !(-DIRECTION_THRESHOLD..=DIRECTION_THRESHOLD).contains(&x) {
            Self::Low
        } else {
            Self::Neutral
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NormalControl {
    pub action: crate::ActionKey,
    pub reach: f32,
    pub minimum_reach: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ComboWindow {
    pub opens_at: u32,
    pub buffer_until: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct ControlMotions {
    pub walk: MotionBinding,
    pub run: MotionBinding,
    pub stop: MotionBinding,
    pub landing: MotionBinding,
}

#[derive(Debug, Clone)]
pub struct ControlDefinition {
    pub normals: [NormalControl; 7],
    /// Initial values only. The existing live controller owns later page edits.
    pub shortcuts: [u16; 4],
    pub walk_speed: f32,
    pub run_speed: f32,
    pub turn_ticks: u8,
    pub motions: Option<ControlMotions>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Locomotion {
    #[default]
    Idle,
    Walk,
    Run,
    Stop,
    Action,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingTechnique {
    pub action: crate::ActionKey,
    pub target: Option<ActorId>,
    pub issuer: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum NormalState {
    Attack(NormalAttack),
    Landing,
}

impl NormalState {
    pub(crate) fn airborne(self) -> bool {
        matches!(self, Self::Attack(attack) if attack.airborne())
    }

    pub(crate) fn continuations(
        self,
        jump_combo: bool,
    ) -> (&'static [NormalAttack], Option<NormalAttack>) {
        use NormalAttack::*;
        match self {
            Self::Attack(Neutral) => (&[Rising, Thrust, Low], Some(Finisher)),
            Self::Attack(Thrust | Low) => (&[Neutral, Rising, Thrust, Low], None),
            Self::Attack(Rising) => (&[AerialSlash, AerialThrust], Some(AerialSlash)),
            Self::Attack(AerialSlash | AerialThrust) => (&[AerialSlash, AerialThrust], None),
            Self::Landing if jump_combo => (&[Neutral, Rising, Thrust, Low], None),
            Self::Attack(Finisher) | Self::Landing => (&[], None),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Controller {
    combo_input: ControlInput,
    /// Technique selected from the Tech page, independent of buffered combo input.
    pub(crate) queued_technique: Option<PendingTechnique>,
    pub(crate) shortcuts: [u16; 4],
    /// Live Tech-page policy state. Prepared actions remain immutable; these
    /// flags only affect future autonomous admissions.
    pub(crate) disabled_techniques: std::collections::BTreeSet<crate::ActionKey>,
    pub(crate) assist_shortcuts: crate::AssistShortcuts,
    pub(crate) attack_target: ActorId,
    pub(crate) jump_charge: crate::mobility::JumpCharge,
    last_horizontal: i8,
    pub(crate) horizontal_delta: i16,
    pub(crate) run_ticks: u8,
    target_ticks: u16,
    pub(crate) running_left: bool,
}

impl Controller {
    pub(crate) fn cancel_action(&mut self) {
        self.run_ticks = 0;
    }

    pub fn from_setup(
        actor: ActorId,
        setup: &mut crate::ActorSetup,
        target: ActorId,
    ) -> Option<Self> {
        let definition = setup.control.as_deref()?;
        Some(Self {
            combo_input: ControlInput::neutral(actor),
            queued_technique: None,
            shortcuts: definition.shortcuts,
            disabled_techniques: std::mem::take(&mut setup.disabled_techniques),
            assist_shortcuts: setup.assist_shortcuts,
            attack_target: target,
            jump_charge: Default::default(),
            last_horizontal: 0,
            horizontal_delta: 0,
            run_ticks: 0,
            target_ticks: 0,
            running_left: false,
        })
    }
}

impl PreparedBattle {
    pub(crate) fn validate_actor_controls(&self) -> Result<()> {
        for (index, setup) in self.resources.actor_setup.iter().enumerate() {
            ensure!(
                setup.disabled_techniques.is_empty() || setup.control.is_some(),
                "disabled techniques require a controller"
            );
            let Some(definition) = &setup.control else {
                continue;
            };
            ensure!(
                index < self.actors.len() && self.targets[index].index() < self.actors.len(),
                "invalid prepared control actor"
            );

            let actor = &self.actors[index];
            ensure!(
                actor.side == crate::Side::Party
                    && matches!(
                        actor.control,
                        Control::Manual | Control::SemiAuto | Control::Auto
                    ),
                "normal controls require a party actor"
            );
            ensure!(
                actor.side != self.actors[self.targets[index].index()].side,
                "control target must be an opponent"
            );
            ensure!(
                definition.turn_ticks != 0
                    && definition.walk_speed.is_finite()
                    && definition.walk_speed > 0.
                    && definition.run_speed.is_finite()
                    && definition.run_speed >= definition.walk_speed,
                "invalid prepared control movement"
            );
            for (kind, normal) in NormalAttack::ALL.into_iter().zip(definition.normals) {
                ensure!(
                    normal.reach.is_finite()
                        && normal.reach > 0.
                        && normal.minimum_reach.is_finite()
                        && normal.minimum_reach >= 0.
                        && normal.minimum_reach < normal.reach,
                    "invalid prepared normal selector"
                );
                ensure!(
                    self.resources
                        .actions
                        .get(normal.action)
                        .is_some_and(
                            |a| matches!(&a.execution, crate::ActionExecution::Attack(_))
                                && a.tp_cost == 0
                                && a.normal == Some(kind)
                        ),
                    "normal selector needs a zero-cost attack of the matching kind"
                );
            }
            ensure!(
                setup
                    .disabled_techniques
                    .iter()
                    .all(|action| setup.techniques.iter().any(|row| row.action == *action)),
                "disabled technique is outside the actor's prepared capacity"
            );
            for catalogue in definition.shortcuts.into_iter().filter(|&id| id != 0) {
                let learned = self
                    .technique_learning_members
                    .iter()
                    .find(|member| member.actor.index() == index)
                    .map_or_else(
                        || {
                            setup
                                .techniques
                                .iter()
                                .any(|row| row.catalogue == catalogue)
                        },
                        |member| member.member.current().contains(&catalogue),
                    );
                ensure!(learned, "initial shortcut technique is not learned");
            }
        }

        Ok(())
    }
}

/// One validated edit to the existing controller slot. Dropping it changes
/// nothing; the caller may first complete its matching persistent assignment.
#[must_use]
pub struct ShortcutEdit<'a, T> {
    slot: &'a mut T,
    value: T,
}

impl<T> ShortcutEdit<'_, T> {
    /// All fallible work precedes this synchronous assignment.
    pub fn commit(self) {
        *self.slot = self.value;
    }
}

impl Battle {
    pub(crate) fn target_selector_active(&self) -> bool {
        self.target_selector.is_some() || self.command_target.is_some()
    }

    pub(crate) fn admit_target_selector(&mut self, inputs: &[ControlInput]) {
        const TARGET_SELECT_HOLD_TICKS: u16 = 10;
        if self.phase() != crate::BattlePhase::Combat || self.is_paused() {
            return;
        }
        self.target_selector = self.runtime.iter().enumerate().find_map(|(index, state)| {
            let actor = ActorId(index as u8);
            let owner = &self.actors[index];
            (state
                .control
                .as_ref()
                .is_some_and(|control| control.target_ticks >= TARGET_SELECT_HOLD_TICKS)
                && owner.available()
                && owner.time_stop == 0
                && owner.control != Control::Auto
                && inputs
                    .iter()
                    .any(|input| input.actor == actor && input.target.held))
            .then_some(actor)
        });
    }

    /// Project a command page's selected target without consuming navigation.
    pub fn project_command_target(&mut self, selected: Option<(ActorId, ActorId)>) -> Result<()> {
        if let Some((actor, target)) = selected {
            ensure!(
                self.phase() == crate::BattlePhase::Combat,
                "command target requires combat"
            );
            ensure!(
                self.actors
                    .get(actor.index())
                    .is_some_and(|actor| actor.side == crate::Side::Party),
                "command target owner is not a party member"
            );
            ensure!(
                self.actors.get(target.index()).is_some() && self.target_available(actor, target),
                "command target is unavailable"
            );
        }
        self.command_target = selected;
        Ok(())
    }

    /// The four live catalogue assignments. Zero denotes an empty slot.
    pub fn shortcuts(&self, actor: ActorId) -> Option<&[u16; 4]> {
        self.runtime
            .get(actor.index())?
            .control
            .as_ref()
            .map(|control| &control.shortcuts)
    }

    pub(crate) fn shortcut_action(
        &self,
        actor: ActorId,
        catalogue: u16,
    ) -> Option<crate::ActionKey> {
        self.prepared_technique(actor, catalogue)
            .filter(|row| self.learned_technique(actor, row.action).is_some())
            .map(|row| row.action)
    }

    pub fn validate_shortcut(
        &self,
        actor: ActorId,
        slot: usize,
        selected: Option<crate::ActionKey>,
    ) -> Result<()> {
        ensure!(
            self.phase() == crate::BattlePhase::Combat,
            "shortcut assignment requires active combat"
        );
        ensure!(slot < 4, "invalid battle shortcut slot");
        ensure!(
            self.actor(actor)?.side == crate::Side::Party,
            "shortcut assignment requires a party actor"
        );
        self.prepared.actor_setup[actor.index()]
            .control
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("actor has no prepared shortcut bindings"))?;
        ensure!(
            self.runtime[actor.index()].control.is_some(),
            "actor has no live shortcut controller"
        );
        if let Some(selected) = selected {
            ensure!(
                self.learned_technique(actor, selected).is_some(),
                "shortcut differs from its prepared player technique"
            );
        }
        Ok(())
    }

    pub fn prepare_shortcut(
        &mut self,
        actor: ActorId,
        slot: usize,
        selected: Option<crate::ActionKey>,
    ) -> Result<ShortcutEdit<'_, u16>> {
        self.validate_shortcut(actor, slot, selected)?;
        let catalogue = selected
            .and_then(|action| self.prepared.technique(actor, action))
            .map_or(0, |row| row.catalogue);
        let control = self.runtime[actor.index()]
            .control
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("actor has no live shortcut controller"))?;
        Ok(ShortcutEdit {
            slot: &mut control.shortcuts[slot],
            value: catalogue,
        })
    }

    /// Tech mode edits change only the live actor policy. The controller
    /// state, active sequence, target and RNG remain resident for the next
    /// callback visit.
    pub fn set_control_mode(&mut self, actor: ActorId, mode: crate::Control) -> Result<()> {
        ensure!(
            mode != crate::Control::Enemy,
            "Tech cannot assign enemy control"
        );
        let index = actor.index();
        let live_actor = self
            .actors
            .get(index)
            .context("invalid Tech control actor")?;
        ensure!(
            live_actor.side == crate::Side::Party,
            "Tech actor is not a party member"
        );
        ensure!(
            self.phase() == crate::BattlePhase::Combat,
            "Tech mode edit requires active combat"
        );
        ensure!(
            self.runtime
                .get(index)
                .and_then(|state| state.control.as_ref())
                .is_some(),
            "Tech actor has no live controller"
        );
        if mode == crate::Control::Auto {
            ensure!(
                self.prepared
                    .actor_setup
                    .get(index)
                    .and_then(|setup| setup.companion.as_ref())
                    .is_some(),
                "Tech Auto mode has no prepared companion policy"
            );
        }
        self.actors[index].control = mode;
        Ok(())
    }

    pub fn technique_enabled(&self, actor: ActorId, action: crate::ActionKey) -> bool {
        self.runtime
            .get(actor.index())
            .and_then(|state| state.control.as_ref())
            .is_none_or(|control| !control.disabled_techniques.contains(&action))
    }

    /// Enable/disable a prepared technique without replacing its
    /// immutable action binding. Existing action work is untouched.
    pub fn set_technique_enabled(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
        enabled: bool,
    ) -> Result<bool> {
        let index = actor.index();
        ensure!(
            self.phase() == crate::BattlePhase::Combat,
            "Tech enable edit requires active combat"
        );
        ensure!(
            self.actors
                .get(index)
                .is_some_and(|actor| actor.side == crate::Side::Party),
            "Tech actor is not a party member"
        );
        ensure!(
            self.learned_technique(actor, action).is_some(),
            "Tech action is not prepared"
        );
        let control = self
            .runtime
            .get_mut(index)
            .and_then(|state| state.control.as_mut())
            .context("Tech actor has no live controller")?;
        let changed = if enabled {
            control.disabled_techniques.remove(&action)
        } else {
            control.disabled_techniques.insert(action)
        };
        Ok(changed)
    }

    /// Store an assist owner binding in the live controller. The current
    /// companion policy reads the same prepared action catalogue; this
    /// edit therefore never clones or reloads an action resource.
    pub fn prepare_assist_shortcut(
        &mut self,
        owner: ActorId,
        slot: usize,
        selected: Option<(ActorId, crate::ActionKey)>,
    ) -> Result<ShortcutEdit<'_, Option<(ActorId, crate::ActionKey)>>> {
        ensure!(slot < 2, "invalid Tech assist slot");
        let index = owner.index();
        ensure!(
            self.phase() == crate::BattlePhase::Combat,
            "Tech assist edit requires active combat"
        );
        ensure!(
            self.actors
                .get(index)
                .is_some_and(|actor| actor.side == crate::Side::Party),
            "Tech assist owner is not a party member"
        );
        if let Some((target, binding)) = selected {
            ensure!(
                self.actors
                    .get(target.index())
                    .is_some_and(|actor| actor.side == crate::Side::Party),
                "Tech assist target is not a party member"
            );
            ensure!(
                self.learned_technique(target, binding).is_some(),
                "Tech assist action is not prepared"
            );
        }
        let control = self
            .runtime
            .get_mut(index)
            .and_then(|state| state.control.as_mut())
            .context("Tech assist owner has no live controller")?;
        Ok(ShortcutEdit {
            slot: &mut control.assist_shortcuts[slot],
            value: selected,
        })
    }

    pub fn forget_technique(&mut self, actor: ActorId, action: crate::ActionKey) -> Result<()> {
        self.validate_forget_technique(actor, action)?;
        let catalogue = self
            .prepared
            .technique(actor, action)
            .context("forgotten technique has no prepared identity")?
            .catalogue;
        self.learning_members
            .iter_mut()
            .find(|row| row.actor == actor)
            .context("Tech forget needs a learning member")?
            .member
            .forget(catalogue)?;
        let control = self
            .runtime
            .get_mut(actor.index())
            .and_then(|state| state.control.as_mut())
            .context("Tech actor has no live controller")?;
        for shortcut in &mut control.shortcuts {
            if *shortcut == catalogue {
                *shortcut = 0;
            }
        }
        control.disabled_techniques.insert(action);
        // Assist owners retain bindings independently of the recipient's
        // four shortcuts. Forgetting the recipient invalidates every such
        // live edge so a later assist input cannot queue a forgotten action.
        for owner in self
            .runtime
            .iter_mut()
            .filter_map(|state| state.control.as_mut())
        {
            for shortcut in &mut owner.assist_shortcuts {
                if shortcut.is_some_and(|(target, binding)| target == actor && binding == action) {
                    *shortcut = None;
                }
            }
        }
        Ok(())
    }

    pub fn validate_forget_technique(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<()> {
        let index = actor.index();
        ensure!(
            self.current_techniques(actor).is_some(),
            "Tech forget needs a learning member"
        );
        ensure!(
            self.phase() == crate::BattlePhase::Combat,
            "Tech forget requires active combat"
        );
        ensure!(
            self.learned_technique(actor, action).is_some(),
            "Tech forget action is not prepared"
        );
        let control = self
            .runtime
            .get(index)
            .and_then(|state| state.control.as_ref())
            .context("Tech actor has no live controller")?;
        ensure!(
            self.runtime[index].combo.buffered.is_none() && control.queued_technique.is_none(),
            "Tech cannot forget a queued technique"
        );
        ensure!(
            self.actors
                .get(index)
                .is_some_and(|actor| actor.side == crate::Side::Party),
            "Tech forget owner is not a party member"
        );
        Ok(())
    }

    /// Queue a prepared player action for the next eligible callback. This is
    /// deliberately separate from immediate `start_actor_command`: the page
    /// closes while the current action/cast/target/RNG owner remains intact.
    pub fn queue_technique(&mut self, actor: ActorId, action: crate::ActionKey) -> Result<bool> {
        self.queue_technique_from(actor, action, actor)
    }

    /// Current deterministic price, without reserving TP or drawing a payment waiver.
    pub fn technique_tp_cost(&self, actor: ActorId, action: crate::ActionKey) -> Option<u32> {
        self.actors.get(actor.index())?;
        self.prepared.actions.get(action)?;
        Some(self.action_quote(actor, action))
    }

    pub fn technique_queue_admitted(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<bool> {
        let index = actor.index();
        if self.phase() != crate::BattlePhase::Combat
            || !self
                .actors
                .get(index)
                .is_some_and(|actor| actor.available())
            || !self.actors[index].conditions.arte_queue_allowed()
        {
            return Ok(false);
        }
        let Some(_) = self.learned_technique(actor, action) else {
            return Ok(false);
        };
        Ok(self.action_quote(actor, action) <= u32::from(self.actors[index].tp))
    }

    pub fn queue_technique_from(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
        issuer: ActorId,
    ) -> Result<bool> {
        let target = self
            .target(actor)
            .filter(|&target| self.technique_target_eligible(actor, action, target))
            .or_else(|| {
                self.actors
                    .iter()
                    .enumerate()
                    .map(|(index, _)| ActorId(index as u8))
                    .find(|&target| self.technique_target_eligible(actor, action, target))
            });
        let Some(target) = target else {
            return Ok(false);
        };
        self.queue_technique_target_from(actor, action, target, issuer)
    }

    /// Queue an explicit Tech command against the page's selected target.
    /// The normal combo buffer keeps its own target and remains untouched.
    pub fn queue_technique_target(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
        target: ActorId,
    ) -> Result<bool> {
        self.queue_technique_target_from(actor, action, target, actor)
    }

    pub fn queue_technique_target_from(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
        target: ActorId,
        issuer: ActorId,
    ) -> Result<bool> {
        let issuer = self.actor(issuer)?;
        ensure!(
            issuer.side == crate::Side::Party && issuer.control_slot < 4,
            "invalid Tech issuing controller"
        );
        let issuer = issuer.control_slot;
        let index = actor.index();
        ensure!(
            self.phase() == crate::BattlePhase::Combat,
            "Tech queue requires active combat"
        );
        ensure!(
            self.actors
                .get(index)
                .is_some_and(|actor| actor.side == crate::Side::Party),
            "Tech queue owner is not a party member"
        );
        if !self.technique_queue_admitted(actor, action)?
            || !self.technique_target_eligible(actor, action, target)
        {
            return Ok(false);
        }
        let control = self
            .runtime
            .get_mut(index)
            .and_then(|state| state.control.as_mut())
            .context("Tech queue owner has no live controller")?;
        if control.queued_technique.is_some() {
            return Ok(false);
        }
        // The explicit menu command is independent of the companion policy
        // toggle.  `disabled_techniques` only gates automatic admissions.
        control.queued_technique = Some(PendingTechnique {
            action,
            target: Some(target),
            issuer: Some(issuer),
        });
        self.pending_cues.push(Cue::TechniqueQueued { actor });
        Ok(true)
    }
}

impl Battle {
    /// Read physical target-button hold durations without consuming input.
    pub fn target_hold_counts(&self) -> impl Iterator<Item = Option<u16>> + '_ {
        self.runtime
            .iter()
            .map(|state| state.control.as_ref().map(|control| control.target_ticks))
    }

    pub fn target(&self, actor: ActorId) -> Option<ActorId> {
        self.runtime.get(actor.index()).map(|state| state.target)
    }

    /// Sample physical hold duration once per input frame, including paused frames.
    pub(crate) fn sample_target_holds(&mut self, inputs: &[ControlInput]) {
        for (index, state) in self.runtime.iter_mut().enumerate() {
            let Some(control) = &mut state.control else {
                continue;
            };
            let input = inputs.iter().find(|input| input.actor.index() == index);
            control.target_ticks = match input.filter(|input| input.target.held) {
                None => 0,
                Some(input) if input.target.pressed => 1,
                Some(_) => control.target_ticks.saturating_add(1),
            };
        }
    }

    pub(crate) fn validate_controls(&self, inputs: &[ControlInput]) -> Result<()> {
        for (i, input) in inputs.iter().enumerate() {
            ensure!(
                // Result construction retires the mutable combat controller;
                // the prepared input binding remains valid through that handoff.
                self.prepared
                    .actor_setup
                    .get(input.actor.index())
                    .is_some_and(|setup| setup.control.is_some()),
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
                    && (-1..=1).contains(&input.horizontal_pressed)
                    && (-1..=1).contains(&input.vertical_pressed),
                "invalid battle direction input"
            );
        }
        Ok(())
    }

    /// Select commands for an active actor; the actor update owns movement.
    pub(crate) fn control_actor(
        &mut self,
        actor: ActorId,
        input: ControlInput,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if self.phase() != crate::BattlePhase::Combat
            && self.terminal.result != Some(crate::BattleResult::Victory)
        {
            return Ok(());
        }
        let index = actor.index();
        if self.runtime[index].control.is_none() {
            return Ok(());
        }
        let definition = Arc::clone(self.prepared.actor_setup[index].control.as_ref().unwrap());
        if !self.target_available(actor, self.runtime[index].target)
            && let Some(target) = self.nearest_target(actor, None)
        {
            self.runtime[index].target = target;
        }
        let control = self.runtime[index].control.as_mut().unwrap();
        control.combo_input = input;
        let normal_active = self.runtime[index]
            .task()
            .action()
            .is_some_and(|(_, sequence)| sequence.normal.is_some());
        let activity = self.activity(actor);
        let stopping = (activity == Activity::Idle
            && self.actors[index].movement.locomotion == Locomotion::Stop)
            || activity == Activity::Approaching && self.recovery_return_stopping(index);
        let moving = activity == Activity::Approaching && self.approach_moving(index);
        let ordinary = activity == Activity::Idle && !stopping;
        if self.actors[index].control == Control::Auto && stopping {
            self.actors[index].input = InputIntent::default();
        }
        if self.actors[index].control == Control::Auto {
            return Ok(());
        }
        if stopping || ordinary {
            self.project_ground_command(&definition, input, stopping, cues);
        } else if moving {
            self.project_moving_command(&definition, input, cues);
        } else if !normal_active {
            self.player_control(&definition, input, cues)?;
        }
        self.consume_assist_input(actor, input)?;
        if input.target.released
            && let Some(target) = self.nearest_target(actor, Some(self.runtime[index].target))
        {
            if target != self.runtime[index].target {
                match self.activity(actor) {
                    Activity::Taunting => {
                        self.refresh_selector_facing(actor, target);
                    }
                    _ if stopping || moving || ordinary => {
                        self.actors[index].input.face_target = true
                    }
                    _ => {}
                }
            }
            self.runtime[index].target = target;
        }
        let intent = self.actors[index].input;
        let replaces_movement = intent.action.is_some()
            || intent.jump
            || intent.motion == crate::action_selection::GroundMotion::Guard
            || self.actors[index].equipment.quick_turn
                && matches!(
                    intent.motion,
                    crate::action_selection::GroundMotion::Walk
                        | crate::action_selection::GroundMotion::Run
                );
        if (stopping || moving) && replaces_movement {
            self.set_task(index, ActorTask::None);
            self.actors[index].movement.locomotion = Locomotion::Idle;
        }
        if ordinary || (stopping || moving) && replaces_movement {
            self.execute_ground_command(actor, &definition, cues)?;
        }
        Ok(())
    }

    /// Actor actions and casts wait for the same facing rule before advancing.
    pub(crate) fn face_active_action(&mut self, actor: ActorId) -> bool {
        let index = actor.index();
        if !self.actors[index].available()
            || !self.runtime[index]
                .task()
                .action()
                .is_some_and(|(_, sequence)| sequence.running())
        {
            return true;
        }
        let Some(turn_ticks) = self.prepared.actor_setup[index].turn_ticks() else {
            return true;
        };
        let actor = &mut self.actors[index];
        if actor.movement.turning_disabled || actor.side == crate::Side::Party && actor.airborne() {
            return true;
        }
        face(
            actor,
            actor.movement.direction,
            180. / f32::from(turn_ticks),
        )
    }

    pub(crate) fn normal_entry_faces(&self, index: usize) -> bool {
        let actor = &self.actors[index];
        self.runtime[index].control.as_ref().is_some_and(|control| {
            !actor.movement.turning_disabled
                && !actor.airborne()
                && self.runtime[index].target == control.attack_target
                && (actor.control != Control::Manual
                    || crate::distance::dot(
                        actor.facing_direction,
                        actor.movement.target_direction,
                    ) >= 0.)
        })
    }

    pub(crate) fn initialize_normal(&mut self, sequence: &mut crate::action::Sequence) {
        let actor = sequence.actor;
        let index = actor.index();
        let Some(definition) = &self.prepared.actor_setup[index].control else {
            return;
        };
        let Some(attack) = sequence.definition.normal else {
            return;
        };
        if self.normal_entry_faces(index) {
            let turn = 180. / f32::from(definition.turn_ticks);
            let owner = &mut self.actors[index];
            let direction = owner.movement.target_direction;
            owner.movement.direction = direction;
            owner.facing_direction = direction;
            face(owner, direction, turn);
        }
        sequence.normal = Some(NormalState::Attack(attack));
        self.runtime[index].control.as_mut().unwrap().run_ticks = 0;
        self.actors[index].movement.airborne_action = attack.airborne();
        self.runtime[index].combo.normal_kinds |= 1 << attack as u8;
    }

    pub(crate) fn start_control_normal(
        &mut self,
        actor: ActorId,
        definition: &ControlDefinition,
        selection: NormalAttack,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        self.start_actor_command(
            ActionRequest {
                actor,
                action: definition.normals[selection as usize].action,
                target: self.runtime[actor.index()].target,
            },
            cues,
        )
    }

    pub(crate) fn sample_action_combo(&mut self, sequence: &crate::action::Sequence) {
        let actor = sequence.actor;
        let index = actor.index();
        if !matches!(
            self.actors[index].control,
            Control::Manual | Control::SemiAuto
        ) {
            return;
        }
        if sequence
            .combo
            .is_none_or(|window| sequence.age > window.buffer_until)
        {
            return;
        }
        let Some(control) = self.runtime[index].control.as_ref() else {
            return;
        };
        let owner = &self.actors[index];
        let input = control.combo_input;
        let combo = self.runtime[index].combo;
        let buffered = if input.attack.pressed {
            let Some(normal) = &sequence.normal else {
                return;
            };
            let limit = owner.equipment.normal_combo_limit;
            if combo.normal_links >= limit.saturating_sub(1)
                || !combo.history.is_empty()
                || normal.airborne() && !owner.equipment.combo_traits.sky_combo
            {
                return;
            }
            let direction = NormalAttack::from_input(input.stick, owner.airborne());
            let (allowed, fallback) = normal.continuations(owner.equipment.combo_traits.jump_combo);
            let next = allowed
                .contains(&direction)
                .then_some(direction)
                .or(fallback);
            next.map(BufferedAction::Normal)
        } else if input.technique.pressed {
            let direction = NormalAttack::from_input(input.stick, false) as usize;
            self.shortcut_action(actor, control.shortcuts[direction])
                .map(BufferedAction::Technique)
        } else {
            (0..2).rev().find_map(|slot| {
                input.assist[slot]
                    .pressed
                    .then_some(control.assist_shortcuts[slot])
                    .flatten()
                    .filter(|(owner, _)| *owner == actor)
                    .map(|(_, action)| BufferedAction::Technique(action))
            })
        };
        if let Some(buffered) = buffered {
            self.runtime[index].combo.buffered = Some(buffered);
        }
    }

    pub(crate) fn chain_normal(
        &mut self,
        id: ActionId,
        sequence: &crate::action::Sequence,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let actor = sequence.actor;
        let index = actor.index();
        if self.actors[index].control_ex_state.counter_active
            && !self.actors[index].equipment.combo_traits.counter_combo
            || self.actors[index].control == Control::Enemy
        {
            return Ok(false);
        }
        let Some(window) = sequence.combo else {
            return Ok(false);
        };
        if sequence.started_at == self.update || sequence.age < window.opens_at {
            return Ok(false);
        }
        if self.actors[index].control == Control::Auto && !self.visit_companion_combo(sequence)? {
            return Ok(false);
        }
        let Some(pending) = self.runtime[index].combo.buffered.take() else {
            return Ok(false);
        };
        let started = match pending {
            BufferedAction::Technique(action) => {
                if self.can_use_aerial_spell(actor, action) {
                    self.try_aerial_spell_selection(actor, action, Some(id), cues)?;
                    return Ok(false);
                }
                if !self.chain_technique_available(actor, action) {
                    return Ok(false);
                }
                let explicit_target = self.runtime[index]
                    .control
                    .as_ref()
                    .and_then(|control| control.queued_technique)
                    .filter(|pending| pending.action == action)
                    .and_then(|pending| pending.target);
                if explicit_target
                    .is_some_and(|target| !self.technique_target_eligible(actor, action, target))
                {
                    self.clear_technique_command(actor);
                    return Ok(false);
                }
                let started = self.start_actor_command(
                    ActionRequest {
                        actor,
                        action,
                        target: explicit_target.unwrap_or(self.runtime[index].target),
                    },
                    cues,
                )?;
                if started {
                    let variety = self.runtime[index].combo.normal_kinds.count_ones() as u8;
                    self.ledger.normal_variety[index] =
                        self.ledger.normal_variety[index].max(variety);
                }
                started
            }
            BufferedAction::Normal(next) => {
                if !self.runtime[index].combo.history.is_empty() {
                    return Ok(false);
                }
                let definition =
                    Arc::clone(self.prepared.actor_setup[index].control.as_ref().unwrap());
                if !self.start_control_normal(actor, &definition, next, cues)? {
                    return Ok(false);
                }
                let owner = &mut self.actors[index];
                owner.attack_power = owner.attack_power.saturating_sub(15).max(10);
                if owner.equipment.combo_traits.combo_force {
                    owner.attack_power = owner.attack_power.saturating_add(5);
                }
                // Only an actual aerial-to-aerial transition supplies another lift.
                if next.airborne() && sequence.normal.is_some_and(NormalState::airborne) {
                    owner.movement.vertical = AERIAL_COMBO_LIFT;
                }
                true
            }
        };
        if started {
            self.apply_combo_flash(actor);
            cues.push(Cue::Completed { action: id });
        }
        Ok(started)
    }

    fn chain_technique_available(&self, actor: ActorId, action: crate::ActionKey) -> bool {
        self.chain_count_available(actor, self.runtime[actor.index()].combo.normal_links)
            && self.chain_descriptor_available(actor, action)
    }

    fn apply_combo_flash(&mut self, actor: ActorId) {
        let owner = &mut self.actors[actor.index()];
        if owner.equipment.combo_traits.flash {
            owner.reaction.protection.armor(10);
        }
    }

    pub(crate) fn confirm_actor_contact(&mut self, owner: ActorId) {
        self.runtime[owner.index()].combo.confirmed_contact = true;
    }

    fn visit_companion_combo(&mut self, sequence: &crate::action::Sequence) -> Result<bool> {
        let actor = sequence.actor;
        let index = actor.index();
        if self.prepared.actor_setup[index].companion.is_none() {
            return Ok(false);
        }
        let Some(window) = sequence.combo else {
            return Ok(false);
        };
        let combo = self.runtime[index].combo;
        let contact = combo.confirmed_contact
            || self
                .combo_technique(actor)
                .is_some_and(|row| row.capabilities.chains_without_contact);
        if !contact || sequence.age > window.buffer_until {
            return Ok(false);
        }
        self.advance_ai_combo(actor, sequence.normal)
    }

    pub(crate) fn queue_companion_chain(
        &mut self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Result<bool> {
        let index = actor.index();
        ensure!(
            self.prepared.actor_setup[index].companion.is_some(),
            "chain needs a companion policy"
        );
        let pending = if let Some(normal) = self.prepared.actions[action].normal {
            if !self.runtime[index].combo.history.is_empty() {
                return Ok(false);
            }
            BufferedAction::Normal(normal)
        } else {
            if self.pending_technique(actor) != Some(action)
                && !self.technique_enabled(actor, action)
                || !self.chain_technique_available(actor, action)
            {
                return Ok(false);
            }
            BufferedAction::Technique(action)
        };
        if self
            .action_candidate(actor, action, self.selection_range(actor, action))
            .is_err()
        {
            return Ok(false);
        }
        self.runtime[index].combo.buffered = Some(pending);
        Ok(true)
    }

    pub(crate) fn apply_locomotion(
        &mut self,
        index: usize,
        definition: &ControlDefinition,
        locomotion: Locomotion,
    ) -> Result<()> {
        let control = self.runtime[index].control.as_mut().unwrap();
        let actor = &mut self.actors[index];
        match locomotion {
            Locomotion::Walk => {
                actor.movement.forward = actor.walk_speed(
                    definition.walk_speed,
                    actor.body.scale,
                    actor.conditions.effective(),
                );
                control.run_ticks = 0;
            }
            Locomotion::Run => {
                if control.run_ticks == 0 {
                    actor.movement.forward = definition.walk_speed;
                }
                actor.movement.forward = (actor.movement.forward
                    + crate::movement::RUN_ACCELERATION)
                    .min(actor.run_limit(definition.run_speed, actor.conditions.effective()));
                control.run_ticks = control.run_ticks.saturating_add(1);
            }
            Locomotion::Idle => {
                actor.movement.forward = 0.;
                control.run_ticks = 0;
            }
            Locomotion::Stop => control.run_ticks = 0,
            Locomotion::Action => unreachable!(),
        }
        if matches!(locomotion, Locomotion::Walk | Locomotion::Run) {
            actor.facing_direction = actor.movement.direction;
        }
        self.control_motion(index, definition, locomotion)
    }

    pub(crate) fn control_motion(
        &mut self,
        index: usize,
        definition: &ControlDefinition,
        locomotion: Locomotion,
    ) -> Result<()> {
        let changed = self.actors[index].movement.locomotion != locomotion;
        self.actors[index].movement.locomotion = locomotion;
        if changed
            && locomotion != Locomotion::Idle
            && let Some(motions) = definition.motions
        {
            let (motion, blend, repeat) = match locomotion {
                Locomotion::Walk => (motions.walk, WALK_BLEND_TICKS, true),
                Locomotion::Run => (motions.run, RUN_BLEND_TICKS, true),
                Locomotion::Stop => (motions.stop, STOP_BLEND_TICKS, false),
                Locomotion::Idle | Locomotion::Action => unreachable!(),
            };
            let actor = &self.actors[index];
            let rate = if repeat {
                actor.motion_rate(MOVEMENT_PLAYBACK_RATE, actor.conditions.effective())
            } else {
                MOVEMENT_PLAYBACK_RATE
            };
            self.request_pose(
                ActorId(index as u8),
                Some(motion),
                crate::Pose {
                    rate,
                    repeat,
                    blend,
                    ..Default::default()
                },
            );
        }
        Ok(())
    }

    pub(crate) fn target_available(&self, actor: ActorId, target: ActorId) -> bool {
        let target = &self.actors[target.index()];
        target.side != self.actors[actor.index()].side && target.available()
    }

    pub(crate) fn update_control_stop(&mut self, index: usize) -> Result<bool> {
        if self.activity(ActorId(index as u8)) != Activity::Idle
            || self.runtime[index].control.is_none()
            || self.actors[index].movement.locomotion != Locomotion::Stop
        {
            return Ok(false);
        }
        if self.actors[index].hit_stop > 0 {
            return Ok(true);
        }
        let actor = &mut self.actors[index];
        actor.movement.integrate(&mut actor.position);
        if actor.movement.brake(actor.position[1], Activity::Idle) {
            self.actors[index].movement.locomotion = Locomotion::Idle;
        }
        Ok(true)
    }

    pub(crate) fn sample_ground_stick(&mut self, index: usize, stick: i8) {
        if let Some(control) = &mut self.runtime[index].control {
            control.horizontal_delta =
                (i16::from(control.last_horizontal) - i16::from(stick)).abs();
            control.last_horizontal = stick;
        }
    }

    pub(crate) fn brake_control(&mut self, index: usize) -> Result<Option<(ActionId, u32)>> {
        let mut recovery = None;
        if let Some((&id, sequence)) = self.runtime[index]
            .action_mut()
            .filter(|(_, sequence)| sequence.normal.is_some() && sequence.running())
        {
            let actor = &mut self.actors[index];
            if actor.movement.vertical > 0. && actor.airborne() {
                actor.movement.airborne_action = true;
            }
            if actor.movement.airborne_action && !actor.airborne() {
                sequence.cancel_attack_events();
                recovery = Some((id, 16));
                if let Some(motions) = self.prepared.actor_setup[index]
                    .control
                    .as_ref()
                    .unwrap()
                    .motions
                {
                    self.request_pose(
                        ActorId(index as u8),
                        Some(motions.landing),
                        crate::Pose {
                            blend: 6,
                            ..Default::default()
                        },
                    );
                }
            }
        }
        Ok(recovery)
    }

    pub(crate) fn finish_normal_landing(&mut self, sequence: &mut crate::action::Sequence) {
        let index = sequence.actor.index();
        let combo = &mut self.runtime[index].combo;
        if matches!(combo.buffered, Some(BufferedAction::Normal(_))) {
            combo.buffered = None;
        }
        sequence.normal = Some(NormalState::Landing);
        if let Some(window) = &mut sequence.combo {
            window.opens_at = sequence.age;
        }
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

    fn target_screen_x(&self, actor: ActorId) -> f32 {
        let center = self.actors[actor.index()].target_center();
        self.camera
            .as_ref()
            .map_or(center[0], |camera| project_screen_x(camera.pose, center))
    }

    pub(crate) fn control_target_selector(&mut self, inputs: &[ControlInput]) -> Result<()> {
        let actor = self.target_selector.unwrap();
        let input = inputs
            .iter()
            .find(|input| input.actor == actor)
            .copied()
            .unwrap_or_else(|| ControlInput::neutral(actor));
        let previous = self.runtime[actor.index()].target;
        let Some(target) = self.select_target_step(actor, Some(previous), input.target_step) else {
            self.target_selector = None;
            return Ok(());
        };
        self.runtime[actor.index()].target = target;
        self.render_target_selector(target)?;
        if !input.target.held {
            self.target_selector = None;
            self.refresh_selector_facing(actor, target);
        }
        Ok(())
    }

    pub(crate) fn render_command_target(&mut self) -> Result<()> {
        if let Some((_, target)) = self.command_target {
            self.render_target_selector(target)?;
        }
        Ok(())
    }

    /// Choose among live opponents in screen order. The caller owns the selected target.
    pub fn select_target_step(
        &self,
        actor: ActorId,
        previous: Option<ActorId>,
        step: i8,
    ) -> Option<ActorId> {
        self.actors.get(actor.index())?;
        let mut candidates: Vec<_> = (0..self.actors.len())
            .map(|index| ActorId(index as u8))
            .filter(|&target| self.target_available(actor, target))
            .collect();
        candidates.sort_by(|a, b| {
            self.target_screen_x(*a)
                .total_cmp(&self.target_screen_x(*b))
                .then(a.index().cmp(&b.index()))
        });
        let index =
            previous.and_then(|selected| candidates.iter().position(|&target| target == selected));
        let Some(index) = index else {
            return candidates.first().copied();
        };
        let next =
            (index as isize + isize::from(step.signum())).rem_euclid(candidates.len() as isize);
        Some(candidates[next as usize])
    }

    fn render_target_selector(&mut self, target: ActorId) -> Result<()> {
        if let Some(camera) = &mut self.camera {
            camera.step(
                &self.actors,
                self.runtime[camera.definition.leader.index()].target,
                crate::camera::Update::Selecting(target),
            )?;
        }
        Ok(())
    }

    pub(crate) fn refresh_selector_facing(&mut self, actor: ActorId, target: ActorId) {
        let activity = self.activity(actor);
        let target = self.actors[target.index()].position;
        let owner = &mut self.actors[actor.index()];
        if owner.airborne() || matches!(activity, Activity::Action | Activity::Guarding) {
            return;
        }
        let direction =
            crate::distance::planar_direction(target, owner.position, owner.facing_direction);
        owner.facing_direction = direction;
    }
}

pub fn project_screen_x(camera: crate::CameraPose, point: [f32; 3]) -> f32 {
    project_screen_point(camera, point)[0]
}

pub fn project_screen_point(camera: crate::CameraPose, point: [f32; 3]) -> [f32; 2] {
    let [eye_x, eye_y, eye_z] = project_view(camera, point);
    let angle = (0.5 * crate::VERTICAL_FOV_DEGREES).to_radians();
    let cotangent = 1. / angle.tan();
    let projection = cotangent / (4_f32 / 3.);
    let clip_x = eye_x * projection + eye_z * 0.;
    let clip_y = eye_y * cotangent + eye_z * 0.;
    [
        320. + ((1. / -eye_z) * (clip_x * 640. / 2.)),
        224. + ((1. / -eye_z) * (-clip_y * 448. / 2.)),
    ]
}

fn project_view(camera: crate::CameraPose, point: [f32; 3]) -> [f32; 3] {
    let forward =
        crate::distance::normalize(std::array::from_fn(|i| camera.eye[i] - camera.focus[i]));
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

pub fn direction_from_heading(heading: f32) -> [f32; 3] {
    let (sin, cos) = heading.to_radians().sin_cos();
    [sin, 0., cos]
}

/// Turn through the shortest planar arc, including opposite directions.
pub(crate) fn turn_direction(current: [f32; 3], desired: [f32; 3], max_degrees: f32) -> [f32; 3] {
    if desired[0].hypot(desired[2]) <= f32::EPSILON {
        return current;
    }
    if current[0].hypot(current[2]) <= f32::EPSILON {
        return crate::distance::normalize(desired);
    }
    let heading = current[0].atan2(current[2]).to_degrees();
    let wanted = desired[0].atan2(desired[2]).to_degrees();
    direction_from_heading(turn_heading(heading, wanted, max_degrees).0)
}

fn turn_heading(heading: f32, wanted: f32, step: f32) -> (f32, bool) {
    let delta = (wanted - heading + 180.).rem_euclid(360.) - 180.;
    let ready = step == 0. || delta.abs() <= step;
    let turn = if ready { delta } else { delta.signum() * step };
    ((heading + turn + 180.).rem_euclid(360.) - 180., ready)
}

pub(crate) fn snap_heading(actor: &mut crate::Actor, direction: [f32; 3]) {
    face(actor, direction, 0.);
}

/// Advance rendered heading toward desired facing without changing movement direction.
pub(crate) fn face(actor: &mut crate::Actor, direction: [f32; 3], step: f32) -> bool {
    if direction[0].hypot(direction[2]) <= f32::EPSILON {
        return true;
    }
    actor.facing_direction = direction;
    if step != 0. && (actor.movement.turning_disabled || !actor.movement.hover_ready()) {
        return false;
    }
    let wanted = direction[0].atan2(direction[2]).to_degrees();
    let (heading, ready) = turn_heading(actor.heading, wanted, step);
    actor.heading = heading;
    ready
}

pub(crate) fn body_gap(actor: &crate::Actor, target: &crate::Actor) -> f32 {
    let distance = |a: [f32; 3], b: [f32; 3]| {
        (f64::from(a[0]) - f64::from(b[0])).hypot(f64::from(a[2]) - f64::from(b[2]))
    };
    let gap = distance(actor.position, target.position)
        - f64::from(actor.body_radius())
        - f64::from(target.body_radius());
    gap.clamp(0., f64::from(f32::MAX)) as f32
}

#[cfg(test)]
mod tests;
