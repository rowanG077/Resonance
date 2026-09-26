//! Actor-owned hurt update and ordinary recovery (2FB24 / 2B18C).
use crate::{Activity, Actor, Control, Recoil, RecoilKind};
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RecoilDirection {
    Travel,
    #[default]
    AwayFromOwner,
    AwayFromContact,
    TowardContact,
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReactionRule {
    pub recoil: crate::RecoilRule,
    pub direction: RecoilDirection,
    pub hitstun: u8,
    pub alternate_motion: bool,
    pub armor_damage: u8,
    pub stun_chance: u8,
    pub stagger: u8,
    pub hits_down: bool,
}

/// Hits received while below the current threshold still deal damage, but do
/// not interrupt the actor. Crossing the threshold exposes the *next* contact.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Armor {
    pub base: u8,
    pub threshold: u8,
    pub received: u8,
}

impl Armor {
    pub(crate) fn reset(&mut self) {
        self.threshold = self.base;
        self.received = 0;
    }
}

impl ReactionRule {
    pub(crate) fn validate(&self) -> Result<()> {
        self.recoil.validate()?;
        ensure!(
            !self.recoil.knock_down && !self.recoil.launch,
            "knockdown controller is not prepared"
        );
        Ok(())
    }

    pub(crate) fn direction(
        &self,
        owner: [f32; 3],
        target: [f32; 3],
        contact: [f32; 3],
        travel: [f32; 3],
    ) -> [f32; 3] {
        let (a, b) = match self.direction {
            RecoilDirection::Travel => (travel, [0.; 3]),
            RecoilDirection::AwayFromOwner => (target, owner),
            RecoilDirection::AwayFromContact => (target, contact),
            RecoilDirection::TowardContact => (contact, target),
            RecoilDirection::None => return [0.; 3],
        };
        crate::distance::planar_direction(a, b, [0.; 3])
    }
}

/// These clocks and impulses outlive the interrupted action and its VM tasks.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reaction {
    pub stun: crate::Stun,
    pub stagger: crate::Stagger,
    pub protection: crate::Protection,
    pub armor: Armor,
    pub recoil: Recoil,
    pub profile: crate::RecoilProfile,
    pub unflinching: bool,
    pub direction: [f32; 3],
    pub remaining: i16,
    pub combo_hits: i32,
    pub combo_damage: i32,
    /// Original 10B1 normal-selection bits, retained through a normal chain and
    /// cleared by ordinary idle initialization (2B18C).
    pub normal_history: u8,
    /// Profile permits normal hurt recovery without first reaching the floor.
    pub recover_in_air: bool,
    /// Pending source state 2; true retains hurt reason 9 for the shorter delay.
    pub idle_initialization: Option<bool>,
}

