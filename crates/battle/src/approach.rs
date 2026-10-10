//! Native movement toward an action's usable range.
use crate::action_selection::GroundMotion;
use crate::state::ActorTask;
use crate::{ActionRequest, Activity, ActorId, Battle, Cue, MotionBinding};
use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone, Copy)]
pub struct ApproachParameters {
    pub minimum: f32,
    pub maximum: f32,
    pub motion: Option<MotionBinding>,
    pub motion_rate: f32,
    pub speed: f32,
    pub turn_ticks: u8,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Approach {
    action: crate::ActionKey,
    target: ActorId,
    action_target: Option<ActorId>,
    parameters: ApproachParameters,
    range_required: bool,
}

impl Battle {
    pub(crate) fn approach_jump_allowed(&self, index: usize) -> bool {
        self.runtime[index].task().approach().is_some()
    }

    pub(crate) fn approach_moving(&self, index: usize) -> bool {
        self.runtime[index]
            .task()
            .approach()
            .is_some_and(|approach| !self.approach_in_range(index, approach))
    }

    pub(crate) fn approach_within_range(&self, index: usize) -> bool {
        self.runtime[index]
            .task()
            .approach()
            .is_some_and(|approach| self.approach_in_range(index, approach))
    }

    fn approach_in_range(&self, index: usize, approach: Approach) -> bool {
        let gap =
            crate::control::body_gap(&self.actors[index], &self.actors[approach.target.index()]);
        !approach.range_required
            || gap >= approach.parameters.minimum && gap <= approach.parameters.maximum
            || gap < approach.parameters.minimum
                && self.actors[index].movement.steering.arena_contact() != crate::ArenaContact::None
    }

    pub(crate) fn request_approach(
        &mut self,
        actor: ActorId,
        target: ActorId,
        action: crate::ActionKey,
        parameters: ApproachParameters,
    ) -> Result<bool> {
        let owner = self.actor(actor)?;
        if self.terminal.result.is_some()
            || !owner.available()
            || !matches!(
                self.activity(actor),
                Activity::Idle | Activity::Guarding | Activity::Approaching | Activity::Jumping
            )
        {
            return Ok(false);
        }
        let Ok(candidate) =
            self.action_candidate(actor, action, [parameters.minimum, parameters.maximum])
        else {
            return Ok(false);
        };
        self.request_selected_approach(
            actor,
            target,
            action,
            ApproachParameters {
                minimum: candidate.range[0],
                maximum: candidate.range[1],
                ..parameters
            },
        )
    }

    pub(crate) fn request_selected_approach(
        &mut self,
        actor: ActorId,
        target: ActorId,
        action: crate::ActionKey,
        parameters: ApproachParameters,
    ) -> Result<bool> {
        let range_required = self.actors[actor.index()].control != crate::Control::Manual;
        self.begin_approach(
            ActionRequest {
                actor,
                target,
                action,
            },
            parameters,
            range_required,
        )
    }

    pub(crate) fn begin_approach(
        &mut self,
        request: ActionRequest,
        parameters: ApproachParameters,
        range_required: bool,
    ) -> Result<bool> {
        let ActionRequest {
            actor,
            target,
            action,
        } = request;
        let owner = self.actor(actor)?;
        if self.terminal.result.is_some()
            || !owner.available()
            || !matches!(
                self.activity(actor),
                Activity::Idle | Activity::Guarding | Activity::Approaching | Activity::Jumping
            )
        {
            return Ok(false);
        }
        ensure!(
            self.prepared.actions.get(action).is_some(),
            "unprepared approach action"
        );
        ensure!(
            self.actor(target)?.side != owner.side,
            "invalid approach target"
        );
        ensure!(
            parameters.minimum.is_finite()
                && parameters.maximum.is_finite()
                && parameters.minimum >= 0.
                && parameters.maximum > parameters.minimum
                && parameters.speed.is_finite()
                && parameters.speed > 0.
                && parameters.turn_ticks > 0,
            "invalid approach parameters"
        );
        let index = actor.index();
        self.runtime[index].combo = Default::default();
        if let Some(control) = &mut self.runtime[index].control {
            control.attack_target = target;
            control.run_ticks = 0;
        }
        if !self.actors[index].movement.flying && !self.actors[index].airborne() {
            self.actors[index].movement.gravity = crate::movement::GRAVITY;
        }
        self.actors[index].attack_power = 100;
        self.actors[index].guard.active = false;
        self.actors[index].guard.recovery = 0;
        self.actors[index].guard.kind = crate::GuardKind::Normal;

        self.set_task(
            index,
            ActorTask::Approach(Approach {
                action,
                target,
                action_target: None,
                parameters,
                range_required,
            }),
        );
        self.actors[index].movement.locomotion = if self.approach_within_range(index) {
            crate::Locomotion::Action
        } else {
            crate::Locomotion::Run
        };
        self.begin_approach_steering(actor, target)?;
        if !self.approach_within_range(index)
            && let Some(motion) = parameters.motion
        {
            let rate = self.actors[index].motion_rate(
                parameters.motion_rate,
                self.actors[index].conditions.effective(),
            );
            self.request_pose(
                actor,
                Some(motion),
                crate::Pose {
                    rate,
                    repeat: true,
                    ..Default::default()
                },
            );
        }
        Ok(true)
    }

