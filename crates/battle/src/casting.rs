//! Casting progress, payment, storage and independent spell release.
use crate::conditions::Condition;
use crate::{ActionExecution, ActorId, Battle, Cue};
use anyhow::{Result, ensure};
use std::sync::Arc;

impl crate::prepare::BattleResources {
    pub(crate) fn casting(&self, action: crate::ActionKey) -> Option<&CastingDefinition> {
        match &self.actions.get(action)?.execution {
            ActionExecution::Casting(cast) => Some(cast),
            _ => None,
        }
    }
}

/// An ordinary cast can become a prepared spell after its training requirements are met.
#[derive(Debug, Clone, Copy)]
pub struct CastReplacement {
    pub input: crate::ActionKey,
    pub output: crate::ActionKey,
    pub required_uses: &'static [(u16, u16)],
    pub trigger: CastReplacementTrigger,
}

#[derive(Debug, Clone, Copy)]
pub enum CastReplacementTrigger {
    Chance { one_in: u16 },
    Overlimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastPhase {
    /// Casting sound at admission; artwork follows the live cast cadence.
    Started,
    /// Voice timed to finish shortly before casting completes.
    Incantation,
    Chanting,
    Charged,
    Stored,
    /// A stored scene announces its cast before the opening transition.
    SceneOpening,
    Released,
}

pub(crate) enum CastingRun {
    Starting,
    Running(CastRun),
}

pub(crate) fn step(
    battle: &mut Battle,
    id: crate::ActionId,
    sequence: &mut crate::action::Sequence,
    run: CastingRun,
    cues: &mut Vec<Cue>,
) -> Result<Option<u32>> {
    match run {
        CastingRun::Starting => {
            battle.replace_cast(sequence);
            let ActionExecution::Casting(prepared) = &sequence.definition.execution else {
                unreachable!("native cast has a casting definition")
            };
            let actor = sequence.actor;
            if let Some(threshold) = prepared.armor {
                let armor = &mut battle.actors[actor.index()].reaction.armor;
                armor.threshold = threshold;
                armor.received = 0;
            }
            let cast = CastRun::new(battle, sequence, Arc::clone(prepared));
            if cast.definition.chant.is_empty() {
                battle.model_requests.push(crate::ModelRequest::Common {
                    actor,
                    pose: crate::CommonPose::Chant,
                });
            } else {
                cast.chant_pose(battle, actor, 0);
            }
            cues.push(Cue::Casting {
                actor,
                action: sequence.action,
                phase: CastPhase::Started,
            });
            sequence.execution = crate::action::Execution::Casting(CastingRun::Running(cast));
        }
        CastingRun::Running(mut cast) => match cast.step(battle, id, sequence, cues)? {
            Progress::Casting => {
                sequence.execution = crate::action::Execution::Casting(CastingRun::Running(cast))
            }
            Progress::Recovery(remaining) => return Ok(Some(remaining)),
            Progress::Finished => sequence.execution = crate::action::Execution::Finished,
        },
    }
    Ok(None)
}

/// Live equipped casting operands. Recipe validation remains in party preparation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CastingTraits {
    pub speed_cast: bool,
    pub angel_song: bool,
    pub rhythm: bool,
    pub spell_save: bool,
    pub reprise: bool,
    pub nimble: bool,
    pub quick: bool,
    pub random: bool,
    pub reducer: bool,
    pub lucky_magic: bool,
}

/// Repeated and interrupted cast memories survive ordinary actor resets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CastingState {
    /// Live spell strength, shared by the caster's already released contacts.
    pub power_percent: u8,
    pub(crate) previous_spell: Option<crate::ActionKey>,
    /// Prepared action and remaining ticks retained for Spell Save.
    pub(crate) interrupted: Option<(crate::ActionKey, u32)>,
}

/// The global input visit precedes every actor callback. In particular, an
/// Auto caster may read a command issued by a later party slot.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CastInput {
    pub(crate) attack: bool,
    pub(crate) attack_held: bool,
    pub(crate) up_pressed: bool,
    pub(crate) technique: bool,
    pub(crate) guard: bool,
    pub(crate) delay_spell: bool,
}

