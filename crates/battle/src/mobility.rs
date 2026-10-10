//! Native jump, backstep and breakfall movement.
use crate::conditions::Condition;
use crate::state::ActorTask;
use crate::{ActorId, Battle, CommonPose, Control, ControlInput, Cue, ModelRequest};
use anyhow::{Context, Result};

const JUMP_STICK_THRESHOLD: i8 = 48;
const JUMP_CHARGE_UPDATES: u8 = 6;
const BACKSTEP_STICK_THRESHOLD: i8 = 30;
const BACKSTEP_GUARD_TICKS: u32 = 30;
const JUMP_SPEED: f32 = 20.5;
const BACKSTEP_SPEED: f32 = 11.;
const BACKSTEP_LIFT: f32 = 12.5;
const BACKSTEP_GRAVITY: f32 = -1.5;
const LANDING_TICKS: u8 = 16;
const QUICK_LANDING_TICKS: u8 = 12;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct JumpCharge(u8);
impl JumpCharge {
    pub(crate) fn sample(
        &mut self,
        control: Control,
        up: i8,
        guard: bool,
        restricted: bool,
    ) -> bool {
        if up <= JUMP_STICK_THRESHOLD || restricted {
            self.0 = 0;
        } else if control == Control::Manual || control == Control::SemiAuto && guard {
            if self.0 + 1 >= JUMP_CHARGE_UPDATES {
                self.0 = 0;
                return true;
            }
            self.0 += 1;
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mobility {
    Jump { launched: bool },
    Backstep { launched: bool },
    Breakfall,
    Landing { ticks: u8, breakfall: bool },
}
impl Mobility {
    pub(crate) const fn jump() -> Self {
        Self::Jump { launched: false }
    }
    pub(crate) const fn backstep() -> Self {
        Self::Backstep { launched: false }
    }
}

impl Battle {
    pub(crate) fn backstep_requested(
        &self,
        index: usize,
        target: ActorId,
        guarding: bool,
        input: ControlInput,
    ) -> bool {
        if !guarding || !input.guard.held {
            return false;
        }
        let projected = |point: [f32; 3]| {
            self.camera.as_ref().map_or(point[0], |camera| {
                crate::control::project_screen_x(camera.pose, [point[0], 0., point[2]])
            })
        };
        let owner = projected(self.actors[index].position);
        let target = projected(self.actors[target.index()].position);
        input.horizontal_pressed > 0 && input.stick[0] >= BACKSTEP_STICK_THRESHOLD && owner > target
            || input.horizontal_pressed < 0
                && input.stick[0] <= -BACKSTEP_STICK_THRESHOLD
                && owner < target
    }

    pub(crate) fn pending_aerial_jump(
        &self,
        actor: ActorId,
        pending: Option<crate::ActionKey>,
    ) -> bool {
        pending.is_some_and(|action| {
            self.prepared.technique(actor, action).is_some_and(|row| {
                row.capabilities.regal_family == Some(crate::RegalArteFamily::Aerial)
            })
        })
    }

    /// Consume the mapped command after an optional falling queue attempt. Later selection may
    /// replace it before movement completes.
    pub(crate) fn consume_jump_command(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<()> {
        let index = actor.index();
        let owner = &self.actors[index];
        let jumping = matches!(
            self.runtime[index].task().mobility(),
            Some(Mobility::Jump { launched: true } | Mobility::Breakfall)
        );
        if !jumping {
            return Ok(());
        }
        let definition = self.prepared.actor_setup[index]
            .control
            .as_ref()
            .context("jump command needs prepared control")?
            .clone();
        // Only a falling Regal family command attempts this queue. It does not roll for support
        // or candidates.
        if owner.movement.vertical <= 0.
            && let Some(action) = self.pending_technique(actor)
            && self.prepared.technique(actor, action).is_some_and(|row| {
                row.capabilities.regal_family == Some(crate::RegalArteFamily::Aerial)
            })
        {
            if let Some(candidate) = self.project_local_technique(actor, action, cues) {
                self.actors[index].input = crate::action_selection::InputIntent {
                    action: Some(candidate),
                    ..Default::default()
                };
            }
            // Clear the queued command even after TP, condition, or height rejection.
            self.clear_technique_command(actor);
        }
        // Fresh airborne Guard is checked independently of falling. EX60 reverses both movement
        // vectors before selection.
        let breakfall = self.runtime[index].task().mobility() == Some(Mobility::Breakfall);
        if let Some(candidate) = self.actors[index].input.action
            && (!breakfall || self.actors[index].equipment.control_ex.rebound)
        {
            let action = candidate.action;
            let [minimum, maximum] = candidate.range;
            let target = self.target(actor).context("jump command has no opponent")?;
            let parameters = crate::ApproachParameters {
                minimum,
                maximum,
                motion: definition.motions.map(|motions| motions.run),
                motion_rate: 0.5,
                speed: definition.run_speed,
                turn_ticks: definition.turn_ticks,
            };
            self.runtime[index].control.as_mut().unwrap().jump_charge = Default::default();
            if breakfall {
                self.actors[index].movement.direction =
                    self.actors[index].movement.direction.map(|axis| -axis);
                self.actors[index].movement.forward *= -1.;
            }
            self.admit_jump_approach(
                crate::ActionRequest {
                    actor,
                    target,
                    action,
                },
                parameters,
            )?;
            let owner = &mut self.actors[index];
            // Snap heading only. The remaining update still turns toward its retained facing
            // direction.
            let direction = owner.movement.direction;
            crate::control::snap_heading(owner, direction);
            self.runtime[index].combo = Default::default();
            let control = self.runtime[index].control.as_mut().unwrap();
            control.attack_target = target;
            self.actors[index].movement.locomotion = crate::control::Locomotion::Action;
        }
        Ok(())
    }

    pub(crate) fn consume_double_jump(&mut self, actor: ActorId) -> bool {
        let index = actor.index();
        let owner = &self.actors[index];
        let edge = self
            .cast_inputs
            .get(usize::from(owner.control_slot))
            .is_some_and(|input| input.up_pressed);
        if !matches!(
            self.runtime[index].task().mobility(),
            Some(Mobility::Jump { launched: true })
        ) || !owner.equipment.control_ex.double_jump
            || owner.control_ex_state.double_jump_used
            || !edge
        {
            return false;
        }
        self.actors[index].control_ex_state.double_jump_used = true;
        self.actors[index].input.action = None;
        true
    }

    pub(crate) fn begin_queued_jump(&mut self, actor: ActorId) -> bool {
        let index = actor.index();
        let blocked = self.actors[index]
            .conditions
            .effective()
            .contains(Condition::Heavy);
        let Some(control) = &mut self.runtime[index].control else {
            return false;
        };
        if blocked {
            control.queued_technique = None;
            return false;
        }
        self.set_task(index, ActorTask::Mobility(Mobility::jump()));
        self.actors[index].movement.locomotion = crate::control::Locomotion::Action;

        true
    }

    /// Own movement for this update, even when recovery finishes during it.
    pub(crate) fn update_mobility(&mut self, actor: ActorId, cues: &mut Vec<Cue>) -> Result<bool> {
        let index = actor.index();
        let Some(mobility) = self.runtime[index].task().mobility() else {
            return Ok(false);
        };
        if !self.actors[index].available() {
            self.interrupt_actor(actor, cues);
            return Ok(false);
        }
        if self.actors[index].hit_stop > 0 {
            return Ok(true);
        }
        let setup = &self.prepared.actor_setup[index];
        let breakfall = matches!(
            mobility,
            Mobility::Breakfall
                | Mobility::Landing {
                    breakfall: true,
                    ..
                }
        );
        let turn_speed = if breakfall {
            22.5 // Half a turn in eight updates.
        } else {
            180. / f32::from(setup.control.as_ref().unwrap().turn_ticks)
        };
        let mut next = mobility;
        match mobility {
            Mobility::Jump { launched } => {
                if !launched {
                    if self.actors[index]
                        .conditions
                        .effective()
                        .contains(Condition::Heavy)
                    {
                        self.set_task(index, ActorTask::None);
                        self.clear_technique_command(actor);
                        return Ok(false);
                    }
                    self.model_requests.push(ModelRequest::Common {
                        actor,
                        pose: CommonPose::Jump,
                    });
                    let owner = &mut self.actors[index];
                    self.runtime[index].combo = Default::default();
                    owner.guard.active = false;

                    owner.movement.acceleration = 0.;
                    owner.movement.vertical = JUMP_SPEED;
                    owner.movement.gravity = crate::movement::GRAVITY;
                    cues.push(Cue::Jumped {
                        actor,
                        position: owner.position,
                    });
                    next = Mobility::Jump { launched: true };
                    self.set_task(index, ActorTask::Mobility(next));
                } else if self.consume_double_jump(actor) {
                    self.model_requests.push(ModelRequest::Common {
                        actor,
                        pose: CommonPose::Jump,
                    });
                    self.actors[index].movement.vertical = JUMP_SPEED;
                    self.actors[index].movement.gravity = crate::movement::GRAVITY;
                } else {
                    self.consume_jump_command(actor, cues)?;
                }
            }
            Mobility::Backstep { launched: false } => {
                self.model_requests.push(ModelRequest::Common {
                    actor,
                    pose: CommonPose::Backstep,
                });
                let owner = &mut self.actors[index];
                self.runtime[index].combo = Default::default();
                owner.guard.active = false;

                owner.movement.direction = owner.movement.target_direction;
                owner.facing_direction = owner.movement.target_direction;
                owner.reaction.direction = owner.movement.target_direction.map(|axis| -axis);
                owner.movement.forward = BACKSTEP_SPEED;
                owner.movement.vertical = BACKSTEP_LIFT;
                owner.movement.gravity = BACKSTEP_GRAVITY;
                owner.movement.acceleration = 0.;
                owner.movement.turning_disabled = false;
                if owner.equipment.backstep_guard {
                    owner.reaction.protection.armor(BACKSTEP_GUARD_TICKS);
                }
                let direction = owner.facing_direction;
                crate::control::snap_heading(owner, direction);
                next = Mobility::Backstep { launched: true };
            }
            Mobility::Breakfall => {
                if self.runtime[index].control.is_some() {
                    self.consume_jump_command(actor, cues)?;
                }
            }
            Mobility::Backstep { launched: true } | Mobility::Landing { .. } => {}
        }

        // A queued attack can replace this task. Its controller owns further movement.
        if !matches!(self.runtime[index].task(), ActorTask::Mobility(_)) {
            return Ok(false);
        }
        if matches!(next, Mobility::Jump { .. } | Mobility::Breakfall)
            && self.actors[index].movement.vertical <= 0.
        {
            self.model_requests.push(ModelRequest::Common {
                actor,
                pose: CommonPose::Falling,
            });
        }
        let activity = self.activity(actor);
        let recoil = matches!(next, Mobility::Backstep { .. });
        let owner = &mut self.actors[index];
        let direction = if recoil {
            owner.reaction.direction
        } else {
            owner.movement.direction
        };
        owner
            .movement
            .integrate_along(&mut owner.position, direction);
        let stopped = owner.movement.brake(owner.position[1], activity);
        crate::movement::floor(owner);
        let direction = owner.facing_direction;
        crate::control::face(owner, direction, turn_speed);
        let landed = !owner.airborne() && owner.movement.vertical <= 0.;
        let breakfall_landed = !owner.needs_landing() && owner.movement.vertical <= 0.;

        let finished = match next {
            Mobility::Jump { .. } | Mobility::Breakfall
                if (next == Mobility::Breakfall && breakfall_landed) || landed =>
            {
                let breakfall = next == Mobility::Breakfall;
                self.model_requests.push(ModelRequest::Common {
                    actor,
                    pose: CommonPose::Landing,
                });
                if let Some(control) = &mut self.runtime[index].control {
                    control.jump_charge = Default::default();
                }
                let owner = &mut self.actors[index];

                owner.movement.gravity = if owner.movement.flying {
                    0.
                } else {
                    crate::movement::GRAVITY
                };
                if breakfall {
                    self.clear_special_guard(actor);
                    self.clear_special_guard_pending(actor);
                }
                next = Mobility::Landing {
                    ticks: if self.actors[index].equipment.combo_traits.landing {
                        QUICK_LANDING_TICKS
                    } else {
                        LANDING_TICKS
                    },
                    breakfall,
                };
                false
            }
            Mobility::Backstep { .. } => {
                if landed {
                    self.model_requests.push(ModelRequest::Common {
                        actor,
                        pose: CommonPose::Landing,
                    });
                }
                landed && stopped
            }
            Mobility::Landing { ticks, breakfall } => {
                next = Mobility::Landing {
                    ticks: ticks.saturating_sub(1),
                    breakfall,
                };
                ticks <= 1 && (landed || breakfall && breakfall_landed)
            }
            _ => false,
        };
        if finished {
            self.finish_mobility(
                actor,
                matches!(
                    next,
                    Mobility::Landing {
                        breakfall: true,
                        ..
                    }
                ),
            );
        } else {
            self.set_task(index, ActorTask::Mobility(next));
        }
        Ok(true)
    }

    fn finish_mobility(&mut self, actor: ActorId, breakfall: bool) {
        let index = actor.index();
        self.actors[index].movement.direction = self.actors[index].facing_direction;
        if breakfall {
            self.model_requests
                .push(crate::ModelRequest::PrimaryWeapons {
                    actor,
                    visible: true,
                });
        }
        self.enter_idle(actor);
    }
}

#[cfg(test)]
pub(crate) mod tests;
