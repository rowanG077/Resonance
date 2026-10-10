use crate::conditions::Condition;
use crate::state::ActorTask;
use crate::{
    Activity, ActorId, ApproachParameters, Battle, ControlDefinition, ControlInput, Cue,
    action_selection::{GroundMotion, InputIntent},
    control::Locomotion,
};
use anyhow::Result;

impl Battle {
    /// Guard and airborne input; idle locomotion uses the ground command path.
    pub(crate) fn player_control(
        &mut self,
        definition: &ControlDefinition,
        input: ControlInput,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let actor = input.actor;
        let index = actor.index();
        if self.runtime[index].task().mobility().is_some() {
            if self.activity(actor) == Activity::Jumping {
                let motion =
                    if self.actors[index].equipment.control_ex.aerial_guard && input.guard.held {
                        GroundMotion::Guard
                    } else {
                        GroundMotion::Idle
                    };
                self.actors[index].input = InputIntent {
                    motion,
                    ..InputIntent::default()
                };
                if let Some(candidate) = self.player_action_candidate(definition, input, true, cues)
                {
                    self.actors[index].input.action = Some(candidate);
                }
            }
            return Ok(());
        }
        if self.activity(actor) != Activity::Guarding || self.actors[index].hit_stop != 0 {
            return Ok(());
        }
        self.actors[index].movement.locomotion = Locomotion::Action;
        self.runtime[index].control.as_mut().unwrap().run_ticks = 0;
        let commands = !self.actors[index].airborne() || self.actors[index].movement.flying;
        let jump = commands
            && self.runtime[index]
                .control
                .as_mut()
                .unwrap()
                .jump_charge
                .sample(
                    self.actors[index].control,
                    input.stick[1],
                    input.guard.held,
                    false,
                );
        let special = self.prepared.special_guard(actor).filter(|binding| {
            input.guard.held
                && input.vertical_pressed < 0
                && self.special_guard_learned(actor, *binding)
        });
        if let Some(special) = special {
            self.start_special_guard(actor, special, cues)?;
            return Ok(());
        }
        let selected = if commands {
            self.player_action_candidate(
                definition,
                input,
                self.runtime[index].combo.history.is_empty(),
                cues,
            )
        } else {
            None
        };
        if let Some(candidate) = selected {
            let action = candidate.action;
            let [minimum, maximum] = candidate.range;
            self.activate_counter(actor, cues)?;
            self.request_selected_approach(
                actor,
                self.runtime[index].target,
                action,
                ApproachParameters {
                    minimum,
                    maximum,
                    motion: definition.motions.map(|motions| motions.run),
                    motion_rate: 0.5,
                    speed: definition.run_speed,
                    turn_ticks: definition.turn_ticks,
                },
            )?;
        } else if commands
            && !self.actors[index]
                .conditions
                .effective()
                .contains(Condition::Heavy)
            && (jump || self.backstep_requested(index, self.runtime[index].target, true, input))
        {
            self.set_task(
                index,
                ActorTask::Mobility(if jump {
                    crate::mobility::Mobility::jump()
                } else {
                    crate::mobility::Mobility::backstep()
                }),
            );
        } else if !(commands && input.guard.held) && self.actors[index].guard.recovery == 0 {
            self.enter_idle(actor);
        }
        Ok(())
    }
}

impl Battle {
    pub(crate) fn execute_ground_command(
        &mut self,
        actor: ActorId,
        definition: &ControlDefinition,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = actor.index();
        let mut intent = self.actors[index].input;
        if !self.actors[index].needs_landing() {
            self.advance_attack_charge(actor, cues)?;
        }
        if intent.action.is_none() && !self.actors[index].needs_landing() {
            if let Some(locomotion) = intent.motion.locomotion() {
                if locomotion == Locomotion::Stop {
                    intent.jump = false;
                    intent.face_target = false;
                    self.actors[index].input = intent;
                }
                self.apply_locomotion(index, definition, locomotion)?;
            }
            if intent.face_target {
                self.refresh_selector_facing(actor, self.runtime[index].target);
            }
        }
        if intent.motion == GroundMotion::Guard && intent.action.is_none() && !intent.jump {
            self.enter_player_guard(index);
            self.actors[index].input = InputIntent::default();
            self.actors[index].movement.locomotion = Locomotion::Action;
            self.runtime[index].control.as_mut().unwrap().run_ticks = 0;
        }
        if let Some(candidate) = intent.action
            && (!self.actors[index].movement.flying || self.actors[index].movement.hover_ready())
        {
            let action = candidate.action;
            let [minimum, maximum] = candidate.range;
            self.begin_approach(
                crate::ActionRequest {
                    actor,
                    target: self.runtime[index].target,
                    action,
                },
                ApproachParameters {
                    minimum,
                    maximum,
                    motion: definition.motions.map(|m| m.run),
                    motion_rate: 0.5,
                    speed: definition.run_speed,
                    turn_ticks: definition.turn_ticks,
                },
                self.actors[index].control != crate::Control::Manual,
            )?;
        }
        if intent.jump
            && (matches!(self.activity(actor), Activity::Idle | Activity::Guarding)
                || self.activity(actor) == Activity::Approaching
                    && self.approach_jump_allowed(index))
            && !self.actors[index]
                .conditions
                .effective()
                .contains(Condition::Heavy)
        {
            if matches!(intent.motion, GroundMotion::Idle | GroundMotion::Walk) {
                self.remember_idle_home(index);
            }
            self.set_task(
                index,
                ActorTask::Mobility(crate::mobility::Mobility::jump()),
            );
            self.actors[index].movement.locomotion = Locomotion::Action;
            self.runtime[index].control.as_mut().unwrap().run_ticks = 0;
            return Ok(());
        }
        if matches!(intent.motion, GroundMotion::Idle | GroundMotion::Walk) {
            self.remember_idle_home(index);
        }
        if intent.motion == GroundMotion::Taunt {
            self.begin_taunt(actor)?;
            return Ok(());
        }
        let direction = self.actors[index].facing_direction;
        crate::control::face(
            &mut self.actors[index],
            direction,
            180. / f32::from(definition.turn_ticks),
        );
        Ok(())
    }
}