/// Prepared Special Guard threat parameters, published when casting releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastingThreat {
    pub catalogue: u16,
    pub offensive: bool,
    pub element: u8,
}

#[derive(Debug, Clone)]
pub struct CastingDefinition {
    /// Replace the actor's idle hit armor when this cast begins.
    pub armor: Option<u8>,
    pub duration: u32,
    /// Remaining cast ticks when the incantation begins, including its quiet tail.
    pub incantation_lead: Option<u32>,
    /// Ignore casting accelerators and saved progress for an authored fixed clock.
    pub fixed_timing: bool,
    /// Actor timing used by casting skills and Over Limit.
    pub profile_ticks: i16,
    /// Spell timing used by equipment that accelerates casting.
    pub spell_ticks: i16,
    pub recovery: u32,
    /// Optional authored opening and loop poses on the existing casting clock.
    pub chant: Vec<(u16, crate::AttackPose)>,
    pub release_pose: CastReleasePose,
    /// Hide held equipment only while this cast owns its chanting or release pose.
    pub hide_primary_weapons: bool,
    pub release: Arc<crate::PreparedSpell>,
    pub threat: Option<CastingThreat>,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum CastReleasePose {
    #[default]
    Ordinary,
    /// Begin while chanting; resident spells wait for payment and gesture completion.
    Anticipated {
        release: Option<crate::AttackPose>,
        release_loop_start: f32,
    },
    /// Fit the release gesture to the end of the scene transition.
    Scene {
        opening: Option<crate::AttackPose>,
        release: Option<crate::AttackPose>,
        release_loop_start: f32,
        at: u16,
    },
}

impl Battle {
    fn replace_cast(&mut self, sequence: &mut crate::action::Sequence) {
        let actor = sequence.actor;
        let Some(rule) = self.prepared.actor_setup[actor.index()].cast_replacement else {
            return;
        };
        if sequence.unison
            || self.unison.active()
            || sequence.action != rule.input
            || self.actors[actor.index()].tp < self.prepared.actions[rule.output].tp_cost
            || rule
                .required_uses
                .iter()
                .any(|&(arte, minimum)| self.technique_uses(actor, arte).unwrap_or(0) < minimum)
        {
            return;
        }
        let triggered = match rule.trigger {
            CastReplacementTrigger::Chance { one_in } => {
                self.random.next_u16().is_multiple_of(one_in)
            }
            CastReplacementTrigger::Overlimit => self.actors[actor.index()].overlimit.is_active(),
        };
        if !triggered {
            return;
        }
        if let Some(pending) = self.runtime[actor.index()]
            .control
            .as_mut()
            .and_then(|control| control.queued_technique.as_mut())
            .filter(|pending| pending.action == rule.input)
        {
            pending.action = rule.output;
        }
        sequence.action = rule.output;
        sequence.definition = Arc::clone(&self.prepared.actions[rule.output]);
        self.snapshot_technique_proficiency(actor, rule.output);
    }

    pub(crate) fn advance_cast_recovery_pose(&mut self, actor: ActorId, remaining: u32) {
        let Some(models) = &self.models else {
            return;
        };
        let Some(model) = models.definition(actor) else {
            return;
        };
        let Some(clip) = model.reactions.cast_recovery else {
            return;
        };
        let pose = crate::Pose {
            restart: false,
            ..Default::default()
        };
        let Some(duration) = models.motion_duration(actor, clip) else {
            return;
        };
        let duration = (duration / pose.rate) as u32 + u32::from(pose.blend);
        if remaining > 0 && remaining <= duration {
            self.request_pose(
                actor,
                Some(crate::MotionBinding {
                    model: model.resource,
                    clip,
                }),
                pose,
            );
        }
    }