    pub(crate) fn admit_jump_approach(
        &mut self,
        request: ActionRequest,
        parameters: ApproachParameters,
    ) -> Result<()> {
        self.begin_approach(request, parameters, false)?;
        Ok(())
    }

    pub(crate) fn set_approach_action_target(
        &mut self,
        actor: ActorId,
        target: ActorId,
    ) -> Result<()> {
        self.actor(target)?;
        let Some(mut approach) = self.runtime[actor.index()].task().approach() else {
            anyhow::bail!("queued action has no approach");
        };
        approach.action_target = Some(target);
        self.set_task(actor.index(), ActorTask::Approach(approach));
        Ok(())
    }

    pub(crate) fn cancel_approach(&mut self, index: usize) -> Result<()> {
        self.set_task(index, ActorTask::None);

        if self.runtime[index].control.is_some() {
            let definition = self.prepared.actor_setup[index]
                .control
                .as_ref()
                .unwrap()
                .clone();
            self.control_motion(index, &definition, crate::control::Locomotion::Stop)?;
        } else {
            self.enter_idle(ActorId(index as u8));
        }
        Ok(())
    }

    /// Own the complete movement update while an action is approaching. The caller
    /// must not also run generic integration when this returns true.
    pub(crate) fn update_approach(&mut self, actor: ActorId, cues: &mut Vec<Cue>) -> Result<bool> {
        let index = actor.index();
        let Some(mut approach) = self.runtime[index].task().approach() else {
            return Ok(false);
        };
        if self.terminal.result.is_some() {
            self.cancel_approach(index)?;
            cues.push(Cue::Rejected {
                actor,
                reason: crate::Rejection::BattleEnding,
            });
            return Ok(false);
        }
        if !self.actors[index].available() {
            self.interrupt_actor(actor, cues);
            return Ok(false);
        }
        if self.actors[index].hit_stop > 0 {
            return Ok(true);
        }
        if self.has_pending_item(actor) || self.actors[index].input.motion == GroundMotion::Stop {
            self.cancel_approach(index)?;
            return Ok(false);
        }
        if self.actors[index].side == crate::Side::Party
            && self.actors[index].control == crate::Control::Auto
            && self.approach_normal(actor).is_some()
        {
            self.advance_ai_approach(actor)?;
            approach = self.runtime[index].task().approach().unwrap();
        }
        if self.pending_technique(actor) == Some(approach.action)
            && let Some(target) = self.pending_technique_target(actor)
        {
            if !self.technique_target_eligible(actor, approach.action, target) {
                self.clear_technique_command(actor);
                self.cancel_approach(index)?;
                return Ok(false);
            }
            approach.target = target;
        } else if let Some(target) = self
            .target(actor)
            .filter(|target| self.target_available(actor, *target))
        {
            approach.target = target;
        }
        if !self.actors[approach.target.index()].available() {
            self.cancel_approach(index)?;
            return Ok(false);
        }
        self.set_task(index, ActorTask::Approach(approach));
        let direction = crate::distance::planar_direction(
            self.actors[approach.target.index()].position,
            self.actors[index].position,
            self.actors[index].movement.direction,
        );
        self.actors[index].movement.target_direction = direction;
        if self.approach_in_range(index, approach) {
            self.actors[index].movement.locomotion = crate::Locomotion::Action;
            self.actors[index].movement.forward = 0.;
            if !self.actors[index].movement.hover_ready() {
                let owner = &mut self.actors[index];
                owner.movement.integrate(&mut owner.position);
                self.advance_hover(index, false)?;
                return Ok(true);
            }
            if self.pending_aerial_jump(actor, self.pending_technique(actor)) {
                self.begin_queued_jump(actor);
                return self.update_mobility(actor, cues);
            }
            let owner = &mut self.actors[index];
            let ready = owner.control == crate::Control::Manual
                || owner.movement.turning_disabled
                || owner.side == crate::Side::Party && owner.airborne()
                || crate::control::face(
                    owner,
                    direction,
                    180. / f32::from(approach.parameters.turn_ticks),
                );
            if !ready {
                return Ok(true);
            }
            self.set_task(index, ActorTask::None);
            let request = ActionRequest {
                actor,
                target: approach.action_target.unwrap_or(approach.target),
                action: approach.action,
            };
            if let Some(reason) = self.action_rejection(request)? {
                cues.push(Cue::Rejected { actor, reason });
                self.enter_idle(actor);
                return Ok(true);
            }
            if crate::conditions::paralysis_controller_roll(&self.actors[index], &mut self.random) {
                self.clear_technique_command(actor);
                self.begin_paralysis(actor, cues)?;
                return Ok(true);
            }
            if self.prepared.actions[request.action].normal.is_some() {
                self.runtime[index].target = approach.target;
                self.runtime[index].control.as_mut().unwrap().attack_target = approach.target;
                self.runtime[index].combo = Default::default();
            }
            self.start_admitted_action(request, cues)?;
            return Ok(true);
        }
        let gap =
            crate::control::body_gap(&self.actors[index], &self.actors[approach.target.index()]);
        let destination = if gap < approach.parameters.minimum {
            std::array::from_fn(|axis| {
                self.actors[index].position[axis] - direction[axis] * approach.parameters.minimum
            })
        } else {
            self.actors[approach.target.index()].position
        };
        let destination = self.steer_approach(actor, approach.target, destination)?;
        let owner = &mut self.actors[index];
        owner.movement.locomotion = crate::Locomotion::Run;
        let desired = crate::distance::planar_direction(destination, owner.position, direction);
        owner.movement.direction = crate::control::turn_direction(
            owner.movement.direction,
            desired,
            180. / f32::from(approach.parameters.turn_ticks),
        );
        let movement = owner.movement.direction;
        crate::control::face(
            owner,
            movement,
            180. / f32::from(approach.parameters.turn_ticks),
        );
        let desired_gap = approach
            .parameters
            .minimum
            .midpoint(approach.parameters.maximum);
        let remaining = (gap - desired_gap).abs();
        owner.movement.forward = (owner.movement.forward + crate::movement::RUN_ACCELERATION)
            .min(owner.run_limit(approach.parameters.speed, owner.conditions.effective()))
            .min(remaining);
        owner.movement.integrate(&mut owner.position);
        self.advance_hover(index, true)?;
        Ok(true)
    }

    pub(crate) fn approach_normal(&self, actor: ActorId) -> Option<crate::NormalAttack> {
        let approach = self.runtime[actor.index()].task().approach()?;
        self.prepared.actions[approach.action].normal
    }

    pub(crate) fn reselect_approach_normal(
        &mut self,
        actor: ActorId,
        selector: crate::NormalAttack,
    ) -> Result<()> {
        let index = actor.index();
        let normal = self.prepared.actor_setup[index]
            .control
            .as_ref()
            .context("approach needs controls")?
            .normals[selector as usize];
        let maximum = self.actors[index].weapon_reach(normal.reach);
        let Some(mut approach) = self.runtime[index].task().approach() else {
            anyhow::bail!("missing approach");
        };
        approach.action = normal.action;
        approach.parameters.maximum = maximum;
        approach.parameters.minimum = if self.actors[index].control == crate::Control::Auto {
            normal.minimum_reach
        } else {
            0.
        };
        self.set_task(index, ActorTask::Approach(approach));
        Ok(())
    }
}

#[cfg(test)]
mod tests;
