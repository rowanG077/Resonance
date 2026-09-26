//! Stagger buildup and ordinary down/get-up recovery (3BDF8 / 2EB74 / 2E790).
use crate::{Activity, Actor, ActorId, Battle, Side};
use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProtectionMode {
    #[default]
    None,
    Down,
    Recovery,
}

/// Protection has its own common timer, independent of the recovery animation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Protection {
    pub mode: ProtectionMode,
    pub remaining: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HitProtection {
    #[default]
    None,
    Reduced,
    Avoided,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stagger {
    pub threshold: u8,
    pub received: u8,
    pub duration: u8,
    /// Counts down independently after the first grounded down visit.
    pub window: u8,
    pub(crate) initialized: bool,
}

impl Default for Stagger {
    fn default() -> Self {
        Self {
            threshold: u8::MAX,
            received: 0,
            duration: 0,
            window: 0,
            initialized: false,
        }
    }
}

/// Resolved ordinary down and get-up clips. A missing get-up takes the original
/// immediate recovery path. Launch/down impulses have separate motion consumers.
#[derive(Debug, Clone, Copy)]
pub struct KnockdownBinding {
    pub down_motion: u16,
    pub recovery_motion: Option<u16>,
}

impl Protection {
    pub(crate) fn recover(&mut self) {
        self.mode = ProtectionMode::Recovery;
        self.remaining = self.remaining.max(120);
    }

    pub(crate) fn step(&mut self) {
        if self.remaining != 0 {
            self.remaining -= 1;
            if self.remaining == 0 {
                self.mode = ProtectionMode::None;
            }
        }
    }

    /// 629F4: return reaction suppression and the original signed damage shift.
    pub(crate) fn hit(self, side: Side, window: u8, hits_down: bool) -> (HitProtection, u8) {
        match self.mode {
            ProtectionMode::None => (HitProtection::None, 0),
            ProtectionMode::Down if window != 0 && hits_down => (HitProtection::None, 0),
            ProtectionMode::Down if window != 0 => (HitProtection::Reduced, 3),
            ProtectionMode::Down if side == Side::Enemy => (HitProtection::Reduced, 2),
            ProtectionMode::Recovery if side == Side::Enemy => (HitProtection::Reduced, 3),
            _ => (HitProtection::Avoided, 0),
        }
    }
}

pub(crate) fn threshold_reached(actor: &Actor) -> bool {
    !actor.reaction.unflinching
        && actor.reaction.stagger.received >= actor.reaction.stagger.threshold
}

/// Damage resolution has already added the admitted contact's buildup. Reaching
/// the threshold skips the stun roll even when the profile cannot be knocked down.
pub(crate) fn enter(actor: &mut Actor) {
    actor.activity = Activity::KnockedDown;
    actor.reaction.protection.mode = ProtectionMode::Down;
    actor.reaction.remaining = i16::from(actor.reaction.stagger.duration);
    actor.reaction.stagger.received = 0;
}

impl Battle {
    pub(crate) fn advance_knockdown(&mut self, id: ActorId) -> Result<()> {
        let index = id.index();
        let actor = &mut self.actors[index];
        let model = self.models[index]
            .as_mut()
            .context("missing knockdown model")?;
        if actor.activity == Activity::GettingUp {
            if model.finished() && actor.reaction.remaining == 0 {
                actor.reaction.protection.recover();
                crate::reaction::recover(actor, &mut self.random, &mut self.ledger);
            }
            if actor.reaction.remaining != 0 {
                actor.reaction.remaining = actor.reaction.remaining.wrapping_sub(1);
            }
        } else if actor.position[1] <= 0.1 {
            if !actor.reaction.stagger.initialized {
                let window = 60_i32.wrapping_sub(actor.reaction.combo_hits >> 1) as u8;
                actor.reaction.stagger.window = if (window as i8) < 1 { 1 } else { window };
                actor.reaction.stagger.initialized = true;
            }
            model.knockdown_motion(false)?;
            if actor.reaction.remaining <= 0 {
                actor.reaction.remaining = 10;
                if model.knockdown_motion(true)? {
                    actor.activity = Activity::GettingUp;
                } else {
                    actor.reaction.protection.recover();
                    crate::reaction::recover(actor, &mut self.random, &mut self.ledger);
                }
            }
            // 2EB74 observes the newly bound get-up, or the old completed clip
            // on the missing-motion path, before decrementing this signed clock.
            if model.finished() {
                actor.reaction.remaining = actor.reaction.remaining.wrapping_sub(1);
            }
        }
        actor
            .movement
            .integrate_along(&mut actor.position, [0.; 2], actor.reaction.direction);
        actor
            .movement
            .brake(actor.position[1], actor.activity, false, false);
        crate::movement::floor(actor);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
