//! Runtime ownership for actor attacks, casting and recovery.
use crate::{ActionDefinition, ActionExecution, ActionId, ActorId, Battle, Cue};
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) enum Execution {
    Attack(crate::attack::AttackRun),
    Casting(crate::casting::CastingRun),
    LeavingArena,
    Recovering {
        remaining: u32,
    },
    /// Events are complete or cancelled; the recovery transition is pending.
    Complete,
    /// The action has no remaining work.
    Finished,
}

#[derive(Clone)]
pub(crate) struct Sequence {
    pub action: crate::ActionKey,
    pub definition: Arc<ActionDefinition>,
    pub actor: ActorId,
    pub target: ActorId,
    pub age: u32,
    pub combo: Option<crate::control::ComboWindow>,
    pub started_at: u64,
    pub normal: Option<crate::control::NormalState>,
    pub melee: Option<crate::melee::Window>,
    pub contact_target: Option<ActorId>,
    /// A new primary pose clears its contact latch when blending finishes.
    pub contact_blend_ends: Option<u32>,
    /// Any accepted contact, including guard, owned by this action.
    pub contact_made: bool,
    /// Selected Unison artes keep ordinary action playback, with scene-owned admission.
    pub unison: bool,
    /// The first use waits for its acquisition presentation before starting.
    pub learning: bool,
    pub struck_target: Option<ActorId>,
    pub struck_damage: i32,
    pub cyclone_target: Option<(ActorId, [f32; 3])>,
    pub(crate) execution: Execution,
}
impl Sequence {
    pub(crate) fn events_complete(&self) -> bool {
        matches!(self.execution, Execution::Complete | Execution::Finished)
    }

    pub(crate) fn finished(&self) -> bool {
        matches!(self.execution, Execution::Finished)
    }

    pub(crate) fn running(&self) -> bool {
        !matches!(
            self.execution,
            Execution::Recovering { .. } | Execution::Finished
        )
    }
    pub fn new(
        key: crate::ActionKey,
        action: &Arc<ActionDefinition>,
        actor: ActorId,
        target: ActorId,
        started_at: u64,
    ) -> Self {
        let execution = match &action.execution {
            ActionExecution::Casting(_) => Execution::Casting(crate::casting::CastingRun::Starting),
            ActionExecution::Attack(_) => Execution::Attack(crate::attack::AttackRun::Starting),
            ActionExecution::LeaveArena => Execution::LeavingArena,
        };
        Self {
            action: key,
            definition: Arc::clone(action),
            actor,
            target,
            age: 0,
            combo: match &action.execution {
                ActionExecution::Attack(attack) => {
                    attack.chain_at.map(|at| crate::control::ComboWindow {
                        opens_at: u32::from(at),
                        buffer_until: u32::from(attack.end_at),
                    })
                }
                _ => None,
            },
            started_at,
            normal: None,
            melee: None,
            contact_target: None,
            contact_blend_ends: None,
            contact_made: false,
            unison: false,
            learning: false,
            struck_target: None,
            struck_damage: 0,
            cyclone_target: None,
            execution,
        }
    }

    pub(crate) fn activity(&self) -> crate::Activity {
        match &self.execution {
            Execution::LeavingArena => crate::Activity::Escaping,
            Execution::Complete | Execution::Recovering { .. } => crate::Activity::Recovering,
            Execution::Casting(run) => crate::Activity::Casting {
                held: matches!(run, crate::casting::CastingRun::Running(cast) if cast.held),
            },
            _ => crate::Activity::Action,
        }
    }

    /// Discard future attack work without retracting already submitted contacts.
    pub(crate) fn cancel_attack_events(&mut self) {
        self.execution = Execution::Complete;
        self.melee = None;
    }

    pub(crate) fn recover(
        &mut self,
        battle: &mut Battle,
        id: ActionId,
        remaining: u32,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if !self.running() {
            return Ok(());
        }
        self.cancel_attack_events();
        self.execution = Execution::Recovering { remaining };
        cues.push(Cue::RetireActionEffects(id));
        battle.clear_special_guard(self.actor);
        battle.clear_special_guard_pending(self.actor);
        let actor = &mut battle.actors[self.actor.index()];
        actor.movement.gravity = if actor.movement.flying {
            0.
        } else {
            crate::movement::GRAVITY
        };
        if actor.side == crate::Side::Enemy {
            actor.reaction.armor.threshold = 0;
            actor.reaction.armor.received = 0;
        }
        if self.unison {
            battle.restore_combination_weapons(id, self.actor);
            Ok(())
        } else {
            battle.discharge_stored_spell(id, self.actor, cues)
        }
    }
}

pub(crate) fn step_sequence(
    battle: &mut Battle,
    id: ActionId,
    sequence: &mut Sequence,
    contacts: &mut crate::contact::Contacts,
    items: &mut dyn crate::item::Provider,
    cues: &mut Vec<Cue>,
) -> Result<Option<u32>> {
    match std::mem::replace(&mut sequence.execution, Execution::Complete) {
        Execution::Attack(run) => {
            crate::attack::step(battle, id, sequence, run, contacts, items, cues)
        }
        Execution::Casting(run) => crate::casting::step(battle, id, sequence, run, cues),
        execution => {
            sequence.execution = execution;
            Ok(None)
        }
    }
}