    pub(crate) fn cast_hides_primary_weapons(&self, actor: ActorId) -> bool {
        if !self.actors[actor.index()].available() {
            return false;
        }
        let Some(models) = &self.models else {
            return false;
        };
        let (Some(model), Some(pose)) = (
            models.definition(actor),
            models.main_motion_observation(actor),
        ) else {
            return false;
        };
        self.sequences().any(|(_, sequence)| {
            if sequence.actor != actor
                || !matches!(
                    &sequence.execution,
                    crate::action::Execution::Casting(_)
                        | crate::action::Execution::Recovering { .. }
                )
            {
                return false;
            }
            let ActionExecution::Casting(cast) = &sequence.definition.execution else {
                return false;
            };
            cast.hide_primary_weapons
                && (model.reactions.chant == Some(pose.clip)
                    || model.reactions.cast == Some(pose.clip)
                    || matches!(cast.release_pose, CastReleasePose::Scene { opening: Some(opening), .. }
                        if opening.motion.model == model.resource && opening.motion.clip == pose.clip))
        })
    }

    pub(crate) fn learning_actor(&self) -> Option<ActorId> {
        self.sequences().find_map(|(_, sequence)| {
            (sequence.learning
                || matches!(
                    &sequence.execution,
                    crate::action::Execution::Casting(CastingRun::Running(cast)) if cast.learning
                ))
            .then_some(sequence.actor)
        })
    }

    pub(crate) fn clear_cast_memory(&mut self, actor: ActorId) {
        let state = &mut self.actors[actor.index()].casting_state;
        state.previous_spell = None;
        state.interrupted = None;
    }

    pub(crate) fn sample_cast_inputs(&mut self, inputs: &[crate::ControlInput]) {
        self.cast_inputs.fill(CastInput::default());
        for input in inputs {
            let slot = usize::from(self.actors[input.actor.index()].control_slot);
            if let Some(sample) = self.cast_inputs.get_mut(slot) {
                // Several actors can retain one physical slot. A filtered Auto
                // callback sample must not erase another sample of that channel.
                sample.attack |= input.attack.pressed;
                sample.attack_held |= input.attack.held;
                sample.up_pressed |= input.vertical_pressed > 0;
                sample.technique |= input.technique.held;
                sample.guard |= input.guard.held;
                sample.delay_spell |= input.delay_spell.held;
            }
        }
    }

    pub(crate) fn cast_guard_requested(&self, actor: ActorId) -> bool {
        let owner = &self.actors[actor.index()];
        self.phase() == crate::BattlePhase::Combat
            && owner.available()
            && matches!(
                owner.control,
                crate::Control::Manual | crate::Control::SemiAuto
            )
            && self
                .cast_inputs
                .get(usize::from(owner.control_slot))
                .is_some_and(|input| input.guard)
    }

    /// Manual and Semi read Techs; Auto reads the issuing controller’s captured Delay Spell
    /// mapping while its command is pending. Holding the button gates release but does not stop
    /// a positive countdown.
    pub(crate) fn cast_delay_requested(&self, actor: ActorId) -> bool {
        match self.actors[actor.index()].control {
            crate::Control::Manual | crate::Control::SemiAuto => self
                .cast_inputs
                .get(usize::from(self.actors[actor.index()].control_slot))
                .is_some_and(|input| input.technique),
            crate::Control::Auto => self
                .pending_technique_issuer(actor)
                .and_then(|slot| self.cast_inputs.get(usize::from(slot)))
                .is_some_and(|input| input.delay_spell),
            crate::Control::Enemy => false,
        }
    }

    pub(crate) fn cast_command_changed(&self, actor: ActorId, action: crate::ActionKey) -> bool {
        self.pending_technique(actor)
            .is_some_and(|pending| pending != action)
    }

    fn cancel_cast(&mut self, actor: ActorId, cues: &mut Vec<Cue>) {
        self.stop_voice(actor, cues);
        self.clear_cast_memory(actor);
    }
}

const UNISON_CHANT_TICKS: u32 = 30;
const CHARGE_POWER_INTERVAL: u32 = 6;
const CHANT_EFFECT_INTERVAL: u32 = 8;
const CHARGED_EFFECT_INTERVAL: u32 = 32;
const MAX_CHARGED_POWER: u8 = 150;
const NIMBLE_RECOVERY_TICKS: u32 = 30;
pub(crate) const LEARNING_HOLD_TICKS: u16 = 60;

pub(crate) enum Progress {
    Casting,
    Recovery(u32),
    Finished,
}

/// One cast owns its policy until recovery. The released spell is a separate
/// resident, so interrupting its caster never cancels an already released spell.
pub(crate) struct CastRun {
    pub(crate) held: bool,
    pub(crate) remaining: u32,
    rhythm_presses: u8,
    incantation_pending: bool,
    release_pose: Option<u16>,
    release_pending: bool,
    scene_recovery: Option<u32>,
    learning: bool,
    definition: Arc<CastingDefinition>,
}

impl CastRun {
    pub(crate) fn releasing(&self) -> bool {
        self.release_pose.is_some() || self.release_pending || self.scene_recovery.is_some()
    }

