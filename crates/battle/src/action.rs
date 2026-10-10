//! Runtime ownership for actor attacks, casting and recovery.
use crate::{ActionDefinition, ActionExecution, ActionId, ActorId, Battle, Cue};
use anyhow::Result;
use std::sync::Arc;

pub(crate) enum Execution {
    Attack(crate::attack::AttackRun),
    Casting(crate::casting::CastingRun),
    Recovering {
        remaining: u32,
    },
    /// Events are complete or cancelled; the recovery transition is pending.
    Complete,
    /// The action has no remaining work.
    Finished,
}

pub(crate) struct Sequence {
    pub action: crate::ActionKey,
    pub definition: Arc<ActionDefinition>,
    pub actor: ActorId,
    pub target: ActorId,
    pub age: u32,
    pub combo: Option<crate::control::ComboWindow>,
    pub started_at: u64,
    pub normal: Option<crate::control::NormalState>,
    pub collision_bypass: bool,
    pub melee: Option<crate::melee::Window>,
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
            collision_bypass: false,
            melee: None,
            execution,
        }
    }

    pub(crate) fn activity(&self) -> crate::Activity {
        match &self.execution {
            Execution::Complete | Execution::Recovering { .. } => crate::Activity::Recovering,
            Execution::Casting(run) => crate::Activity::Casting {
                held: matches!(run, crate::casting::CastingRun::Running(cast) if cast.held),
            },
            _ => crate::Activity::Action,
        }
    }

    /// Discard future attack work without retracting already submitted contacts.
    pub(crate) fn cancel_attack_events(&mut self) {
        self.collision_bypass = false;
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
        battle.discharge_stored_spell(id, self.actor, cues)
    }
}

pub(crate) fn step_sequence(
    battle: &mut Battle,
    id: ActionId,
    sequence: &mut Sequence,
    contacts: &mut crate::contact::Contacts,
    cues: &mut Vec<Cue>,
) -> Result<Option<u32>> {
    match std::mem::replace(&mut sequence.execution, Execution::Complete) {
        Execution::Attack(run) => crate::attack::step(battle, id, sequence, run, contacts, cues),
        Execution::Casting(run) => crate::casting::step(battle, id, sequence, run, cues),
        execution => {
            sequence.execution = execution;
            Ok(None)
        }
    }
}
