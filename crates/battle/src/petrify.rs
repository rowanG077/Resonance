use crate::{Activity, ActorAvailability, ActorId, Battle, Cue, conditions::Cure};

impl Battle {
    pub(crate) fn enter_petrified(&mut self, target: ActorId, cues: &mut Vec<Cue>) {
        self.release_capture_links(target);
        self.cancel_pending_item(target);
        self.clear_technique_command(target);
        self.interrupt_actor(target, cues);
        let actor = &mut self.actors[target.index()];
        if actor.conditions.retain_petrified() {
            actor.elements.enchantment = None;
        }
        actor.availability = ActorAvailability::Petrified;
        actor.guard.active = false;
        actor.guard.recovery = 0;
        actor.reaction.combo_hits = 0;
        actor.reaction.combo_damage = 0;
    }

    pub(crate) fn apply_cure(&mut self, target: ActorId, cure: Cure, cues: &mut Vec<Cue>) {
        if self.actors[target.index()].is_petrified() && cure.thaws_petrify() {
            self.interrupt_actor(target, cues);
            let actor = &mut self.actors[target.index()];
            actor.availability = ActorAvailability::Active;
        }
        let actor = &mut self.actors[target.index()];
        if actor.conditions.cure(cure) {
            actor.elements.enchantment = None;
        }
    }

    pub(crate) fn advance_petrified(&mut self, index: usize) {
        let actor = &mut self.actors[index];
        actor
            .movement
            .integrate_along(&mut actor.position, actor.reaction.direction);
        actor.movement.brake(actor.position[1], Activity::Idle);
    }
}
