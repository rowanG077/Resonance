//! Stagger buildup and knockdown recovery.
use crate::state::ActorTask;
use crate::{Activity, Actor, ActorId, Battle, Side};
use anyhow::Result;

/// Brief get-up delay before control resumes, independent of pose length.
pub(crate) const GET_UP_DURATION: u32 = 10;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Recovery {
    Down { remaining: u32 },
    Rising { remaining: u32 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProtectionMode {
    #[default]
    None,
    /// Full damage with ordinary reaction suppression.
    Armor,
    Down,
    Recovery,
    /// Escape protection retains its extra mode flag.
    Escape,
}

/// Protection has its own common timer, independent of the recovery animation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Protection {
    pub mode: ProtectionMode,
    pub remaining: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HitProtection {
    #[default]
    None,
    Armored,
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

impl Protection {
    pub(crate) fn item(&mut self) {
        self.armor(60);
    }

    /// Shared protection for item use and Taunt Guard.
    pub(crate) fn armor(&mut self, duration: u32) {
        // Recovery and escape protection take priority over ordinary armor.
        if !matches!(self.mode, ProtectionMode::Recovery | ProtectionMode::Escape) {
            self.mode = ProtectionMode::Armor;
            self.remaining = self.remaining.max(duration);
        }
    }
    pub(crate) fn recover(&mut self) {
        self.mode = ProtectionMode::Recovery;
        self.remaining = self.remaining.max(120);
    }

    pub(crate) fn escape(&mut self) {
        self.protect_transition(180);
    }

    /// Shared protection for Escape and lethal rescue.
    pub(crate) fn protect_transition(&mut self, duration: u32) {
        self.mode = ProtectionMode::Escape;
        self.remaining = self.remaining.max(duration);
    }

    pub(crate) fn step(&mut self) {
        if self.remaining != 0 {
            self.remaining -= 1;
            if self.remaining == 0 {
                self.mode = ProtectionMode::None;
            }
        }
    }

    /// Return reaction protection and the power-of-two damage reduction.
    pub(crate) fn hit(self, side: Side, window: u8, hits_down: bool) -> (HitProtection, u8) {
        match self.mode {
            ProtectionMode::None => (HitProtection::None, 0),
            ProtectionMode::Armor => (HitProtection::Armored, 0),
            ProtectionMode::Down if window != 0 && hits_down => (HitProtection::None, 0),
            ProtectionMode::Down if window != 0 => (HitProtection::Reduced, 3),
            ProtectionMode::Down if side == Side::Enemy => (HitProtection::Reduced, 2),
            ProtectionMode::Recovery | ProtectionMode::Escape if side == Side::Enemy => {
                (HitProtection::Reduced, 3)
            }
            _ => (HitProtection::Avoided, 0),
        }
    }
}

pub(crate) fn threshold_reached(actor: &Actor) -> bool {
    actor.hp > 0
        && actor.time_stop == 0
        && actor.reaction.stagger.received >= actor.reaction.stagger.threshold
}

/// Automatic breakfall is offered once when grounded or descending from a launch.
pub(crate) fn automatic_breakfall_ready(actor: &Actor, activity: Activity) -> bool {
    actor.reaction.recoil.kind.launching() && actor.movement.vertical <= 0.
        || actor.reaction.recoil.kind == crate::RecoilKind::Down
            && activity == Activity::KnockedDown
            && !actor.airborne()
}

/// Manual controls request recovery with Guard. Autonomous actors try once per
/// eligible contact, with a chance proportional to their remaining health.
pub(crate) fn sample_contact_recovery(
    actor: &mut Actor,
    activity: Activity,
    enabled: bool,
    guard_pressed: bool,
    roll: impl FnOnce() -> u16,
) {
    if !enabled
        || !actor.available()
        || !actor.reaction.recoil.kind.contact()
        || !matches!(activity, Activity::Hurt | Activity::KnockedDown)
    {
        actor.reaction.contact_recovery_command = false;
        return;
    }
    match actor.control {
        crate::Control::Manual | crate::Control::SemiAuto => {
            actor.reaction.contact_recovery_command = guard_pressed;
        }
        crate::Control::Auto | crate::Control::Enemy
            if actor.reaction.recoil.delay == 0
                && !actor.reaction.contact_recovery_sampled
                && automatic_breakfall_ready(actor, activity) =>
        {
            actor.reaction.contact_recovery_command = (roll() % 100) < actor.hp_percent() as u16;
            actor.reaction.contact_recovery_sampled = true;
        }
        _ => {}
    }
}

impl Battle {
    pub(crate) fn enter_knockdown(&mut self, id: ActorId, cues: &mut Vec<crate::Cue>) {
        self.interrupt_actor(id, cues);
        let actor = &mut self.actors[id.index()];

        actor.reaction.protection.mode = ProtectionMode::Down;
        let remaining = u32::from(actor.reaction.stagger.duration);
        self.set_task(
            id.index(),
            ActorTask::Knockdown(Recovery::Down { remaining }),
        );
    }

    pub(crate) fn enter_get_up(&mut self, id: ActorId) {
        self.set_task(
            id.index(),
            ActorTask::Knockdown(Recovery::Rising {
                remaining: GET_UP_DURATION,
            }),
        );
    }

    pub(crate) fn advance_knockdown(
        &mut self,
        id: ActorId,
        recovery: Recovery,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<bool> {
        let index = id.index();
        match recovery {
            Recovery::Rising { remaining } => {
                if remaining <= 1 {
                    self.actors[index].reaction.protection.recover();
                    self.enter_idle(id);
                } else {
                    self.set_task(
                        index,
                        ActorTask::Knockdown(Recovery::Rising {
                            remaining: remaining - 1,
                        }),
                    );
                }
            }
            Recovery::Down { remaining } if !self.actors[index].airborne() => {
                self.initialize_knockdown_window(index, cues);
                self.apply_knockdown_impact(id, cues);
                if !self.begin_contact_recovery(id, false, cues)? {
                    if remaining <= 1 {
                        self.enter_get_up(id);
                    } else {
                        self.set_task(
                            index,
                            ActorTask::Knockdown(Recovery::Down {
                                remaining: remaining - 1,
                            }),
                        );
                    }
                }
            }
            Recovery::Down { .. } => {}
        }
        let activity = self.activity(id);
        let actor = &mut self.actors[index];
        actor
            .movement
            .integrate_along(&mut actor.position, actor.reaction.direction);
        actor.movement.brake(actor.position[1], activity);
        crate::movement::floor(actor);
        Ok(true)
    }
}

#[cfg(test)]
mod tests;

impl Battle {
    /// Controls retain the request; Hurt or Down decides when to consume it.
    pub(crate) fn begin_contact_recovery(
        &mut self,
        id: ActorId,
        from_hurt: bool,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<bool> {
        let index = id.index();
        if !self.actors[index].reaction.contact_recovery_command
            || if from_hurt {
                !self.actors[index].reaction.recoil.kind.contact()
            } else {
                !self.actors[index].reaction.recoil.kind.breakfall_allowed()
            }
        {
            return Ok(false);
        }
        if !self.prepared.actor_setup[index].contact_recovery {
            return Ok(false);
        }
        let actor = &mut self.actors[index];
        // Reset combo without ordinary recovery accounting.
        actor.guard.pressure = 0;
        actor.reaction.combo_hits = 0;
        actor.reaction.combo_damage = 0;
        actor.movement.airborne_action = true;
        actor.reaction.contact_recovery_sampled = false;
        actor.reaction.contact_recovery_command = false;
        actor.movement.vertical = 16.5;
        actor.movement.forward = 5.5;
        actor.movement.direction = actor.reaction.direction;
        actor.reaction.recoil.kind = crate::RecoilKind::Normal;
        actor.reaction.protection = Default::default();
        cues.push(crate::Cue::Breakfall {
            actor: ActorId(index as u8),
        });
        self.set_task(
            index,
            ActorTask::Mobility(crate::mobility::Mobility::Breakfall),
        );
        self.model_requests.push(crate::ModelRequest::Common {
            actor: id,
            pose: crate::CommonPose::Breakfall,
        });
        Ok(true)
    }
}

impl Battle {
    /// Run after impact and before get-up, countdown, and recovery.
    pub(crate) fn apply_knockdown_impact(&mut self, id: ActorId, cues: &mut Vec<crate::Cue>) {
        let index = id.index();
        let actor = &self.actors[index];
        // Down and launched hits retain distinct contact states; only launch triggers an
        // immediate impact.
        if actor.airborne() || !actor.reaction.recoil.kind.launching() {
            return;
        }
        self.actors[index].reaction.recoil.kind.finish_impact();
        let mut damage =
            (i64::from(self.runtime[index].recovery.last_hit_damage) * 25 / 100) as i32;
        if self.actors[index].equipment.control_ex.roll {
            damage >>= 1;
        }
        self.apply_knockdown_incidental(id, damage, cues);
        cues.push(crate::Cue::KnockdownImpact { actor: id });
    }

    fn apply_knockdown_incidental(&mut self, id: ActorId, damage: i32, cues: &mut Vec<crate::Cue>) {
        let index = id.index();
        let actor = &mut self.actors[index];
        if damage != 0 {
            actor.hp = actor.hp.saturating_sub(damage.max(0)).max(1);
            self.runtime[index].recovery.last_hit_damage = damage;
            self.runtime[index].recovery.last_hit_recovery = damage >> 1;
            cues.push(crate::Cue::IncidentalDamage {
                actor: id,
                amount: damage,
            });
        }
        // Apply the recovery finish flag even when the nominal amount is zero.
        actor.hp = actor.hp.max(1);
    }
}