    fn chant_pose(&self, battle: &mut Battle, actor: ActorId, age: u32) {
        for &(_, pose) in self
            .definition
            .chant
            .iter()
            .filter(|(at, _)| u32::from(*at) == age)
        {
            pose.play(battle, actor.index());
        }
    }

    fn feedback(&mut self, sequence: &crate::action::Sequence, cues: &mut Vec<Cue>) {
        if self.incantation_pending
            && self
                .definition
                .incantation_lead
                .is_some_and(|lead| self.remaining < lead)
        {
            self.incantation_pending = false;
            cues.push(Cue::Casting {
                actor: sequence.actor,
                action: sequence.action,
                phase: CastPhase::Incantation,
            });
        }
        // Admission owns age zero; periodic artwork begins on the first casting update.
        let elapsed = sequence.age.saturating_sub(1);
        for (interval, phase, enabled) in [
            (CHARGED_EFFECT_INTERVAL, CastPhase::Charged, self.held),
            (CHANT_EFFECT_INTERVAL, CastPhase::Chanting, true),
        ] {
            if enabled && elapsed.is_multiple_of(interval) {
                cues.push(Cue::Casting {
                    actor: sequence.actor,
                    action: sequence.action,
                    phase,
                });
            }
        }
    }

    pub(crate) fn save_progress(&self, actor: &mut crate::Actor, action: crate::ActionKey) {
        if !self.releasing() && !self.learning {
            actor.casting_state.interrupted = Some((action, self.remaining));
        }
    }

    /// Every five physical Attack presses shorten this cast by one update.
    fn rhythm(&mut self, battle: &Battle, actor: ActorId) {
        let owner = &battle.actors[actor.index()];
        if self.remaining > 0
            && self.definition.release.summon.is_none()
            && owner.side == crate::Side::Party
            && owner.equipment.casting.rhythm
            && battle
                .cast_inputs
                .get(usize::from(owner.control_slot))
                .is_some_and(|input| input.attack)
        {
            self.rhythm_presses += 1;
            if self.rhythm_presses == 5 {
                self.rhythm_presses = 0;
                self.remaining -= 1;
            }
        }
    }

    pub(crate) fn new(
        battle: &mut Battle,
        sequence: &crate::action::Sequence,
        definition: Arc<CastingDefinition>,
    ) -> Self {
        if sequence.unison || definition.fixed_timing || definition.release.summon.is_some() {
            let remaining = if sequence.unison {
                UNISON_CHANT_TICKS
            } else {
                definition.duration.max(1)
            };
            return Self {
                held: false,
                remaining,
                incantation_pending: definition
                    .incantation_lead
                    .is_some_and(|lead| remaining >= lead),
                rhythm_presses: 0,
                release_pose: None,
                release_pending: false,
                scene_recovery: None,
                learning: false,
                definition,
            };
        }
        let actor = sequence.actor;
        let owner = &battle.actors[actor.index()];
        let traits = if owner.side == crate::Side::Party {
            owner.equipment.casting
        } else {
            Default::default()
        };
        let memory = owner.casting_state;
        let resumed = memory.interrupted.filter(|&(action, remaining)| {
            traits.spell_save && action == sequence.action && remaining > 0
        });
        let profile_ticks = i64::from(definition.profile_ticks);
        let mut reduction = 0;
        if owner.overlimit.is_active() {
            reduction += profile_ticks.div_euclid(2);
        }
        if traits.speed_cast {
            reduction += profile_ticks.div_euclid(4);
        }
        if traits.angel_song {
            reduction += profile_ticks.div_euclid(2);
        }
        if traits.reprise && memory.previous_spell == Some(sequence.action) {
            reduction += profile_ticks.div_euclid(2);
        }
        let mut duration = i64::from(definition.duration) - reduction;
        if duration != 0
            && owner
                .conditions
                .effective()
                .contains(Condition::CastingSpeed)
        {
            duration -= i64::from(definition.spell_ticks).div_euclid(4);
        }
        let duration = resumed.map_or(
            duration.clamp(0, i64::from(u32::MAX)) as u32,
            |(_, remaining)| remaining,
        );
        let duration =
            crate::tp::initialize_random_clock(owner, resumed.is_some(), duration, || {
                battle.random.next_u16() % 100
            })
            .max(1);
        Self {
            held: false,
            remaining: duration,
            incantation_pending: definition
                .incantation_lead
                .is_some_and(|lead| duration >= lead),
            rhythm_presses: 0,
            release_pose: None,
            release_pending: false,
            scene_recovery: None,
            learning: false,
            definition,
        }
    }

