//! Actor integration and braking: original REL 24314 / 244D0 / 24040.
use crate::{Activity, Actor, ActorId, Battle, Cue, EffectAppearance, PreparedBattle};
use anyhow::{Context, Result, ensure};

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
/// profile flags and initial coefficients; scripts change the continuous values.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Movement {
    pub direction: [f32; 3],
    /// Source18FC: direction between the preceding sampled ground centers.
    /// 31C88 refreshes this before replacing the owner's body-center sample.
    pub target_direction: [f32; 3],
    pub forward: f32,
    pub vertical: f32,
    pub acceleration: f32,
    pub gravity: f32,
    pub braking: f32,
    pub flying: bool,
    pub fixed_height: bool,
    /// Combined prepared condition0x200 prevents jump/backstep admission.
    pub mobility_blocked: bool,
    /// Profile flag0x80 suppresses24D24's heading and cached-facing writes.
    pub turning_disabled: bool,
    pub hover_height: f32,
    pub hover: Hover,
    pub floor: Floor,
    /// Original airborne-action flag; the action handles landing before the
    /// floor correction clears it on strictly negative height.
    pub airborne_action: bool,
    pub previous_position: [f32; 3],
    pub previous_velocity: [f32; 2],
    /// Rounded displacement from the most recent integration, before adding it
    /// to the actor's position. Detached contacts can consume this same value.
    pub translation: [f32; 3],
    pub steering: crate::Steering,
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

    /// 23F70 runs after integration: the newly updated vertical speed is used
    /// by the strict capture comparison, while gravity changes next update.
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
                .chain(&self.previous_position)
                .chain(&self.previous_velocity)
                .chain(&self.translation)
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

    /// Root displacement has already been rotated from the sampled model axes.
    pub(crate) fn integrate(&mut self, position: &mut [f32; 3], root: [f32; 2]) {
        self.integrate_along(position, root, self.direction);
    }

    /// Hurt controllers retain a recoil direction independently of action motion.
    pub(crate) fn integrate_along(
        &mut self,
        position: &mut [f32; 3],
        root: [f32; 2],
        direction: [f32; 3],
    ) {
        self.previous_position = [position[0], 0., position[2]];
        self.previous_velocity = [self.forward, self.vertical];
        let x = direction[0] * self.forward;
        let z = direction[2] * self.forward;
        self.translation = [
            root[0] + x,
            if self.fixed_height { 0. } else { self.vertical },
            root[1] + z,
        ];
        for (position, translation) in position.iter_mut().zip(self.translation) {
            *position += translation;
        }
        self.forward += self.acceleration;
        self.vertical += self.gravity;
    }

    pub(crate) fn brake(
        &mut self,
        height: f32,
        activity: Activity,
        stop_reposition: bool,
        unfinished_stop_motion: bool,
    ) -> bool {
        if self.flying && activity == Activity::Hurt && height > 0.1 {
            return false;
        }
        let mut changed = self.flying;
        if self.flying || height <= 0.1 {
            // Two independent tests: an overshoot can enter the second branch.
            if self.forward > 0.55 {
                self.forward -= self.braking;
            }
            if self.forward < -0.55 {
                self.forward += self.braking;
            }
        }
        if !self.flying && stop_reposition {
            self.forward = 0.;
            changed = true;
        }
        if self.forward.abs() <= 0.55 {
            self.forward = 0.;
            changed = true;
        }
        changed && !unfinished_stop_motion
    }
}

/// The floor branch is strict. At exactly zero the vertical accumulator survives
/// until a later update goes below the floor (23AD0).
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
        actor.movement.airborne_action = false;
    }
}

impl PreparedBattle {
    /// Bind the original shared degree-indexed sine table to the actors whose
    /// profiles request ordinary bobbing. No host trigonometry is substituted.
    pub fn with_hover_bobbing(mut self, actors: Vec<ActorId>, sine: &[f32]) -> Result<Self> {
        let sine: [f32; 360] = sine.try_into().context("invalid hover sine table length")?;
        ensure!(
            sine.iter().all(|v| v.is_finite() && v.abs() <= 1.),
            "invalid hover sine table"
        );
        for actor in actors {
            let actor = self
                .actors
                .get_mut(actor.index())
                .context("invalid bobbing actor")?;
            actor.movement.hover.bobbing = true;
        }
        self.hover_sine = Some(Box::new(sine));
        Ok(self)
    }

    pub fn with_landing_effect(mut self, appearance: EffectAppearance) -> Result<Self> {
        ensure!(
            self.effects
                .get(&appearance.resource)
                .is_some_and(|bank| bank.members.contains_key(&appearance.member)),
            "unprepared landing effect"
        );
        self.landing_effect = Some(appearance);
        Ok(self)
    }
}

impl Battle {
    pub(crate) fn advance_hover(&mut self, index: usize, bob: bool) -> Result<()> {
        let actor = &mut self.actors[index];
        actor.movement.settle_hover(&mut actor.position[1]);
        let hover = &actor.movement.hover;
        if bob && hover.settled && hover.bobbing {
            let sine = self
                .prepared
                .hover_sine
                .as_ref()
                .context("unprepared hover sine table")?;
            actor.position[1] =
                0.5_f32.mul_add(sine[usize::from(hover.phase) * 4 % 360], actor.position[1]);
        }
        Ok(())
    }

    pub(crate) fn landing_effect(&mut self, index: usize, cues: &mut Vec<Cue>) -> Result<()> {
        let actor = &mut self.actors[index];
        let Some(origin) = actor.movement.floor.landing.take() else {
            return Ok(());
        };
        let heading = actor.heading;
        let scale = actor.effect_scale;
        let Some(appearance) = self.prepared.landing_effect else {
            return Ok(());
        };
        // Common17 only spawns its prepared particle25; the stored origin keeps
        // the pre-floor position even when a callback already corrected height.
        self.show_effect(
            crate::effect::Spawn {
                action: crate::ActionId(self.next_action),
                scene: None,
                owner: ActorId(index as u8),
                target: ActorId(index as u8),
                appearance,
                origin,
                heading,
                follow: None,
                scale,
                late: false,
                tint: Default::default(),
            },
            cues,
        )
    }
}

#[cfg(test)]
mod hover_tests;
#[cfg(test)]
mod tests;
