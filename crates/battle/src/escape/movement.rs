use crate::{ActorId, Battle, BattlePhase, BattleResult, Cue, Locomotion, Side, state::ActorTask};
use anyhow::Result;

impl Battle {
    pub(crate) fn escape_ending_phase(&self) -> bool {
        self.phase() == BattlePhase::Ending && self.terminal.result == Some(BattleResult::Escaped)
    }

    /// Departure replaces combat activity and owns movement until the battle ends.
    pub(crate) fn advance_escape_actor(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let index = actor.index();
        if !self.escape_ending_phase()
            || self.actors[index].side != Side::Party
            || !self.actors[index].available()
        {
            return Ok(false);
        }
        let Some(definition) = &self.prepared.actor_setup[index].control else {
            return Ok(false);
        };
        let (run_speed, turn_ticks, motion) = (
            definition.run_speed,
            definition.turn_ticks,
            definition.motions.map(|motions| motions.run),
        );
        if !matches!(self.runtime[index].task(), ActorTask::Escaping) {
            let owner = &self.actors[index];
            let target = self.target(actor).map_or(owner.position, |target| {
                self.actors[target.index()].position
            });
            let mut direction = crate::distance::normalize([
                owner.position[0] - target[0],
                0.,
                owner.position[2] - target[2],
            ]);
            if direction == [0.; 3] {
                direction = crate::distance::normalize([
                    -owner.facing_direction[0],
                    0.,
                    -owner.facing_direction[2],
                ]);
                if direction == [0.; 3] {
                    direction = [1., 0., 0.];
                }
            }
            self.cancel_pending_item(actor);
            self.clear_technique_command(actor);
            self.interrupt_actor(actor, cues);
            self.enter_idle(actor);
            self.set_task(index, ActorTask::Escaping);
            let owner = &mut self.actors[index];
            owner.movement.locomotion = Locomotion::Run;
            owner.movement.direction = direction;
        }
        let owner = &mut self.actors[index];
        let conditions = owner.conditions.effective();
        let speed = owner.run_limit(run_speed, conditions);
        let rate = owner.motion_rate(0.5, conditions);
        let direction = owner.movement.direction;
        crate::control::face(owner, direction, 180. / f32::from(turn_ticks));
        owner.movement.forward =
            (owner.movement.forward + crate::movement::RUN_ACCELERATION).min(speed);
        owner.movement.integrate(&mut owner.position);
        self.advance_hover(index, true)?;

        self.request_pose(
            actor,
            motion,
            crate::Pose {
                rate,
                repeat: true,
                blend: 0,
                restart: false,
                ..Default::default()
            },
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
