use crate::conditions::Condition;
use crate::{
    ActorId, Battle, Control, ControlDefinition, ControlInput, Cue, Rejection,
    action_selection::{ActionCandidate, GroundMotion, InputIntent},
    control::{
        Locomotion, MOVEMENT_STICK_THRESHOLD, NormalAttack, RUN_FACING_MARGIN, RUN_FLICK_THRESHOLD,
        RUN_STOP_AFTER_TICKS, WALK_FACING_MARGIN,
    },
};

impl Battle {
    pub(crate) fn project_ground_command(
        &mut self,
        definition: &ControlDefinition,
        input: ControlInput,
        stopping: bool,
        cues: &mut Vec<Cue>,
    ) {
        let actor = input.actor;
        let index = actor.index();
        let mut intent = InputIntent::default();
        if self.actors[index].airborne() && !self.actors[index].movement.flying {
            self.actors[index].input = intent;
            return;
        }
        if input.taunt.pressed
            && self.actors[index].equipment.taunt_enabled
            && self.unison_gauge < crate::MAX_UNISON_GAUGE
        {
            intent.motion = GroundMotion::Taunt;
        } else {
            if let Some(candidate) = self.player_action_candidate(
                definition,
                input,
                self.runtime[index].combo.history.is_empty(),
                cues,
            ) {
                intent.action = Some(candidate);
            }
            if intent.action.is_some() {
                if self.actors[index].control == Control::SemiAuto
                    && self.actors[index].movement.locomotion == crate::control::Locomotion::Run
                    && crate::distance::dot(
                        self.actors[index].facing_direction,
                        self.actors[index].movement.target_direction,
                    ) < 0.
                {
                    intent.motion = GroundMotion::Stop;
                }
            } else if input.guard.held {
                intent.motion = GroundMotion::Guard;
            } else {
                intent.motion = self.locomotion_intent(index, input.stick[0], stopping);
            }
            let control = self.runtime[index].control.as_mut().unwrap();
            intent.jump = control.jump_charge.sample(
                self.actors[index].control,
                input.stick[1],
                input.guard.held,
                false,
            );
        }
        self.actors[index].input = intent;
    }

    pub(crate) fn project_moving_command(
        &mut self,
        definition: &ControlDefinition,
        input: ControlInput,
        cues: &mut Vec<Cue>,
    ) {
        let actor = input.actor;
        let index = actor.index();
        let delta = self.ground_screen_delta(index, self.runtime[index].target.index());
        let away = if delta < 0. {
            input.horizontal_pressed > 0
        } else {
            input.horizontal_pressed < 0
        };
        let mut intent = InputIntent::default();
        if input.guard.held {
            intent.motion = GroundMotion::Guard;
        } else if away {
            intent.motion = GroundMotion::Stop;
        } else if input.technique.pressed {
            let direction = NormalAttack::from_input(input.stick, false) as usize;
            let shortcut = self.runtime[index].control.as_ref().unwrap().shortcuts[direction];
            intent.action = if let Some(binding) = self.shortcut_action(actor, shortcut) {
                self.project_local_technique(actor, binding, cues)
            } else {
                let normal = definition.normals[direction];
                self.action_candidate(actor, normal.action, [0., normal.reach])
                    .ok()
            };
        }
        let control = self.runtime[index].control.as_mut().unwrap();
        intent.jump = control.jump_charge.sample(
            self.actors[index].control,
            input.stick[1],
            input.guard.held,
            self.actors[index]
                .conditions
                .effective()
                .contains(Condition::Heavy),
        );
        self.actors[index].input = intent;
    }

    pub(crate) fn player_action_candidate(
        &self,
        definition: &ControlDefinition,
        input: ControlInput,
        allow_normal: bool,
        cues: &mut Vec<Cue>,
    ) -> Option<ActionCandidate> {
        let actor = input.actor;
        let control = self.runtime[actor.index()].control.as_ref().unwrap();
        let direction = NormalAttack::from_input(input.stick, false) as usize;
        let technique = if input.technique.pressed {
            self.shortcut_action(actor, control.shortcuts[direction])
        } else {
            input
                .assist
                .iter()
                .zip(control.assist_shortcuts)
                .rev()
                .find_map(|(button, binding)| {
                    binding
                        .filter(|(recipient, _)| button.pressed && *recipient == actor)
                        .map(|(_, binding)| binding)
                })
        };
        if let Some(action) = technique {
            self.project_local_technique(actor, action, cues)
        } else if input.attack.pressed && !input.technique.pressed && allow_normal {
            let direction =
                NormalAttack::from_input(input.stick, self.actors[actor.index()].airborne())
                    as usize;
            let normal = definition.normals[direction];
            self.action_candidate(actor, normal.action, [0., normal.reach])
                .ok()
        } else {
            None
        }
    }

    pub(crate) fn project_local_technique(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
        cues: &mut Vec<Cue>,
    ) -> Option<ActionCandidate> {
        let result = self
            .learned_technique(actor, action)
            .ok_or(Rejection::Unavailable)
            .and_then(|row| self.action_candidate(actor, action, row.player_range));
        match result {
            Ok(candidate) => Some(candidate),
            Err(reason) => {
                cues.push(Cue::Rejected { actor, reason });
                None
            }
        }
    }

    fn ground_screen_delta(&self, actor: usize, target: usize) -> f32 {
        self.camera.as_ref().map_or(
            self.actors[target].position[0] - self.actors[actor].position[0],
            |camera| {
                let project = |index: usize| {
                    crate::project_screen_x(
                        camera.pose,
                        [
                            self.actors[index].position[0],
                            0.,
                            self.actors[index].position[2],
                        ],
                    )
                };
                project(target) - project(actor)
            },
        )
    }

    pub(crate) fn locomotion_intent(
        &mut self,
        index: usize,
        stick: i8,
        stopping: bool,
    ) -> GroundMotion {
        let control = self.runtime[index].control.as_ref().unwrap();
        if stopping && !self.actors[index].equipment.quick_turn {
            return GroundMotion::Stop;
        }
        let running = !stopping && self.actors[index].movement.locomotion == Locomotion::Run;
        let has_direction = stick.unsigned_abs() >= MOVEMENT_STICK_THRESHOLD;
        if running && (!has_direction || control.running_left != (stick < 0)) {
            // Keep momentum for this update; the next input may reverse after braking.
            return if control.run_ticks > RUN_STOP_AFTER_TICKS {
                GroundMotion::Stop
            } else {
                GroundMotion::Walk
            };
        }
        if !has_direction {
            return GroundMotion::Idle;
        }
        let flicked = control.horizontal_delta >= RUN_FLICK_THRESHOLD;
        self.project_movement_direction(index, stick, running);
        if running || flicked {
            GroundMotion::Run
        } else {
            GroundMotion::Walk
        }
    }

    pub(crate) fn project_movement_direction(&mut self, index: usize, stick: i8, running: bool) {
        let delta = self.ground_screen_delta(index, self.runtime[index].target.index());
        let threshold = if running {
            RUN_FACING_MARGIN
        } else {
            WALK_FACING_MARGIN
        };
        let actor = &mut self.actors[index];
        if delta.abs() > threshold {
            let sign = delta.signum() * if stick < 0 { -1. } else { 1. };
            actor.movement.direction = actor.movement.target_direction.map(|value| value * sign);
        }
        actor.facing_direction = actor.movement.direction;
        self.runtime[index].control.as_mut().unwrap().running_left = stick < 0;
    }
}
