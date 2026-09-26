//! Grounded player selection and ordinary held guard (32738, 2A540, 2F284).
//! Actions use the shared approach and authored sequence runtime.
use crate::{
    Activity, ApproachParameters, Battle, ControlDefinition, ControlInput, Cue, Rejection,
    control::{Controller, Locomotion, normal_direction},
};
use anyhow::Result;

impl Battle {
    pub(crate) fn player_control(
        &mut self,
        definition: &ControlDefinition,
        control: &mut Controller,
        input: ControlInput,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = definition.actor.index();
        if control.mobility.is_some() {
            control.hold_movement = true;
            // 2EEE4 accepts the ordinary aerial normal after its launch visit.
            if self.actors[index].activity == Activity::Jumping
                && self.actors[index].reaction.remaining > 0
                && input.attack.pressed
            {
                let owner = &mut self.actors[index];
                owner.guard.reset_auto_chance(50, &mut self.random);
                owner.movement.turning_disabled = false;
                let direction = owner.movement.direction;
                crate::control::snap_heading(owner, direction);
                control.jump_charge = Default::default();
                if owner.position[1] > 0.1 {
                    let selection = normal_direction(input.stick, owner.position[1]);
                    control.mobility = None;
                    control.combo = 0;
                    control.attack_target = control.target;
                    owner.activity = Activity::Idle;
                    owner.reaction.remaining = 0;
                    self.start_control_normal(definition, control, selection, cues)?;
                    // 2EEE4 finishes its old callback after298C0 replaces it.
                    // The ordinary action initializer holds later integration.
                    self.integrate_mobility(definition.actor, false)?;
                    self.actors[index].reaction.remaining = 1;
                }
                // On the grounded boundary,2EEE4's following landing branch
                // supersedes the selected normal; the authored task owns it.
            }
            return Ok(());
        }
        let guarding = self.actors[index].activity == Activity::Guarding;
        let commands = self.actors[index].position[1] <= 0.1 || self.actors[index].movement.flying;
        let jump = commands
            && matches!(
                self.actors[index].activity,
                Activity::Idle | Activity::Guarding
            )
            && control.jump_charge.sample(
                self.actors[index].control,
                input.stick[1],
                input.guard.held,
                false,
            );
        if guarding {
            control.hold_movement = true;
            control.locomotion = Locomotion::Action;
            control.run_ticks = 0;
            self.actors[index].guard.active = true;
            if self.actors[index].position[1] <= 0.1
                && let Some(model) = &mut self.models[index]
            {
                model.guard(false)?;
            }
            if self.actors[index].hit_stop != 0 {
                self.face_player_guard(index, definition.turn_ticks);
                return Ok(());
            }
        } else if self.actors[index].activity != Activity::Idle {
            return Ok(());
        }
        let shortcut = definition.shortcuts[usize::from(normal_direction(input.stick, 0.))];
        let action_enabled = commands && (!input.technique.pressed || shortcut.is_some());
        let mut selected = None;
        if action_enabled && input.attack.pressed {
            let normal = definition.normals[usize::from(normal_direction(
                input.stick,
                self.actors[index].position[1],
            ))];
            // 1F548 retains the authored minimum only for Auto control.
            selected = Some((normal.action, 0., normal.reach));
        }
        if commands && input.technique.pressed {
            // Mode1 in 268C4 always uses the four grounded shortcut directions.
            if let Some(shortcut) = shortcut {
                let action = self
                    .prepared
                    .actions
                    .iter()
                    .find(|action| action.id == shortcut.action)
                    .unwrap();
                if self.actors[index].tp >= action.tp_cost {
                    selected = Some((shortcut.action, shortcut.minimum, shortcut.maximum));
                } else {
                    cues.push(Cue::Rejected {
                        actor: definition.actor,
                        reason: Rejection::InsufficientTp,
                    });
                }
            }
        }
        if !guarding && control.locomotion != Locomotion::Stop {
            // 3415C updates the shared count before the ordinary callback.
            // Any selected command resets30; neutral decrements to zero.
            let command = selected.is_some()
                || jump
                || (commands
                    && (input.guard.held
                        || !(-29..=29).contains(&input.stick[0])
                        || control.locomotion == Locomotion::Run));
            let remaining = &mut self.actors[index].reaction.remaining;
            if command {
                *remaining = 30;
            }
            *remaining = remaining.wrapping_sub(1).max(0);
        }
        if let Some((action, minimum, maximum)) = selected {
            if !guarding && matches!(control.locomotion, Locomotion::Idle | Locomotion::Walk) {
                self.remember_idle_home(index);
            }
            if guarding {
                self.actors[index].guard.active = false;
                self.actors[index].reaction.remaining = 0;
                self.actors[index].activity = Activity::Idle;
            }
            if self.request_approach(
                definition.actor,
                control.target,
                action,
                ApproachParameters {
                    minimum,
                    maximum,
                    motion: definition.motions.map(|motions| motions.run),
                    stop_motion: definition.motions.map(|motions| motions.stop),
                    motion_rate: 0.5,
                    speed: definition.run_speed,
                    turn_ticks: definition.turn_ticks,
                },
            )? {
                control.combo = 0;
                control.attack_target = control.target;
                control.locomotion = Locomotion::Action;
                control.run_ticks = 0;
            }
        } else if commands
            && !self.actors[index].movement.mobility_blocked
            && (jump || self.backstep_requested(index, control.target, guarding, input))
        {
            if !guarding {
                self.remember_idle_home(index);
            }
            control.mobility = Some(if jump {
                crate::mobility::Mobility::JumpEntry
            } else {
                crate::mobility::Mobility::BackstepEntry
            });
            control.hold_movement = true;
            control.locomotion = Locomotion::Action;
            control.run_ticks = 0;
            // The authored decision performs this callback's initializer and
            // integration later in this same actor visit.
            return Ok(());
        } else if (commands && input.guard.held)
            || (guarding && self.actors[index].reaction.remaining > 0)
        {
            // 339F8 maps a neutral selector result back to command4 while
            // the guard count is positive; releasing the button is not enough.
            // Callback11 selects no ordinary walk, so horizontal input alone
            // also keeps this latch. Actions and mobility retain priority.
            self.enter_player_guard(index, !guarding)?;
            if !guarding {
                // 2A540 clears command1B1, so 31290 observes low nibble zero.
                self.remember_idle_home(index);
            }
            control.locomotion = Locomotion::Action;
            control.run_ticks = 0;
        } else if guarding {
            self.actors[index].guard.active = false;
            // 2F284 calls2B18C once, switches directly to ordinary idle,
            // then runs its old integration/count tail. The idle body clip
            // is selected by the following ordinary callback.
            self.reset_ordinary_actor(definition.actor);
            control.locomotion = Locomotion::Action;
        } else if commands {
            self.move_control_actor(index, definition, control, input.stick[0], false)?;
            if matches!(control.locomotion, Locomotion::Idle | Locomotion::Walk) {
                self.remember_idle_home(index);
            }
        }
        if guarding {
            if self.actors[index].reaction.remaining != 0 {
                self.actors[index].reaction.remaining =
                    self.actors[index].reaction.remaining.wrapping_sub(1);
            }
            let direction = self.actors[index].reaction.direction;
            self.integrate_player_guard(index, direction);
            self.face_player_guard(index, definition.turn_ticks);
        }
        Ok(())
    }