    fn commit(
        &mut self,
        battle: &mut Battle,
        id: crate::ActionId,
        sequence: &mut crate::action::Sequence,
        store: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<Progress> {
        let actor = sequence.actor;
        let requested = sequence.action;
        if self.definition.release.summon.is_some() && sequence.unison {
            battle.cancel_cast(actor, cues);
            cues.push(Cue::Rejected {
                actor,
                reason: crate::Rejection::Unavailable,
            });
            return Ok(Progress::Finished);
        }
        let lucky = if sequence.unison {
            false
        } else {
            let owner = &battle.actors[actor.index()];
            let quote = crate::tp::action_quote(owner, sequence.action, &sequence.definition);
            if u32::from(owner.tp) < quote {
                battle.cancel_cast(actor, cues);
                cues.push(Cue::Rejected {
                    actor,
                    reason: crate::Rejection::InsufficientTp,
                });
                return Ok(Progress::Finished);
            }
            let learned = if store {
                None
            } else {
                battle.commit_cast_use(actor, requested)?
            };
            if let Some(action) = learned {
                let definition = Arc::clone(&battle.prepared.actions[action]);
                let ActionExecution::Casting(cast) = &definition.execution else {
                    anyhow::bail!("learned spell has no prepared cast");
                };
                self.definition = Arc::clone(cast);
                sequence.action = action;
                sequence.definition = definition;
                battle.snapshot_technique_proficiency(actor, action);
                self.learning = true;
                false
            } else {
                let owner = &battle.actors[actor.index()];
                let (amount, lucky) =
                    crate::tp::commit_spell_cost(owner, quote, || battle.random.next_u16() % 100);
                battle.actors[actor.index()].tp -= amount as u16;
                // Storage retains the requested spell and its existing payment order.
                if store {
                    let _ = battle.commit_cast_use(actor, requested)?;
                }
                lucky
            }
        };
        battle.clear_matching_technique_command(actor, requested);
        let owner = &mut battle.actors[actor.index()];
        if lucky && !store {
            cues.push(Cue::ExSkillLabel {
                actor,
                position: owner.condition_label_origin(),
            });
        }
        owner.casting_state.interrupted = None;
        if store {
            battle.store_committed_cast(actor, sequence.action, cues)?;
            return Ok(Progress::Finished);
        }
        if self.learning {
            battle.request_timed_hold(LEARNING_HOLD_TICKS, None);
            return Ok(Progress::Casting);
        }
        self.begin_release(battle, id, sequence, cues)
    }

    fn release_gesture(&self, battle: &Battle, actor: ActorId, unison: bool) -> CastReleasePose {
        let owner = &battle.actors[actor.index()];
        if matches!(
            self.definition.release_pose,
            CastReleasePose::Anticipated { .. }
        ) && owner.side == crate::Side::Party
            && (unison || owner.control != crate::Control::Auto)
        {
            CastReleasePose::Ordinary
        } else {
            self.definition.release_pose
        }
    }

    fn begin_release(
        &mut self,
        battle: &mut Battle,
        id: crate::ActionId,
        sequence: &mut crate::action::Sequence,
        cues: &mut Vec<Cue>,
    ) -> Result<Progress> {
        let actor = sequence.actor;
        match self.release_gesture(battle, actor, sequence.unison) {
            CastReleasePose::Anticipated {
                release,
                release_loop_start,
            } => {
                // Stored scenes register on the payment update and own their
                // transition; resident effects wait for the gesture to finish.
                if self.definition.release.transition_ticks == 0 {
                    if battle.actors[actor.index()].side == crate::Side::Enemy
                        && let Some(pose) = release
                    {
                        // Enemies finish chanting before beginning the release gesture.
                        battle.request_pose(
                            actor,
                            Some(pose.motion),
                            crate::Pose {
                                frame: 0.,
                                blend: crate::Pose::default().blend,
                                loop_start: release_loop_start,
                                ..pose.playback()
                            },
                        );
                    }
                    self.held = false;
                    self.release_pending = true;
                    return Ok(Progress::Casting);
                }
            }
            CastReleasePose::Ordinary => {
                if self.definition.release.summon.is_none() {
                    battle.model_requests.push(crate::ModelRequest::Common {
                        actor,
                        pose: crate::CommonPose::Cast,
                    });
                }
            }
            CastReleasePose::Scene {
                opening,
                release,
                release_loop_start,
                at,
            } => {
                ensure!(
                    at <= self.definition.release.transition_ticks,
                    "cast handoff must lie within the spell transition"
                );
                if let Some(pose) = if at == 0 { release } else { opening } {
                    let mut playback = pose.playback();
                    if at == 0 {
                        playback.loop_start = release_loop_start;
                    }
                    battle.request_pose(actor, Some(pose.motion), playback);
                }
            }
        }
        // Resident spells wait for the release pose; stored scenes own a transition.
        if battle.actors[actor.index()].side == crate::Side::Party
            && self.definition.release.transition_ticks == 0
            && self.definition.release.summon.is_none()
        {
            self.release_pose = battle
                .models
                .as_ref()
                .and_then(|models| models.definition(actor))
                .and_then(|model| model.reactions.cast);
        }
        self.held = false;
        if self.release_pose.is_some() {
            Ok(Progress::Casting)
        } else {
            self.release(battle, id, sequence, cues)
        }
    }

    fn release(
        &mut self,
        battle: &mut Battle,
        id: crate::ActionId,
        sequence: &mut crate::action::Sequence,
        cues: &mut Vec<Cue>,
    ) -> Result<Progress> {
        let actor = sequence.actor;
        sequence.target = battle.spell_target(actor, &self.definition.release);
        battle.dispatch_spell(
            actor,
            sequence.action,
            crate::SpellSlot::Primary,
            Some(id),
            cues,
        )?;
        if let Some(threat) = self.definition.threat {
            battle.publish_special_guard_threat(actor, sequence.target, threat);
        }
        let owner = &mut battle.actors[actor.index()];
        owner.casting_state.previous_spell = Some(sequence.action);
        if self.definition.release.summon.is_some() {
            // The cinematic completes this action at its restore boundary.
            return Ok(Progress::Casting);
        }
        let recovery = if owner.side == crate::Side::Party && owner.equipment.casting.nimble {
            NIMBLE_RECOVERY_TICKS
        } else {
            self.definition.recovery
        };
        if self.definition.release.transition_ticks > 0 {
            self.scene_recovery = Some(recovery);
            Ok(Progress::Casting)
        } else {
            Ok(Progress::Recovery(recovery))
        }
    }

    pub(crate) fn step(
        &mut self,
        battle: &mut Battle,
        id: crate::ActionId,
        sequence: &mut crate::action::Sequence,
        cues: &mut Vec<Cue>,
    ) -> Result<Progress> {
        let actor = sequence.actor;
        let index = actor.index();

        let occupied = battle.spell_active(actor, crate::SpellSlot::Primary);
        if battle.phase() != crate::BattlePhase::Combat {
            battle.cancel_cast(actor, cues);
            return Ok(Progress::Finished);
        }
        // A long release pose can outlast scene framing. Finish its first
        // cycle before recovery, without waiting for the independent spell.
        if let Some(recovery) = self.scene_recovery {
            if matches!(
                self.definition.release_pose,
                CastReleasePose::Scene {
                    release: Some(_),
                    ..
                }
            ) && battle.models.as_ref().is_some_and(|models| {
                !models.motion_finished_this_update(actor, battle.actors[index].hit_stop == 0)
            }) {
                return Ok(Progress::Casting);
            }
            self.scene_recovery = None;
            if matches!(self.definition.release_pose, CastReleasePose::Scene { .. }) {
                cues.push(Cue::Casting {
                    actor,
                    action: sequence.action,
                    phase: crate::CastPhase::Released,
                });
            }
            return Ok(Progress::Recovery(recovery));
        }
        if self.learning {
            self.learning = false;
            return self.begin_release(battle, id, sequence, cues);
        }
        if self.release_pending {
            if matches!(
                self.release_gesture(battle, actor, sequence.unison),
                CastReleasePose::Anticipated {
                    release: Some(_),
                    ..
                }
            ) && battle.models.as_ref().is_some_and(|models| {
                !models.motion_finished_this_update(actor, battle.actors[index].hit_stop == 0)
            }) {
                return Ok(Progress::Casting);
            }
            self.release_pending = false;
            return self.release(battle, id, sequence, cues);
        }
        if let Some(clip) = self.release_pose {
            let complete = battle
                .models
                .as_ref()
                .and_then(|models| models.main_motion_observation(actor))
                .is_some_and(|motion| {
                    motion.clip == clip && !motion.repeat && motion.frame >= motion.end
                });
            return if complete && !occupied {
                self.release(battle, id, sequence, cues)
            } else {
                Ok(Progress::Casting)
            };
        }
        self.chant_pose(battle, actor, sequence.age);
        if let CastReleasePose::Anticipated {
            release: Some(pose),
            release_loop_start,
        } = self.release_gesture(battle, actor, sequence.unison)
            && (battle.actors[index].side == crate::Side::Party
                || self.definition.release.transition_ticks > 0)
            && let Some(duration) = battle
                .models
                .as_ref()
                .and_then(|models| models.motion_duration(actor, pose.motion.clip))
        {
            let lead = (duration / pose.rate) as u32 + u32::from(pose.blend);
            if self.remaining > 0 && self.remaining <= lead {
                battle.request_pose(
                    actor,
                    Some(pose.motion),
                    crate::Pose {
                        restart: false,
                        loop_start: release_loop_start,
                        ..pose.playback()
                    },
                );
            }
        }
        if sequence.unison {
            if self.remaining == 0 && !occupied {
                return self.commit(battle, id, sequence, false, cues);
            }
            if !occupied {
                self.remaining = self.remaining.saturating_sub(1);
            }
            self.feedback(sequence, cues);
            return Ok(Progress::Casting);
        }
        let remaining = self.remaining;
        let guard = battle.cast_guard_requested(actor);
        let delay = battle.cast_delay_requested(actor);
        let was_held = self.held;
        self.held = remaining == 0 && (delay || guard && was_held);
        if remaining == 0 && battle.store_cast_requested(actor, sequence.action)? {
            return self.commit(battle, id, sequence, true, cues);
        }
        if remaining == 0 && !self.held && !occupied {
            return self.commit(battle, id, sequence, false, cues);
        }
        if remaining > 0
            && (guard
                || battle.has_pending_item(actor)
                || battle.cast_command_changed(actor, sequence.action)
                || battle.special_guard_threat_pending(actor))
        {
            battle.cancel_cast(actor, cues);
            return Ok(Progress::Finished);
        }
        self.rhythm(battle, actor);
        if !occupied {
            self.remaining = self.remaining.saturating_sub(1);
        }
        if self.held
            && sequence.age.is_multiple_of(CHARGE_POWER_INTERVAL)
            && battle.actors[index].casting_state.power_percent < MAX_CHARGED_POWER
        {
            battle.actors[index].casting_state.power_percent += 1;
        }
        self.feedback(sequence, cues);
        Ok(Progress::Casting)
    }
}

#[cfg(test)]
mod tests;
