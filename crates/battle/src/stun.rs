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
    draw: impl FnOnce(u16) -> anyhow::Result<u16>,
) -> anyhow::Result<bool> {
    if target.hp == 0
        || hit.suppresses_reaction()
        || target.time_stop != 0
        || matches!(hit.guard, crate::GuardResult::Blocked { .. })
    {
        return Ok(false);
    }
    let chance = chance_percent(owner, target, chance);
    Ok(draw(chance)? % 100 < chance && !target.conditions.immunity().contains(Condition::Stun))
}

fn chance_percent(owner: &Actor, target: &Actor, base: u8) -> u16 {
    let adjusted = (i16::from(base) + i16::from(owner.reaction.stun.chance_bonus)
        - i16::from(target.reaction.stun.resistance))
    .max(0) as u16;
    adjusted + if owner.equipment.stun_ex_bonus { 5 } else { 0 }
}

impl Battle {
    pub(crate) fn enter_action_stun(&mut self, id: ActorId, cues: &mut Vec<Cue>) {
        self.enter_stun(id, cues);
        // The command's entry visit precedes the ordinary stun countdown.
        if let ActorTask::Stunned { remaining, .. } = self.runtime[id.index()].task() {
            self.set_task(
                id.index(),
                ActorTask::Stunned {
                    remaining: remaining + 1,
                    struggle_ticks: 0,
                },
            );
        }
    }

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
        self.set_task(
            index,
            ActorTask::Stunned {
                remaining,
                struggle_ticks: 0,
            },
        );
    }

    pub(crate) fn advance_stun(
        &mut self,
        id: ActorId,
        remaining: u32,
        struggle_ticks: u8,
        struggle: bool,
        cues: &mut Vec<Cue>,
    ) -> bool {
        let index = id.index();
        let actor = &mut self.actors[index];
        let grounded = !actor.airborne();
        if grounded {
            self.model_requests.push(crate::ModelRequest::Common {
                actor: id,
                pose: crate::CommonPose::Stunned,
            });
        }
        // A fresh direction accelerates this update and the next one.
        let struggle_ticks =
            if grounded && struggle && matches!(actor.control, Control::Manual | Control::SemiAuto)
            {
                2
            } else {
                struggle_ticks.saturating_sub(1)
            };
        let elapsed = u32::from(grounded) + u32::from(struggle_ticks != 0);
        let remaining = remaining.saturating_sub(elapsed);
        self.set_task(
            index,
            ActorTask::Stunned {
                remaining,
                struggle_ticks,
            },
        );
        if remaining == 0 {
            self.interrupt_actor(id, cues);
            self.set_task(
                index,
                ActorTask::Recovering {
                    remaining: 0,
                    after_hit: true,
                    idle_pose: None,
                },
            );
            // Clear the birds now; commands and the idle pose resume next update.
            if let Some(models) = &mut self.models {
                models.preserve_pose_through(id, self.update + 1);
            }
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
