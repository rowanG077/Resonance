use crate::{Activity, ActorAvailability, ActorId, Battle, Cue, conditions::Cure};

impl Battle {
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
