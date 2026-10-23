//! Death and revival change availability independently of their visual feedback.
use crate::{Actor, ActorId, Battle, Cue, Side};
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActorAvailability {
    Absent,
    Petrified,
    Dead,
    #[default]
    Active,
}

impl Actor {
    pub fn available(&self) -> bool {
        self.availability == ActorAvailability::Active
    }

    pub fn is_petrified(&self) -> bool {
        self.availability == ActorAvailability::Petrified
    }
}

impl Battle {
    /// Result performance requires active Over Limit; a merely full gauge is insufficient.
    pub fn end_overlimit(&mut self, id: ActorId) -> Result<()> {
        ensure!(!self.ended, "Over Limit change after battle completion");
        self.actor(id)?;
        let actor = &mut self.actors[id.index()];
        if actor.overlimit.is_active() {
            actor.overlimit = crate::OverLimit::default();
        }
        Ok(())
    }

    fn react_to_ally_death(&mut self, victim: ActorId) {
        for index in 0..self.actors.len() {
            let actor = &self.actors[index];
            if index != victim.index() && actor.side == Side::Party && actor.available() {
                self.gain_overlimit(ActorId(index as u8), 10);
            }
        }
    }

    pub(crate) fn advance_dead(&mut self, index: usize) {
        let actor = &mut self.actors[index];
        if !self.prepared.actor_setup[index].stationary_defeat {
            // Defeat retains the motion established by the last action or recoil.
            // In particular, scripted airborne defeat can keep zero gravity.
            actor
                .movement
                .integrate_along(&mut actor.position, actor.reaction.direction);
        }
        actor
            .movement
            .brake(actor.position[1], crate::Activity::Defeated);
    }

    pub(crate) fn enter_death(&mut self, id: ActorId, cues: &mut Vec<Cue>) {
        if self.defeat_actor(id, cues) {
            cues.push(Cue::Defeated { actor: id });
        }
    }

    /// Scripted deaths supply their own feedback through the action timeline.
    pub(crate) fn defeat_actor(&mut self, id: ActorId, cues: &mut Vec<Cue>) -> bool {
        if self.actors[id.index()].availability == ActorAvailability::Dead {
            return false;
        }
        self.release_capture_links(id);
        self.cancel_pending_item(id);
        self.clear_technique_command(id);
        self.interrupt_actor(id, cues);
        if self.actors[id.index()].side == Side::Party {
            let first = self.actors.iter().position(|a| a.side == Side::Party) == Some(id.index());
            self.ledger.death(id, first);
            self.react_to_ally_death(id);
        }
        let actor = &mut self.actors[id.index()];
        if actor.conditions.clear_base() {
            actor.elements.enchantment = None;
        }
        actor.availability = ActorAvailability::Dead;
        actor.time_stop = 0;
        actor.guard.active = false;
        actor.reaction.protection = Default::default();
        actor.overlimit = crate::OverLimit::default();
        self.ledger.record_combo_damage(actor.reaction.combo_damage);
        actor.reaction.combo_hits = 0;
        actor.reaction.combo_damage = 0;
        true
    }

    pub(crate) fn enter_revival(&mut self, id: ActorId, cues: &mut Vec<Cue>) {
        self.interrupt_actor(id, cues);
        let actor = &mut self.actors[id.index()];
        actor.reaction.protection.recover();
        actor.reaction.recoil = Default::default();
        actor.availability = ActorAvailability::Active;
        if let Some((pose, ticks)) = self.prepared.actor_setup[id.index()].revival {
            pose.play(self, id.index());
            self.set_task(
                id.index(),
                crate::state::ActorTask::Recovering {
                    remaining: ticks.max(crate::knockdown::GET_UP_DURATION) - 1,
                    after_hit: false,
                    idle_pose: None,
                },
            );
        } else {
            self.enter_get_up(id);
        }
    }
}

#[cfg(test)]
mod tests;