    fn enter_player_guard(&mut self, index: usize, initial: bool) -> Result<()> {
        let actor = &mut self.actors[index];
        actor.guard.auto_chance = 100;
        actor.reaction.protection = Default::default();
        if initial {
            actor.reaction.direction = actor.movement.direction;
            actor.reaction.remaining = 30;
        }
        if let Some(model) = &mut self.models[index] {
            model.guard(actor.position[1] > 0.1 && !actor.movement.flying)?;
        }
        actor.activity = Activity::Guarding;
        actor.movement.gravity = -f32::from(u8::from(!actor.movement.flying));
        actor.movement.braking = 0.55;
        // 2A540 does not set active; the next 2F284 callback does.
        Ok(())
    }

    fn integrate_player_guard(&mut self, index: usize, direction: [f32; 3]) {
        let actor = &mut self.actors[index];
        actor
            .movement
            .integrate_along(&mut actor.position, [0.; 2], direction);
        actor
            .movement
            .brake(actor.position[1], actor.activity, false, false);
        crate::movement::floor(actor);
    }

    fn face_player_guard(&mut self, index: usize, turn_ticks: u8) {
        let direction = self.actors[index].facing_direction;
        crate::control::face_cached(
            &mut self.actors[index],
            direction,
            180. / f32::from(turn_ticks),
        );
        crate::movement::floor(&mut self.actors[index]);
    }
}
