//! Recoil speed initialization (6124C) and delayed release (2FB24).
//! Contact admission, hitstun, controller transitions and model selection are
//! separate from this operation; callers must pass their original response gates.
use crate::Movement;
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RecoilKind {
    #[default]
    Normal,
    Down,
    Launched,
}

/// A contact's selected source impulse and hit-rule operands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecoilRule {
    pub impulse: [f32; 2],
    pub delay: u8,
    pub knock_down: bool,
    pub launch: bool,
    pub lift_guard: bool,
    pub guard_speed: f32,
}

impl Default for RecoilRule {
    fn default() -> Self {
        Self {
            impulse: [0.; 2],
            delay: 0,
            knock_down: false,
            launch: false,
            lift_guard: false,
            guard_speed: 3.,
        }
    }
}

impl RecoilRule {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.impulse.iter().all(|v| v.is_finite()) && self.guard_speed.is_finite(),
            "invalid recoil impulse"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerticalRecoil {
    Unchanged,
    Scale(f32),
    Grounded,
}

/// Prepared actor properties; a grounded actor also cannot launch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecoilProfile {
    pub vertical: VerticalRecoil,
    pub can_knock_down: bool,
    pub can_launch: bool,
    pub clear_pending: bool,
}

impl Default for RecoilProfile {
    fn default() -> Self {
        Self {
            vertical: VerticalRecoil::Unchanged,
            can_knock_down: true,
            can_launch: true,
            clear_pending: false,
        }
    }
}

impl RecoilProfile {
    pub fn validate(&self) -> Result<()> {
        if let VerticalRecoil::Scale(scale) = self.vertical {
            ensure!(scale.is_finite(), "invalid recoil scale");
        }
        Ok(())
    }
}

/// Actor-owned pending motion; it has no action/task lifetime of its own.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Recoil {
    pub kind: RecoilKind,
    pub pending: [f32; 2],
    pub delay: u8,
}

impl Recoil {
    /// Called after the contact's immunity/capture gates. `suppressed` is the
    /// controlled-actor proximity rule, not a global immunity. A permitted knock
    /// down or launch overrides it. The return controls copying the hit direction.
    pub fn start(
        &mut self,
        movement: &mut Movement,
        rule: RecoilRule,
        profile: RecoilProfile,
        guarded: bool,
        suppressed: bool,
    ) -> bool {
        let mut allowed = !suppressed;
        self.kind = RecoilKind::Normal;
        if rule.knock_down && profile.can_knock_down {
            self.kind = RecoilKind::Down;
            allowed = true;
        }
        if rule.launch && profile.can_launch {
            self.kind = RecoilKind::Launched;
            allowed = true;
        }
        self.pending = rule.impulse;
        if !allowed {
            self.pending[0] = 0.;
        }
        self.delay = rule.delay;
        [movement.forward, movement.vertical] = if self.delay == 0 {
            self.pending
        } else {
            [0.; 2]
        };
        match profile.vertical {
            VerticalRecoil::Unchanged => {}
            VerticalRecoil::Scale(scale) => {
                movement.vertical *= scale;
                self.pending[1] *= scale;
            }
            VerticalRecoil::Grounded => {
                movement.vertical = 0.;
                self.pending[1] = 0.;
            }
        }
        if guarded && !rule.lift_guard {
            movement.vertical = 0.;
            if allowed {
                movement.forward = rule.guard_speed;
                self.kind = RecoilKind::Normal;
            }
        }
        if profile.clear_pending {
            // The original clears pending speeds after copying the immediate
            // impulse. It deliberately leaves the live speeds above intact.
            self.pending = [0.; 2];
            self.kind = RecoilKind::Normal;
        }
        allowed
    }

    /// The hurt controller calls this before testing local hit-stop. On the
    /// release update, pending speeds become live even if integration is held.
    pub fn advance_delay(&mut self, movement: &mut Movement) {
        if self.delay != 0 {
            self.delay -= 1;
            if self.delay == 0 {
                [movement.forward, movement.vertical] = self.pending;
            }
        }
    }
}

#[cfg(test)]
mod tests;