impl Reaction {
    pub(crate) fn validate(&self) -> Result<()> {
        self.profile.validate()?;
        ensure!(
            self.stun.particle.is_none() && self.stun.pulse <= 2,
            "invalid initial stun state"
        );
        ensure!(
            self.protection.remaining <= i16::MAX as u16,
            "battle protection duration exceeds signed clock"
        );
        ensure!(
            self.direction
                .iter()
                .chain(&self.recoil.pending)
                .all(|v| v.is_finite()),
            "invalid battle reaction motion"
        );
        ensure!(
            self.recoil.kind == RecoilKind::Normal,
            "knockdown controller is not prepared"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactReaction {
    Hurt { alternate: bool },
    Guard,
}

/// Ordinary contact writes, in wrapper/dispatcher order. The contact's later
/// stun, status, audio and effect operations have separate consumers.
pub(crate) fn respond(
    target: &mut Actor,
    rule: ReactionRule,
    hit: crate::HitResult,
    direction: [f32; 3],
) -> Option<ContactReaction> {
    if hit.armored
        || hit.protection != crate::HitProtection::None
        || matches!(
            hit.affinity,
            crate::Affinity::Absorb | crate::Affinity::Immune
        )
    {
        return None;
    }
    let guarded = hit.guard != crate::GuardResult::None;
    if !target.reaction.unflinching {
        if target.reaction.recoil.start(
            &mut target.movement,
            rule.recoil,
            target.reaction.profile,
            guarded,
            false,
        ) {
            target.reaction.direction = direction;
        }
        if !guarded {
            // The wrapper observes the old combo count; the dispatcher adds this hit later.
            let reduction =
                i32::from(rule.hitstun).wrapping_mul(target.reaction.combo_hits >> 1) / 100;
            target.reaction.remaining =
                (i32::from(rule.hitstun).wrapping_sub(reduction) as i16).max(2);
        }
    }
    if !guarded {
        target.reaction.combo_hits = target.reaction.combo_hits.wrapping_add(1);
        target.reaction.combo_damage = target.reaction.combo_damage.wrapping_add(hit.amount);
    }
    let broken = hit.guard == crate::GuardResult::Broken;
    if target.hp <= 0 || target.reaction.unflinching {
        return None;
    }
    if matches!(hit.guard, crate::GuardResult::Blocked { special: true, .. }) {
        return None;
    }
    if guarded && !broken {
        target.activity = Activity::Guarding;
        target.guard.auto_chance = 100;
        target.reaction.remaining = 30;
        target.movement.braking = 0.55;
        // The contact tail overwrites 2A540's flying gravity with -1 (3CEC8).
        target.movement.gravity = -1.;
        return Some(ContactReaction::Guard);
    }
    if broken {
        target.reaction.remaining = 45;
    }
    target.reaction.protection = Default::default();
    target.reaction.stagger.initialized = false;
    target.activity = Activity::Hurt;
    target.body.jitter.stop();
    // 2A710 keeps recoil1944 separate, copying cached facing18E4 into18F0.
    target.movement.direction = target.facing_direction;
    target.hit_stop = 0;
    target.movement.acceleration = 0.;
    target.movement.gravity = -1.;
    target.movement.braking = 0.275;
    Some(ContactReaction::Hurt {
        alternate: broken || rule.alternate_motion,
    })
}

pub(crate) fn advance_hurt(
    actor: &mut Actor,
    random: &mut crate::state::Random,
    ledger: &mut crate::Ledger,
) {
    // The delay releases even during local hit-stop. Floor correction and the
    // signed countdown also continue when either gate skips movement.
    actor.reaction.recoil.advance_delay(&mut actor.movement);
    if actor.reaction.recoil.delay == 0 && actor.hit_stop == 0 {
        actor
            .movement
            .integrate_along(&mut actor.position, [0.; 2], actor.reaction.direction);
        actor
            .movement
            .brake(actor.position[1], Activity::Hurt, false, false);
        let landed = actor.position[1] <= 0.1 && actor.movement.vertical <= 0.;
        if (landed || actor.reaction.recover_in_air) && actor.reaction.remaining <= 0 {
            recover(actor, random, ledger);
        }
    }
    crate::movement::floor(actor);
    // This also decrements the value written by recovery on the transition visit.
    actor.reaction.remaining = actor.reaction.remaining.wrapping_sub(1);
}

/// Ordinary automatic/enemy guard (2F284). Automatic follow-up selection and held
/// guard/counter inputs have their own branches; they do not use this countdown.
/// Return the requested body motion, sampled on the next model visit.
pub(crate) fn advance_guard(
    actor: &mut Actor,
    random: &mut crate::state::Random,
    ledger: &mut crate::Ledger,
) -> Option<bool> {
    actor.guard.active = true;
    let mut motion = (actor.position[1] <= 0.1).then_some(false);
    if actor.hit_stop == 0 {
        if actor.reaction.remaining > 0 {
            // 2A540(mode=0) clears the contact's follow-up flag on this first
            // update. Expiry therefore takes ordinary recovery, not 298C0.
            actor.guard.auto_chance = 100;
            actor.movement.gravity = -f32::from(u8::from(!actor.movement.flying));
            actor.movement.braking = 0.55;
            motion = Some(actor.position[1] > 0.1 && !actor.movement.flying);
        } else {
            recover(actor, random, ledger);
            actor.guard.active = false;
        }
        if actor.reaction.remaining != 0 {
            actor.reaction.remaining = actor.reaction.remaining.wrapping_sub(1);
        }
        // Unlike hurt recovery, guard recovery precedes this visit's integration.
        actor
            .movement
            .integrate_along(&mut actor.position, [0.; 2], actor.reaction.direction);
        actor
            .movement
            .brake(actor.position[1], actor.activity, false, false);
    }
    crate::movement::floor(actor);
    motion
}

/// Shared ordinary recovery writes (2B18C). Each caller owns the timing of its
/// integration and countdown around this transition.
pub(crate) fn recover(
    actor: &mut Actor,
    random: &mut crate::state::Random,
    ledger: &mut crate::Ledger,
) {
    actor.reaction.idle_initialization = match actor.activity {
        Activity::Guarding => None,
        Activity::Hurt | Activity::KnockedDown | Activity::GettingUp | Activity::Stunned => {
            Some(true)
        }
        _ => Some(false),
    };
    actor.movement.reset_hover();
    actor.movement.steering.returning = None;
    actor.reset_hud_targets();
    actor.reaction.armor.reset();
    actor.reaction.stagger.received = 0;
    actor.reaction.stagger.initialized = false;
    actor.guard.reset_auto_chance(75, random);
    actor.body.jitter.stop();
    actor.guard.pressure = 0;
    actor.activity = Activity::Idle;
    actor.reaction.remaining = if matches!(actor.control, Control::Manual | Control::SemiAuto) {
        30
    } else {
        0
    };
    ledger.record_combo_damage(actor.reaction.combo_damage);
    actor.reaction.combo_hits = 0;
    actor.reaction.combo_damage = 0;
    actor.reaction.normal_history = 0;
    actor.movement.forward = 0.;
    actor.movement.vertical = 0.;
    actor.movement.acceleration = 0.;
    // The original multiplies -1 by the non-flying flag, retaining -0.
    actor.movement.gravity = -f32::from(u8::from(!actor.movement.flying));
    actor.movement.braking = 0.55;
}

#[cfg(test)]
mod tests;
