//! Actor movement, gravity, and braking.
use crate::{
    Activity, Actor, ActorId, Battle, Cue, PreparedBattle,
    conditions::{Condition, ConditionSet},
};
use anyhow::{Context, Result, ensure};

pub(crate) const RUN_ACCELERATION: f32 = 0.5;
pub(crate) const GROUND_TOLERANCE: f32 = 0.1;
pub(crate) const GRAVITY: f32 = -1.;

/// Ordinary floating actors settle before selecting another action. Bobbing
/// uses the shared actor-common clock, independently of action and motion clocks.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Hover {
    settled: bool,
    phase: u16,
    bobbing: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Floor {
    armed: bool,
    landing: Option<[f32; 3]>,
}

/// Actor-local motion, independent of action and animation clocks. Loading supplies
/// profile flags and initial coefficients; actions change speed and direction.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Movement {
    pub locomotion: crate::control::Locomotion,
    pub direction: [f32; 3],
    /// Direction toward the current opponent, independent of movement and desired facing.
    pub target_direction: [f32; 3],
    pub forward: f32,
    pub vertical: f32,
    pub acceleration: f32,
    pub gravity: f32,
    pub braking: f32,
    pub flying: bool,
    pub fixed_height: bool,
    pub turning_disabled: bool,
    pub hover_height: f32,
    pub hover: Hover,
    pub floor: Floor,
    /// Attack classification retained through the final landing contacts.
    pub airborne_action: bool,
    pub steering: crate::Steering,
}

impl crate::Actor {
    /// Halve walk speed before body scale and strength.
    pub(crate) fn walk_speed(&self, profile: f32, scale: f32, conditions: ConditionSet) -> f32 {
        let profile = if conditions.contains(Condition::Heavy) {
            profile * 0.5
        } else {
            profile
        };
        let bonus = f32::from(u8::from(conditions.contains(Condition::MovementBoost)));
        (profile * scale) * 0.1_f32.mul_add(bonus, self.equipment.speed_multiplier)
    }

    /// Keep acceleration at 0.5 while scaling only the live speed ceiling.
    pub(crate) fn run_limit(&self, profile: f32, conditions: ConditionSet) -> f32 {
        let bonus = f32::from(u8::from(conditions.contains(Condition::MovementBoost)));
        let speed = profile * 0.2_f32.mul_add(bonus, self.equipment.speed_multiplier);
        if conditions.contains(Condition::Heavy) {
            speed * 0.5
        } else {
            speed
        }
    }

    /// Bind animation speed when entering the movement state.
    pub(crate) fn motion_rate(&self, base: f32, conditions: ConditionSet) -> f32 {
        let bonus = f32::from(u8::from(conditions.contains(Condition::MovementBoost)));
        let rate = base * 0.1_f32.mul_add(bonus, self.equipment.speed_multiplier);
        if conditions.contains(Condition::Heavy) {
            rate * 0.5
        } else {
            rate
        }
    }
}

impl Movement {
    pub fn hover_ready(&self) -> bool {
        !self.flying || self.hover.settled
    }

    /// Shared ordinary recovery resets settling without changing profile flags.
    pub(crate) fn reset_hover(&mut self) {
        self.hover.settled = false;
        self.hover.phase = 0;
    }

    /// After integration, compare the new vertical speed for capture; gravity changes the next
    /// update.
    pub(crate) fn settle_hover(&mut self, height: &mut f32) {
        if !self.flying || self.hover.settled {
            return;
        }
        if (*height - self.hover_height).abs() < self.vertical.abs() {
            self.hover.settled = true;
            *height = self.hover_height;
            self.vertical = 0.;
            self.gravity = 0.;
        } else if *height > self.hover_height {
            self.vertical = -2.5;
            self.gravity = 0.;
        } else {
            self.gravity += 0.01;
            if self.vertical > 6. {
                self.vertical = 6.;
            }
        }
    }

    pub(crate) fn advance_hover_phase(&mut self) {
        if self.hover.settled {
            self.hover.phase = (self.hover.phase + 1) % 360;
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.direction
                .iter()
                .chain(&self.target_direction)
                .chain([
                    &self.forward,
                    &self.vertical,
                    &self.acceleration,
                    &self.gravity,
                    &self.hover_height,
                    &self.braking
                ])
                .all(|v| v.is_finite())
                && self.braking >= 0.,
            "invalid battle movement"
        );
        Ok(())
    }

    pub(crate) fn integrate(&mut self, position: &mut [f32; 3]) {
        self.integrate_along(position, self.direction);
    }

    /// Hurt controllers retain a recoil direction independently of action motion.
    pub(crate) fn integrate_along(&mut self, position: &mut [f32; 3], direction: [f32; 3]) {
        position[0] += direction[0] * self.forward;
        if !self.fixed_height {
            position[1] += self.vertical;
        }
        position[2] += direction[2] * self.forward;
        self.forward += self.acceleration;
        self.vertical += self.gravity;
    }

    pub(crate) fn brake(&mut self, height: f32, activity: Activity) -> bool {
        if self.flying && activity == Activity::Hurt && height > GROUND_TOLERANCE {
            return false;
        }
        let grounded = self.flying || height <= GROUND_TOLERANCE;
        if grounded {
            self.forward = self.forward.signum() * (self.forward.abs() - self.braking).max(0.);
        }
        self.forward == 0.
    }
}

/// At exactly floor height, retain vertical velocity until a later update crosses below.
pub(crate) fn floor(actor: &mut Actor) {
    if actor.position[1] <= 0. {
        if actor.movement.floor.armed {
            actor.movement.floor.armed = false;
            actor.movement.floor.landing = Some(actor.position);
        }
    } else if actor.position[1] > 5. {
        actor.movement.floor.armed = true;
    }
    if actor.position[1] < 0. {
        actor.position[1] = 0.;
        actor.movement.vertical = 0.;
    }
}

impl PreparedBattle {
    /// Enable a small idle bob for floating actors.
    pub fn with_hover_bobbing(mut self, actors: Vec<ActorId>) -> Result<Self> {
        for actor in actors {
            let actor = self
                .actors
                .get_mut(actor.index())
                .context("invalid bobbing actor")?;
            actor.movement.hover.bobbing = true;
        }
        Ok(self)
    }
}

impl Battle {
    pub(crate) fn advance_hover(&mut self, index: usize, bob: bool) -> Result<()> {
        let actor = &mut self.actors[index];
        actor.movement.settle_hover(&mut actor.position[1]);
        let hover = &actor.movement.hover;
        if bob && hover.settled && hover.bobbing {
            let phase = f32::from(hover.phase) * 4.;
            actor.position[1] = actor.movement.hover_height + 0.5 * phase.to_radians().sin();
        }
        Ok(())
    }

    pub(crate) fn publish_landing(&mut self, index: usize, cues: &mut Vec<Cue>) {
        if let Some(position) = self.actors[index].movement.floor.landing.take() {
            cues.push(Cue::Landed {
                actor: ActorId(index as u8),
                position,
            });
        }
    }
}

#[cfg(test)]
mod hover_tests;
#[cfg(test)]
mod tests;
