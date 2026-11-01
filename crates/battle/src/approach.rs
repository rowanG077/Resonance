//! Native movement toward an action's usable range.
use crate::action_selection::GroundMotion;
use crate::state::ActorTask;
use crate::{ActionRequest, Activity, ActorId, Battle, Cue, MotionBinding};
use anyhow::{Context, Result, ensure};

/// Retreat begins braking inside the arena's physical boundary.
const RETREAT_STOP_RADIUS: f32 = 765.;

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
    /// An enemy completes its retreat independently of the forward threshold.
    retreating: bool,
    /// Keep selection separate from an immediately usable action's admission.
    entry_pending: bool,
    elapsed: u16,
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
        let gap = self.approach_body_gap(ActorId(index as u8), approach.target);
        let owner = &self.actors[index];
        // Retreat stops before the arena edge, independently of the later
        // physical boundary constraint. It still completes ordinary braking.
        let retreat_edge = crate::distance::length([owner.position[0], 0., owner.position[2]])
            > RETREAT_STOP_RADIUS;
        !approach.range_required
            || approach.retreating
                && (gap > approach.parameters.minimum - owner.movement.forward || retreat_edge)
            || gap >= approach.parameters.minimum && gap <= approach.parameters.maximum
    }

    /// Movement tracks the opponent while the action retains its own recipient.
    pub(crate) fn request_approach(
        &mut self,
        request: ActionRequest,
        opponent: ActorId,
        parameters: ApproachParameters,
    ) -> Result<bool> {
        let ActionRequest {
            actor,
            target: recipient,
            action,
        } = request;
        self.actor(actor)?;
        let Ok(candidate) =
            self.action_candidate(actor, action, [parameters.minimum, parameters.maximum])
        else {
            return Ok(false);
        };
        let range_required = self.approach_range_required(actor);
        self.begin_approach(
            ActionRequest {
                actor,
                target: opponent,
                action,
            },
            (recipient != opponent).then_some(recipient),
            ApproachParameters {
                minimum: candidate.range[0],
                maximum: candidate.range[1],
                ..parameters
            },
            range_required,
        )
    }

    pub(crate) fn approach_range_required(&self, actor: ActorId) -> bool {
        let owner = &self.actors[actor.index()];
        owner.control != crate::Control::Manual
            && !(owner.control == crate::Control::Enemy && owner.movement.turning_disabled)
    }

    pub(crate) fn begin_approach(
        &mut self,
        request: ActionRequest,
        action_target: Option<ActorId>,
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
        if let Some(recipient) = action_target {
            self.actor(recipient)?;
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
                && parameters.maximum >= 0.
                && (owner.control == crate::Control::Enemy
                    || parameters.maximum > parameters.minimum)
                && parameters.speed.is_finite()
                && parameters.speed > 0.
                && parameters.turn_ticks > 0,
            "invalid approach parameters"
        );
        let retreating = range_required
            && owner.control == crate::Control::Enemy
            && self.approach_body_gap(actor, target) < parameters.minimum;
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
                action_target,
                parameters,
                range_required,
                retreating,
                entry_pending: true,
                elapsed: 0,
            }),
        );
        self.actors[index].movement.locomotion = if self.approach_within_range(index) {
            crate::Locomotion::Action
        } else if self.runtime[index].enemy_walk_override.is_some() {
            crate::Locomotion::Walk
        } else {
            crate::Locomotion::Run
        };
        self.begin_approach_steering(actor, target)?;
        if !self.approach_within_range(index)
            && matches!(
                self.actors[index].control,
                crate::Control::Auto | crate::Control::Enemy
            )
        {
            let direction = crate::distance::planar_direction(
                self.actors[target.index()].position,
                self.actors[index].position,
                self.actors[index].facing_direction,
            );
            // A fresh approach starts toward its destination, not along a previous retreat.
            let direction = if self.approach_body_gap(actor, target) < parameters.minimum {
                direction.map(|value| -value)
            } else {
                direction
            };
            self.actors[index].movement.direction = direction;
            self.actors[index].facing_direction = direction;
        }
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
        self.begin_approach(request, None, parameters, false)?;
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

    pub(crate) fn cancel_approach(&mut self, index: usize) {
        self.set_task(index, ActorTask::None);

        if self.runtime[index].control.is_some() {
            let definition = self.prepared.actor_setup[index]
                .control
                .as_ref()
                .unwrap()
                .clone();
            self.control_motion(index, &definition, crate::control::Locomotion::Stop);
        } else {
            self.enter_idle(ActorId(index as u8));
        }
    }

    /// Own the complete movement update while an action is approaching. The caller
    /// must not also run generic integration when this returns true.
    pub(crate) fn update_approach(&mut self, actor: ActorId, cues: &mut Vec<Cue>) -> Result<bool> {
        let index = actor.index();
        let Some(mut approach) = self.runtime[index].task().approach() else {
            return Ok(false);
        };
        if self.terminal.result.is_some() {
            self.cancel_approach(index);
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
            self.cancel_approach(index);
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
                self.cancel_approach(index);
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
            self.cancel_approach(index);
            return Ok(false);
        }
        let motion_ready = self
            .models
            .as_ref()
            .is_none_or(|models| models.motion_ready_this_update(actor));
        let entry_pending = approach.entry_pending;
        approach.entry_pending = false;
        let gap = self.approach_body_gap(actor, approach.target);
        // A moving enemy chooses retreat from this visit's gap. A changing pose
        // or target can enter range before braking begins; only an already
        // admitted stopping phase keeps its retreat until it finishes facing.
        if approach.range_required && gap >= approach.parameters.minimum {
            approach.retreating = false;
        }
        approach.retreating |= approach.range_required
            && self.actors[index].control == crate::Control::Enemy
            && gap < approach.parameters.minimum;
        let in_range = self.approach_in_range(index, approach);
        let enemy = self.actors[index].control == crate::Control::Enemy;
        let walking = enemy
            && approach.range_required
            && (!in_range || self.actors[index].movement.locomotion == crate::Locomotion::Run);
        if enemy && entry_pending {
            // Enemy selection finishes the outgoing idle hover before approach takes over.
            self.advance_hover(index, true);
        }
        if walking && entry_pending {
            let direction = crate::distance::planar_direction(
                self.actors[approach.target.index()].position,
                self.actors[index].position,
                self.actors[index].movement.direction,
            );
            let direction = if approach.retreating {
                direction.map(|value| -value)
            } else {
                direction
            };
            let owner = &mut self.actors[index];
            owner.movement.direction = direction;
            owner.facing_direction = direction;
            crate::control::face(
                owner,
                direction,
                180. / f32::from(approach.parameters.turn_ticks),
            );
            self.set_task(index, ActorTask::Approach(approach));
            return Ok(true);
        }
        let elapsed = u32::from(approach.elapsed) + u32::from(motion_ready);
        approach.elapsed = elapsed.min(u32::from(u16::MAX)) as u16;
        self.emit_movement_effect(actor, approach.elapsed, cues);
        if !in_range
            && self.actors[index].side == crate::Side::Party
            && self.actors[index].control == crate::Control::Auto
            && let Some(policy) = self.companion_policy(actor)
            && (elapsed > u32::from(policy.positioning.approach_timeout_ticks)
                || policy.position == crate::companion::PositionStrategy::HoldPosition
                    && self.pending_technique(actor) != Some(approach.action))
        {
            self.cancel_approach(index);
            return Ok(false);
        }

        if walking {
            // Finish this movement visit before admitting an action from its starting gap.
            self.move_approach(actor, approach, gap)?;
        }
        // Retreat stopping compares the visit's sampled gap with the newly
        // accelerated speed, after movement.
        let in_range = in_range
            || walking
                && approach.retreating
                && gap > approach.parameters.minimum - self.actors[index].movement.forward;
        let airborne_selection = entry_pending
            && in_range
            && self.actors[index].airborne()
            && !self.actors[index].movement.flying;
        let direction =
            if enemy && !approach.retreating && (!approach.range_required || airborne_selection) {
                self.actors[index].movement.direction
            } else {
                crate::distance::planar_direction(
                    self.actors[approach.target.index()].position,
                    self.actors[index].position,
                    self.actors[index].movement.direction,
                )
            };
        if matches!(
            self.actors[index].control,
            crate::Control::Auto | crate::Control::Enemy
        ) && in_range
        {
            // Once an automatic actor arrives, finish facing without chasing changes in posed bounds.
            approach.range_required = false;
        }
        self.set_task(index, ActorTask::Approach(approach));
        if in_range {
            self.actors[index].movement.locomotion = crate::Locomotion::Action;
            if walking {
                if enemy && approach.retreating && !self.actors[index].movement.flying {
                    self.model_requests.push(crate::ModelRequest::Common {
                        actor,
                        pose: crate::CommonPose::Stopping,
                    });
                }
                if !approach.retreating {
                    let owner = &mut self.actors[index];
                    crate::control::face(
                        owner,
                        direction,
                        180. / f32::from(approach.parameters.turn_ticks),
                    );
                    owner.movement.direction = owner.movement.target_direction;
                    owner.facing_direction = owner.movement.direction;
                }
                return Ok(true);
            }
            let normal_momentum = self.actors[index].control == crate::Control::Auto
                && self.prepared.actions[approach.action].normal.is_some();
            if !enemy && !normal_momentum {
                self.actors[index].movement.forward = 0.;
            }
            if enemy && approach.retreating {
                // Finish coasting away before turning back toward the target.
                let owner = &mut self.actors[index];
                owner.movement.integrate(&mut owner.position);
                let stopped = if owner.movement.flying {
                    owner.movement.forward = 0.;
                    true
                } else {
                    owner
                        .movement
                        .brake(owner.position[1], Activity::Approaching)
                };
                self.advance_hover(index, false);
                let stop_finished = self.actors[index].movement.flying
                    || self
                        .models
                        .as_ref()
                        .is_none_or(|models| models.stop_finished_this_update(actor, true));
                if !stopped || !self.actors[index].movement.hover_ready() || !stop_finished {
                    return Ok(true);
                }
            } else if !self.actors[index].movement.hover_ready() {
                let owner = &mut self.actors[index];
                owner.movement.integrate(&mut owner.position);
                self.advance_hover(index, false);
                return Ok(true);
            }
            if self.pending_aerial_jump(actor, self.pending_technique(actor)) {
                self.begin_queued_jump(actor);
                return self.update_mobility(actor, cues);
            }
            if entry_pending && self.actors[index].side == crate::Side::Party {
                return Ok(true);
            }
            let owner = &mut self.actors[index];
            let ready = if owner.control == crate::Control::Manual
                || owner.movement.turning_disabled
                || owner.side == crate::Side::Party && owner.airborne()
            {
                true
            } else {
                owner.movement.direction = direction;
                crate::control::face(
                    owner,
                    direction,
                    180. / f32::from(approach.parameters.turn_ticks),
                )
            };
            if !ready || entry_pending {
                return Ok(true);
            }
            if enemy && approach.retreating {
                // Retreat finishes facing before the following visit admits the action.
                approach.retreating = false;
                self.set_task(index, ActorTask::Approach(approach));
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
                self.begin_paralysis(actor, cues);
                return Ok(true);
            }
            if self.prepared.actions[request.action].normal.is_some() {
                self.runtime[index].set_target(approach.target);
                self.runtime[index].control.as_mut().unwrap().attack_target = approach.target;
                self.runtime[index].combo = Default::default();
            }
            self.start_admitted_action(request, cues)?;
            return Ok(matches!(
                self.prepared.actions[request.action].execution,
                crate::ActionExecution::Attack(_)
            ) && !self.enemy_physical_arte(actor, request.action));
        }
        if !walking {
            self.move_approach(actor, approach, gap)?;
        }
        Ok(true)
    }

    fn move_approach(&mut self, actor: ActorId, approach: Approach, gap: f32) -> Result<()> {
        let index = actor.index();
        let direction = crate::distance::planar_direction(
            self.actors[approach.target.index()].position,
            self.actors[index].position,
            self.actors[index].movement.direction,
        );
        let destination = if gap < approach.parameters.minimum
            && (approach.retreating || self.actors[index].control != crate::Control::Enemy)
        {
            std::array::from_fn(|axis| {
                self.actors[index].position[axis] - direction[axis] * approach.parameters.minimum
            })
        } else {
            self.actor_start_centers[approach.target.index()]
        };
        let destination = self.steer_approach(actor, approach.target, destination)?;
        let speed = self.runtime[index]
            .enemy_walk_override
            .unwrap_or(approach.parameters.speed);
        let owner = &mut self.actors[index];
        owner.movement.locomotion = if self.runtime[index].enemy_walk_override.is_some() {
            crate::Locomotion::Walk
        } else {
            crate::Locomotion::Run
        };
        let desired = crate::distance::planar_direction(destination, owner.position, direction);
        const DIRECTION_BLEND: f32 = 0.1;
        owner.movement.direction = crate::distance::normalize(std::array::from_fn(|axis| {
            owner.movement.direction[axis] * (1. - DIRECTION_BLEND)
                + desired[axis] * DIRECTION_BLEND
        }));
        let movement = owner.movement.direction;
        crate::control::face(
            owner,
            movement,
            180. / f32::from(approach.parameters.turn_ticks),
        );
        let limit = owner.run_limit(speed, owner.conditions.effective());
        if owner.control == crate::Control::Enemy {
            owner.movement.integrate(&mut owner.position);
            owner.movement.forward =
                (owner.movement.forward + crate::movement::RUN_ACCELERATION).min(limit);
        } else {
            let desired_gap = approach
                .parameters
                .minimum
                .midpoint(approach.parameters.maximum);
            owner.movement.forward = (owner.movement.forward + crate::movement::RUN_ACCELERATION)
                .min(limit)
                .min((gap - desired_gap).abs());
            owner.movement.integrate(&mut owner.position);
        }
        self.advance_hover(index, true);
        Ok(())
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
