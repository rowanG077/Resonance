use crate::{ActorId, Battle, BattlePhase, BattleResult, Cue, Locomotion, Side, state::ActorTask};
use anyhow::{Context, Result};

/// Living enemy departures finish outside the ordinary arena constraint.
const ENEMY_DEPARTURE_RADIUS: f32 = 900.;

fn away_from(owner: &crate::Actor, target: [f32; 3]) -> [f32; 3] {
    let direction = crate::distance::normalize([
        owner.position[0] - target[0],
        0.,
        owner.position[2] - target[2],
    ]);
    if direction != [0.; 3] {
        return direction;
    }
    let direction =
        crate::distance::normalize([-owner.facing_direction[0], 0., -owner.facing_direction[2]]);
    if direction == [0.; 3] {
        [1., 0., 0.]
    } else {
        direction
    }
}

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
            let direction = away_from(owner, target);
            self.cancel_pending_item(actor);
            self.clear_technique_command(actor);
            self.interrupt_actor(actor, cues);
            self.enter_idle(actor);
            self.set_task(index, ActorTask::Escaping);
            let owner = &mut self.actors[index];
            owner.movement.locomotion = Locomotion::Run;
            owner.movement.direction = direction;
        }
        self.advance_departure_movement(actor, run_speed, turn_ticks, motion)?;
        Ok(true)
    }

    pub(crate) fn begin_enemy_departure(&mut self, actor: ActorId, target: ActorId) -> Result<()> {
        let index = actor.index();
        let motion = self.enemy_decision(actor)?.walk_motion;
        let direction = away_from(&self.actors[index], self.actors[target.index()].position);
        let owner = &mut self.actors[index];
        owner.guard.active = false;
        owner.guard.recovery = 0;
        owner.movement.locomotion = Locomotion::Walk;
        owner.movement.direction = direction;
        owner.movement.forward = 0.;
        owner.movement.steering.leaving_arena = true;
        let rate = owner.motion_rate(0.5, owner.conditions.effective());
        self.request_pose(
            actor,
            motion,
            crate::Pose {
                rate,
                repeat: true,
                blend: 0,
                ..Default::default()
            },
        );
        Ok(())
    }

    /// The selected row owns this actor's movement, independently of party escape.
    pub(crate) fn advance_enemy_departure(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let index = actor.index();
        let Some((&id, sequence)) = self.runtime[index].task().action() else {
            return Ok(false);
        };
        if !matches!(sequence.execution, crate::action::Execution::LeavingArena) {
            return Ok(false);
        }
        if self.actors[index].hit_stop != 0 || sequence.started_at == self.update {
            return Ok(true);
        }
        let definition = self.prepared.actor_setup[index]
            .enemy_decision
            .as_ref()
            .context("departing enemy has no movement profile")?;
        self.advance_departure_movement(
            actor,
            definition.walk_speed,
            definition.turn_ticks,
            definition.walk_motion,
        )?;
        if let Some((_, sequence)) = self.runtime[index].action_mut() {
            sequence.age += 1;
        }
        let position = self.actors[index].position;
        if position[0].hypot(position[2]) >= ENEMY_DEPARTURE_RADIUS {
            self.actors[index].availability = crate::ActorAvailability::Absent;
            self.ledger.exclude_reward(actor);
            self.set_task(index, ActorTask::None);
            cues.push(Cue::Completed { action: id });
        }
        Ok(true)
    }

    fn advance_departure_movement(
        &mut self,
        actor: ActorId,
        speed: f32,
        turn_ticks: u8,
        motion: Option<crate::MotionBinding>,
    ) -> Result<()> {
        let index = actor.index();
        let owner = &mut self.actors[index];
        let conditions = owner.conditions.effective();
        let speed = owner.run_limit(speed, conditions);
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
        Ok(())
    }
}

#[cfg(test)]
mod tests;
