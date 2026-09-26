//! Ordinary jump and backstep admission (32738). Authored `mobility.sym` owns
//! the callback sequence; Movement remains the only continuous integrator.
use crate::{Activity, ActorId, Battle, Control, ControlInput};
use anyhow::{Context, Result, ensure};

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
        if up <= 48 || restricted {
            self.0 = 0;
        } else if control == Control::Manual || control == Control::SemiAuto && guard {
            if self.0 > 4 {
                self.0 = 0;
                return true;
            }
            self.0 += 1;
        }
        // 33480 skips the counter writes on an unguarded SemiAuto up visit.
        false
    }
}

/// Callback ownership only. Counts live in the shared source1BE clock, and
/// velocities live in Movement; no second animation or physics clock exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mobility {
    JumpEntry,
    Jump,
    BackstepEntry,
    Backstep,
    Landing,
    Finished,
}
impl Mobility {
    pub(crate) fn matches(self, activity: Activity) -> bool {
        match self {
            Self::JumpEntry | Self::BackstepEntry => {
                matches!(activity, Activity::Idle | Activity::Guarding)
            }
            Self::Jump => activity == Activity::Jumping,
            Self::Backstep => activity == Activity::Evading,
            Self::Landing => activity == Activity::Recovering,
            Self::Finished => activity == Activity::Idle,
        }
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
        input.horizontal_pressed > 0 && input.stick[0] >= 30 && owner > target
            || input.horizontal_pressed < 0 && input.stick[0] <= -30 && owner < target
    }

    pub(crate) fn mobility_state(&self, actor: ActorId) -> Vec<i32> {
        let index = actor.index();
        let owner = &self.actors[index];
        let mobility = self.controls[index]
            .as_ref()
            .and_then(|control| control.mobility)
            .filter(|mobility| mobility.matches(owner.activity));
        let (kind, initial) = match mobility {
            Some(Mobility::JumpEntry) => (1, true),
            Some(Mobility::Jump) => (1, false),
            Some(Mobility::BackstepEntry) => (2, true),
            Some(Mobility::Backstep) => (2, false),
            Some(Mobility::Landing) => (3, false),
            _ => (0, false),
        };
        vec![
            kind,
            i32::from(initial),
            i32::from(owner.reaction.remaining),
            owner.position[1].to_bits() as i32,
            owner.movement.vertical.to_bits() as i32,
            i32::from(owner.activity == Activity::Guarding),
        ]
    }

    pub(crate) fn begin_mobility(&mut self, actor: ActorId) -> Result<()> {
        let index = actor.index();
        let control = self.controls[index]
            .as_mut()
            .context("mobility needs player controls")?;
        let owner = &mut self.actors[index];
        match control.mobility {
            Some(Mobility::JumpEntry) => {
                owner.guard.reset_auto_chance(50, &mut self.random);
                owner.activity = Activity::Jumping;
                control.mobility = Some(Mobility::Jump);
            }
            Some(Mobility::BackstepEntry) => {
                owner.movement.direction = owner.movement.target_direction;
                owner.facing_direction = owner.movement.target_direction;
                owner.reaction.direction = owner.movement.target_direction.map(|value| -value);
                owner.movement.turning_disabled = false;
                let direction = owner.movement.direction;
                crate::control::snap_heading(owner, direction);
                owner.activity = Activity::Evading;
                control.mobility = Some(Mobility::Backstep);
            }
            _ => anyhow::bail!("mobility initializer without admission"),
        }
        owner.guard.active = false;
        Ok(())
    }

    pub(crate) fn land_mobility(&mut self, actor: ActorId) -> Result<()> {
        let index = actor.index();
        let control = self.controls[index]
            .as_mut()
            .context("mobility needs player controls")?;
        ensure!(
            control.mobility == Some(Mobility::Jump),
            "landing without an ordinary jump"
        );
        control.jump_charge = Default::default();
        control.mobility = Some(Mobility::Landing);
        self.actors[index].activity = Activity::Recovering;
        Ok(())
    }

    pub(crate) fn finish_mobility(&mut self, actor: ActorId, initialize_idle: bool) -> Result<()> {
        let index = actor.index();
        let control = self.controls[index]
            .as_mut()
            .context("mobility needs player controls")?;
        ensure!(
            control.mobility
                == Some(if initialize_idle {
                    Mobility::Landing
                } else {
                    Mobility::Backstep
                }),
            "mobility recovery does not match its callback"
        );
        let owner = &mut self.actors[index];
        owner.movement.direction = owner.facing_direction;
        crate::reaction::recover(owner, &mut self.random, &mut self.ledger);
        if !initialize_idle {
            // 2E660 explicitly bypasses31B68 and enters ordinary callback3.
            owner.reaction.idle_initialization = None;
        }
        control.mobility = initialize_idle.then_some(Mobility::Finished);
        control.locomotion = crate::control::Locomotion::Idle;
        self.idle_timers[index] = owner.reaction.remaining;
        self.restore_idle_expression(index);
        Ok(())
    }

    pub(crate) fn integrate_mobility(&mut self, actor: ActorId, recoil: bool) -> Result<bool> {
        let index = actor.index();
        let owner = &mut self.actors[index];
        let direction = if recoil {
            owner.reaction.direction
        } else {
            owner.movement.direction
        };
        owner
            .movement
            .integrate_along(&mut owner.position, [0.; 2], direction);
        let stopped = owner
            .movement
            .brake(owner.position[1], owner.activity, false, false);
        self.face_mobility(actor)?;
        Ok(stopped)
    }

    pub(crate) fn face_mobility(&mut self, actor: ActorId) -> Result<()> {
        let index = actor.index();
        let step = 180.
            / f32::from(
                self.prepared.controls[index]
                    .as_ref()
                    .context("mobility needs prepared controls")?
                    .turn_ticks,
            );
        let owner = &mut self.actors[index];
        let direction = owner.facing_direction;
        crate::control::face_cached(owner, direction, step);
        crate::movement::floor(owner);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
