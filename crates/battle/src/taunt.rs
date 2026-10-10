use crate::state::ActorTask;
use crate::{ActorId, Battle, Cue, PreparedBattle};
use anyhow::{Result, ensure};

pub const MAX_UNISON_GAUGE: i16 = 3200;
const SAVED_GAUGE_SCALE: i16 = 16;
const TAUNT_GAUGE_GAIN: i16 = 48;
const TAUNT_DURATION_TICKS: u32 = 90;

impl PreparedBattle {
    pub fn with_unison_gauge(
        mut self,
        saved: u8,
        unlocked: bool,
        initial_full: bool,
    ) -> Result<Self> {
        ensure!(
            i16::from(saved) <= MAX_UNISON_GAUGE / SAVED_GAUGE_SCALE,
            "invalid saved U.Attack gauge"
        );
        self.resources.unison_gauge = if initial_full {
            MAX_UNISON_GAUGE
        } else {
            i16::from(saved) * SAVED_GAUGE_SCALE
        };
        self.resources.unison_available = unlocked;
        Ok(self)
    }
}

impl Battle {
    pub fn unison_gauge(&self) -> i16 {
        self.unison_gauge
    }
    pub fn saved_unison_gauge(&self) -> u8 {
        (self.unison_gauge.clamp(0, MAX_UNISON_GAUGE) / SAVED_GAUGE_SCALE) as u8
    }

    pub(crate) fn begin_taunt(&mut self, actor: ActorId) -> Result<()> {
        let index = actor.index();
        self.set_task(
            index,
            ActorTask::Taunt {
                remaining: TAUNT_DURATION_TICKS,
            },
        );
        if let Some(control) = &mut self.runtime[index].control {
            control.run_ticks = 0;
        }
        let owner = &mut self.actors[index];

        owner.guard.active = false;
        if owner.equipment.taunt_guard {
            owner.reaction.protection.armor(TAUNT_DURATION_TICKS);
        }
        owner.movement.locomotion = crate::Locomotion::Action;
        owner.movement.gravity = if owner.movement.flying { 0. } else { -1. };
        self.model_requests.push(crate::ModelRequest::Common {
            actor,
            pose: crate::CommonPose::Taunt,
        });
        Ok(())
    }

    pub(crate) fn advance_taunt(
        &mut self,
        actor: ActorId,
        remaining: u32,
        input: crate::ControlInput,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let index = actor.index();
        if matches!(
            self.actors[index].control,
            crate::Control::Manual | crate::Control::SemiAuto
        ) && self.actors[index].equipment.taunt_cancel
            && input.guard.held
        {
            self.cancel_taunt(actor)?;
            self.integrate_taunt(actor);
            return Ok(true);
        }
        if remaining <= 1 {
            self.finish_taunt(actor, cues)?;
        } else {
            self.set_task(
                index,
                ActorTask::Taunt {
                    remaining: remaining - 1,
                },
            );
            self.integrate_taunt(actor);
            self.advance_hover(index, true)?;
        }
        Ok(true)
    }

    fn cancel_taunt(&mut self, actor: ActorId) -> Result<()> {
        let index = actor.index();
        self.enter_player_guard(index);
        self.set_task(index, ActorTask::None);
        let control = self.runtime[index].control.as_mut().unwrap();
        self.actors[index].movement.locomotion = crate::control::Locomotion::Action;
        control.run_ticks = 0;
        Ok(())
    }

    fn finish_taunt(&mut self, actor: ActorId, cues: &mut Vec<Cue>) -> Result<()> {
        let index = actor.index();
        if self.actors[index].side == crate::Side::Party {
            self.add_unison_gauge(TAUNT_GAUGE_GAIN, cues);
        }
        self.complete_taunt_recovery(actor, cues)?;
        let owner = &mut self.actors[index];
        owner.movement.direction = owner.facing_direction;
        self.enter_idle(actor);
        Ok(())
    }

    fn integrate_taunt(&mut self, actor: ActorId) {
        let index = actor.index();
        let setup = &self.prepared.actor_setup[index];
        let turn_ticks = setup
            .control
            .as_ref()
            .map(|definition| definition.turn_ticks)
            .or_else(|| {
                setup
                    .enemy_decision
                    .as_ref()
                    .map(|definition| definition.turn_ticks)
            });
        let owner = &mut self.actors[index];
        if let Some(ticks) = turn_ticks {
            let direction = owner.facing_direction;
            crate::control::face(owner, direction, 180. / f32::from(ticks));
        }
        owner
            .movement
            .integrate_along(&mut owner.position, owner.movement.direction);
        owner
            .movement
            .brake(owner.position[1], crate::Activity::Taunting);
        crate::movement::floor(owner);
    }

    pub(crate) fn add_unison_gauge(&mut self, amount: i16, cues: &mut Vec<Cue>) {
        if !self.prepared.unison_available {
            return;
        }
        let next = (i32::from(self.unison_gauge) + i32::from(amount))
            .clamp(0, i32::from(MAX_UNISON_GAUGE)) as i16;
        let crossed = self.unison_gauge < MAX_UNISON_GAUGE && next == MAX_UNISON_GAUGE;
        if crossed {
            cues.push(Cue::UnisonReady);
        }
        self.unison_gauge = next;
    }
}

#[cfg(test)]
mod tests;
