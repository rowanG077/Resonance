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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastPhase {
    /// One-time audio at admission; artwork follows the live cast cadence.
    Started,
    Chanting,
    Charged,
    Stored,
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
            let ActionExecution::Casting(prepared) = &sequence.definition.execution else {
                unreachable!("native cast has a casting definition")
            };
            let actor = sequence.actor;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastingState {
    /// Live spell strength, shared by the caster's already released contacts.
    pub power_percent: u8,
    pub(crate) previous_spell: Option<crate::ActionKey>,
    /// Prepared action and remaining ticks retained for Spell Save.
    pub(crate) interrupted: Option<(crate::ActionKey, u32)>,
}

impl Default for CastingState {
    fn default() -> Self {
        Self {
            power_percent: 100,
            previous_spell: None,
            interrupted: None,
        }
    }
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
    pub duration: u32,
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
    /// Hold an opening during the scene transition, then sustain the cast gesture.
    SustainAfter {
        opening: crate::MotionBinding,
        hold_ticks: u16,
    },
}

impl Battle {
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
                    || matches!(cast.release_pose, CastReleasePose::SustainAfter { opening, .. }
                        if opening.model == model.resource && opening.clip == pose.clip))
        })
    }

    pub(crate) fn cast_learning_actor(&self) -> Option<ActorId> {
        self.sequences().find_map(|(_, sequence)| {
            matches!(
                &sequence.execution,
                crate::action::Execution::Casting(CastingRun::Running(cast)) if cast.learning
            )
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
const LEARNING_HOLD_TICKS: u16 = 60;

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
    release_pose: Option<u16>,
    learning: bool,
    definition: Arc<CastingDefinition>,
}

impl CastRun {
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

    fn feedback(&self, sequence: &crate::action::Sequence, cues: &mut Vec<Cue>) {
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
        if self.release_pose.is_none() && !self.learning {
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
        if sequence.unison || definition.release.summon.is_some() {
            return Self {
                held: false,
                remaining: if sequence.unison {
                    UNISON_CHANT_TICKS
                } else {
                    definition.duration.max(1)
                },
                rhythm_presses: 0,
                release_pose: None,
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
        let base = u64::from(definition.duration);
        let mut reduction = 0;
        if owner.overlimit.is_active() {
            reduction += base / 2;
        }
        if traits.speed_cast {
            reduction += base / 4;
        }
        if traits.angel_song {
            reduction += base / 2;
        }
        if traits.reprise && memory.previous_spell == Some(sequence.action) {
            reduction += base / 2;
        }
        if owner
            .conditions
            .effective()
            .contains(Condition::CastingSpeed)
        {
            reduction += base / 4;
        }
        let duration = resumed.map_or(base.saturating_sub(reduction) as u32, |(_, remaining)| {
            remaining
        });
        let duration =
            crate::tp::initialize_random_clock(owner, resumed.is_some(), duration, || {
                battle.random.next_u16() % 100
            })
            .max(1);
        Self {
            held: false,
            remaining: duration,
            rhythm_presses: 0,
            release_pose: None,
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
                position: std::array::from_fn(|axis| {
                    owner.position[axis] + owner.body.center_offset[axis] * 2.
                }),
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

    fn begin_release(
        &mut self,
        battle: &mut Battle,
        id: crate::ActionId,
        sequence: &mut crate::action::Sequence,
        cues: &mut Vec<Cue>,
    ) -> Result<Progress> {
        let actor = sequence.actor;
        match self.definition.release_pose {
            CastReleasePose::Ordinary => {
                battle.model_requests.push(crate::ModelRequest::Common {
                    actor,
                    pose: crate::CommonPose::Cast,
                });
            }
            CastReleasePose::SustainAfter {
                opening,
                hold_ticks,
            } => {
                ensure!(
                    hold_ticks > 0 && hold_ticks <= self.definition.release.transition_ticks,
                    "cast opening must end within the spell transition"
                );
                battle.request_pose(
                    actor,
                    Some(opening),
                    crate::Pose {
                        rate: 0.,
                        repeat: true,
                        blend: 0,
                        ..Default::default()
                    },
                );
            }
        }
        // Zero-HP revival dispatches while its caster begins the release pose.
        // Other resident spells wait for that pose; stored scenes own a transition.
        if battle.actors[actor.index()].side == crate::Side::Party
            && self.definition.release.transition_ticks == 0
            && self.definition.release.summon.is_none()
            && !self
                .definition
                .release
                .recovery
                .iter()
                .any(|pulse| matches!(pulse.benefit, crate::SpellBenefit::Resurrect(0)))
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
        &self,
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
        Ok(Progress::Recovery(recovery))
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
        if self.learning {
            self.learning = false;
            return self.begin_release(battle, id, sequence, cues);
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
