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
        actor
            .movement
            .integrate_along(&mut actor.position, actor.reaction.direction);
        actor
            .movement
            .brake(actor.position[1], crate::Activity::Defeated);
    }

    pub(crate) fn enter_death(&mut self, id: ActorId, cues: &mut Vec<Cue>) {
        if self.actors[id.index()].availability == ActorAvailability::Dead {
            return;
        }
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
        cues.push(Cue::Defeated { actor: id });
        actor.time_stop = 0;
        actor.guard.active = false;
        actor.reaction.protection = Default::default();
        actor.overlimit = crate::OverLimit::default();
        self.ledger.record_combo_damage(actor.reaction.combo_damage);
        actor.reaction.combo_hits = 0;
        actor.reaction.combo_damage = 0;
    }

    pub(crate) fn enter_revival(&mut self, id: ActorId, cues: &mut Vec<Cue>) {
        self.interrupt_actor(id, cues);
        let actor = &mut self.actors[id.index()];
        actor.reaction.protection.recover();
        actor.reaction.recoil = Default::default();
        actor.availability = ActorAvailability::Active;
        self.enter_get_up(id);
    }
}

#[cfg(test)]
mod tests;
