//! Casting progress, payment, storage and independent spell release.
use crate::conditions::Condition;
use crate::{ActionExecution, ActorId, Battle, Cue};
use anyhow::Result;
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
            battle.model_requests.push(crate::ModelRequest::Common {
                actor,
                pose: crate::CommonPose::Chant,
            });
            cues.push(Cue::Casting {
                actor,
                action: sequence.action,
                phase: CastPhase::Chanting,
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
    pub duration: u32,
    pub recovery: u32,
    pub release: Arc<crate::PreparedVolley>,
    pub threat: Option<CastingThreat>,
}

impl Battle {
    pub(crate) fn clear_cast_memory(&mut self, actor: ActorId) {
        self.actors[actor.index()].casting_state = Default::default();
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

const CHARGE_POWER_INTERVAL: u32 = 6;
const MAX_CHARGED_POWER: u16 = 150;
const NIMBLE_RECOVERY_TICKS: u32 = 30;

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
    definition: Arc<CastingDefinition>,
}

impl CastRun {
    pub(crate) fn save_progress(&self, actor: &mut crate::Actor, action: crate::ActionKey) {
        actor.casting_state.interrupted = Some((action, self.remaining));
    }

    /// Every five physical Attack presses shorten this cast by one update.
    fn rhythm(&mut self, battle: &Battle, actor: ActorId) {
        let owner = &battle.actors[actor.index()];
        if self.remaining > 0
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
        let (amount, lucky) =
            crate::tp::commit_spell_cost(owner, quote, || battle.random.next_u16() % 100);
        battle.actors[actor.index()].tp -= amount as u16;
        battle.clear_matching_technique_command(actor, sequence.action);
        battle.commit_cast_use(actor, sequence.action)?;
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
        self.release(battle, id, sequence, cues)
    }

    fn release(
        &self,
        battle: &mut Battle,
        id: crate::ActionId,
        sequence: &mut crate::action::Sequence,
        cues: &mut Vec<Cue>,
    ) -> Result<Progress> {
        let actor = sequence.actor;
        sequence.target = battle.runtime[actor.index()].target;
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
        let recovery = if owner.side == crate::Side::Party && owner.equipment.casting.nimble {
            NIMBLE_RECOVERY_TICKS
        } else {
            self.definition.recovery
        };
        battle.model_requests.push(crate::ModelRequest::Common {
            actor,
            pose: crate::CommonPose::Cast,
        });
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
        let remaining = self.remaining;
        let guard = battle.cast_guard_requested(actor);
        let delay = battle.cast_delay_requested(actor);
        let was_held = self.held;
        self.held = remaining == 0 && (delay || guard && was_held);
        if self.held && !was_held {
            cues.push(Cue::Casting {
                actor,
                action: sequence.action,
                phase: CastPhase::Charged,
            });
        }
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
            && battle.actors[index].attack_power < MAX_CHARGED_POWER
        {
            battle.actors[index].attack_power += 1;
        }
        Ok(Progress::Casting)
    }
}

#[cfg(test)]
mod tests;
