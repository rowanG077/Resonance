use crate::conditions::Condition;
use crate::state::ActorTask;
use crate::{Actor, ActorId, Battle, Control, Cue, HitResult};

const DURATION: u32 = 180;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stun {
    pub chance_bonus: u8,
    pub resistance: u8,
}

pub(crate) fn roll(
    owner: &Actor,
    target: &Actor,
    chance: u8,
    hit: HitResult,
    random: &mut crate::state::Random,
) -> bool {
    if target.hp == 0
        || hit.suppresses_reaction()
        || target.time_stop != 0
        || matches!(hit.guard, crate::GuardResult::Blocked { .. })
    {
        return false;
    }
    random.next_u16() % 100 < chance_percent(owner, target, chance)
        && !target.conditions.immunity().contains(Condition::Stun)
}

fn chance_percent(owner: &Actor, target: &Actor, base: u8) -> u16 {
    let adjusted = (i16::from(base) + i16::from(owner.reaction.stun.chance_bonus)
        - i16::from(target.reaction.stun.resistance))
    .max(0) as u16;
    adjusted + if owner.equipment.stun_ex_bonus { 5 } else { 0 }
}

impl Battle {
    pub(crate) fn enter_stun(&mut self, id: ActorId, cues: &mut Vec<Cue>) {
        let index = id.index();
        self.interrupt_actor(id, cues);
        let actor = &mut self.actors[index];

        actor.reaction.protection = Default::default();
        actor.reaction.armor.threshold = 0;
        let remaining = if actor.conditions.immunity().contains(Condition::ShortStun) {
            DURATION / 2
        } else {
            DURATION
        };
        self.set_task(index, ActorTask::Stunned { remaining });
    }

    pub(crate) fn advance_stun(
        &mut self,
        id: ActorId,
        remaining: u32,
        struggle: bool,
        cues: &mut Vec<Cue>,
    ) -> bool {
        let index = id.index();
        let actor = &mut self.actors[index];
        const STRUGGLE_REDUCTION: u32 = 2;
        let grounded = !actor.airborne();
        let struggle = struggle && matches!(actor.control, Control::Manual | Control::SemiAuto);
        let elapsed = u32::from(grounded) + if struggle { STRUGGLE_REDUCTION } else { 0 };
        let remaining = remaining.saturating_sub(elapsed);
        self.set_task(index, ActorTask::Stunned { remaining });
        if remaining == 0 {
            self.interrupt_actor(id, cues);
            self.enter_idle(id);
        }
        let actor = &mut self.actors[index];
        actor
            .movement
            .integrate_along(&mut actor.position, actor.reaction.direction);
        actor
            .movement
            .brake(actor.position[1], crate::Activity::Stunned);
        crate::movement::floor(actor);
        true
    }
}

#[cfg(test)]
mod tests;
