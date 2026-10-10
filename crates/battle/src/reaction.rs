use crate::state::ActorTask;
use crate::{
    Activity, Actor, ActorId, Battle, Control, Recoil, RecoilKind, Side, mobility::Mobility,
};
use anyhow::{Result, ensure};

pub use resonance_content::battle_projectile::RecoilDirection;

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContactTraits {
    pub technique_balance: i8,
    pub endure: bool,
    pub hard_hit: bool,
    pub air_brake: bool,
}

/// These clocks and impulses outlive the interrupted action.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reaction {
    pub(crate) contributor_hits: [u8; 12],
    pub stun: crate::Stun,
    pub stagger: crate::Stagger,
    pub protection: crate::Protection,
    pub armor: Armor,
    pub recoil: Recoil,
    pub profile: crate::RecoilProfile,
    pub direction: [f32; 3],
    pub combo_hits: i32,
    pub combo_damage: i32,
    pub(crate) contact_recovery_sampled: bool,
    pub(crate) contact_recovery_command: bool,
    /// Profile permits normal hurt recovery without first reaching the floor.
    pub recover_in_air: bool,
    pub(crate) spell_revenge_used: bool,
}

impl Reaction {
    pub(crate) fn validate(&self) -> Result<()> {
        self.profile.validate()?;
        ensure!(
            self.direction
                .iter()
                .chain(&self.recoil.pending)
                .all(|v| v.is_finite()),
            "invalid battle reaction motion"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactReaction {
    Hurt { alternate: bool, remaining: u32 },
    Guard,
}

impl Battle {
    pub(crate) fn suppress_recoil(
        &self,
        attacker: ActorId,
        target: ActorId,
        suppression_distance: f32,
    ) -> bool {
        if self.actors[target.index()].reaction.recoil.kind != RecoilKind::Normal {
            return false;
        }
        let Some(leader) = self
            .actors
            .iter()
            .position(|actor| actor.side == Side::Party && actor.control != Control::Auto)
        else {
            return false;
        };
        if !self.actors[leader].available() {
            return false;
        }
        let leader = ActorId(leader as u8);
        let selected = self.target(leader);
        let other = if attacker != leader && selected == Some(target) {
            target
        } else if target == leader && selected != Some(attacker) {
            attacker
        } else {
            return false;
        };
        let a = self.actors[leader.index()].position;
        let b = self.actors[other.index()].position;
        crate::distance::length([a[0] - b[0], 0., a[2] - b[2]]) < suppression_distance
    }
}

/// Resolve a contact into a reaction; the battle installs its owning task.
pub(crate) fn respond(
    owner: &Actor,
    target: &mut Actor,
    rule: ReactionRule,
    hit: crate::HitResult,
    direction: [f32; 3],
    suppressed: bool,
) -> Option<ContactReaction> {
    const GUARD_BREAK_TICKS: u32 = 45;
    if hit.suppresses_reaction() {
        return None;
    }
    let guarded = hit.guard != crate::GuardResult::None;
    if target.time_stop == 0
        && target.reaction.recoil.start(
            &mut target.movement,
            rule.recoil,
            target.reaction.profile,
            guarded,
            suppressed,
        )
    {
        target.reaction.direction = direction;
    }
    let remaining = if hit.guard == crate::GuardResult::Broken {
        GUARD_BREAK_TICKS
    } else {
        hitstun(owner, target, rule.hitstun)
    };
    if !guarded {
        target.reaction.combo_hits = target.reaction.combo_hits.saturating_add(1);
        target.reaction.combo_damage = target
            .reaction
            .combo_damage
            .saturating_add(hit.amount.max(0));
    }
    if target.hp <= 0
        || target.time_stop != 0
        || matches!(hit.guard, crate::GuardResult::Blocked { special: true, .. })
    {
        return None;
    }
    if matches!(hit.guard, crate::GuardResult::Blocked { .. }) {
        target.guard.auto_chance = 100;
        target.guard.recovery = crate::guard::BLOCK_RECOVERY_TICKS;
        target.movement.braking = 0.55;
        target.movement.gravity = crate::movement::GRAVITY;
        return Some(ContactReaction::Guard);
    }
    Some(ContactReaction::Hurt {
        alternate: hit.guard == crate::GuardResult::Broken || rule.alternate_motion,
        remaining,
    })
}

fn hitstun(owner: &Actor, target: &Actor, base: u8) -> u32 {
    let mut duration = i64::from(base);
    if owner.side == Side::Party && owner.equipment.contact.technique_balance <= -100 {
        duration += 5;
    }
    if target.equipment.contact.endure {
        duration -= 5;
    }
    if if owner.movement.airborne_action {
        owner.equipment.contact.air_brake
    } else {
        owner.equipment.contact.hard_hit
    } {
        duration += 10;
    }
    let reduction = i64::from(base) * i64::from(target.reaction.combo_hits.max(0) / 2) / 100;
    (duration - reduction).max(2) as u32
}

pub(crate) fn enter_hurt(target: &mut Actor) {
    target.reaction.protection = Default::default();
    target.reaction.stagger.initialized = false;
    target.reaction.contact_recovery_sampled = false;
    target.reaction.contact_recovery_command = false;
    target.guard.active = false;
    target.guard.recovery = 0;
    target.guard.recent_hurt_ticks = 60;
    target.movement.direction = target.facing_direction;
    target.hit_stop = 0;
    target.movement.acceleration = 0.;
    target.movement.gravity = crate::movement::GRAVITY;
    target.movement.braking = 0.275;
}

impl Battle {
    pub(crate) fn begin_hurt(
        &mut self,
        actor: ActorId,
        remaining: u32,
        cues: &mut Vec<crate::Cue>,
    ) {
        self.interrupt_actor(actor, cues);
        self.clear_technique_command(actor);
        enter_hurt(&mut self.actors[actor.index()]);
        self.set_task(actor.index(), ActorTask::Hurt { remaining });
    }

    /// A failed paralysis check stalls this command without paying its cost.
    pub(crate) fn begin_paralysis(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<()> {
        const PARALYSIS_TICKS: u8 = 60;
        let index = actor.index();
        self.interrupt_actor(actor, cues);
        let owner = &mut self.actors[index];
        owner.reaction.recoil = Default::default();
        owner.guard.active = false;
        owner.movement.forward = 0.;
        owner.movement.acceleration = 0.;
        owner.movement.gravity = if owner.movement.flying {
            0.
        } else {
            crate::movement::GRAVITY
        };
        self.set_task(
            index,
            ActorTask::Paralysis {
                remaining: PARALYSIS_TICKS,
            },
        );
        cues.push(crate::Cue::Paralyzed { actor });
        self.model_requests.push(crate::ModelRequest::Hurt {
            actor,
            alternate: true,
        });
        Ok(())
    }

    pub(crate) fn advance_paralysis(&mut self, actor: ActorId, remaining: u8) -> Result<bool> {
        let index = actor.index();
        if self.actors[index].hit_stop > 0 {
            return Ok(true);
        }
        let owner = &mut self.actors[index];
        owner.movement.integrate(&mut owner.position);
        crate::movement::floor(owner);
        if remaining <= 1 {
            self.enter_idle(actor);
        } else {
            self.set_task(
                index,
                ActorTask::Paralysis {
                    remaining: remaining - 1,
                },
            );
        }
        Ok(true)
    }

    pub(crate) fn advance_hurt(
        &mut self,
        id: ActorId,
        remaining: u32,
        input: &[crate::ControlInput],
        cues: &mut Vec<crate::Cue>,
    ) -> Result<bool> {
        let index = id.index();
        self.try_spell_revenge(id, input, cues)?;
        let actor = &mut self.actors[index];
        if actor.hit_stop > 0 {
            return Ok(true);
        }
        actor.reaction.recoil.advance_delay(&mut actor.movement);
        if actor.reaction.recoil.delay > 0 {
            return Ok(true);
        }
        let kind = actor.reaction.recoil.kind;
        if self.begin_contact_recovery(id, true, cues)? {
            return Ok(true);
        }
        let actor = &mut self.actors[index];
        actor
            .movement
            .integrate_along(&mut actor.position, actor.reaction.direction);
        actor.movement.brake(actor.position[1], Activity::Hurt);
        let landed = !actor.airborne() && actor.movement.vertical <= 0.;
        crate::movement::floor(actor);
        if (landed || (!kind.contact() && actor.reaction.recover_in_air))
            && (kind.breakfall_allowed() || remaining <= 1)
        {
            self.finish_combo_recovery(id, cues);
            if kind.contact() {
                self.actors[index].reaction.recoil.kind.land();
                self.enter_knockdown(id, cues);
                self.initialize_knockdown_window(index, cues);
            } else {
                self.enter_idle(id);
            }
        } else {
            self.set_task(
                index,
                ActorTask::Hurt {
                    remaining: remaining.saturating_sub(1),
                },
            );
        }
        Ok(true)
    }

    pub(crate) fn enter_idle(&mut self, id: ActorId) {
        let index = id.index();
        let actor = &self.actors[index];
        let mobility = self.runtime[index].task().mobility();
        let after_hit = matches!(
            self.runtime[index].task(),
            ActorTask::Hurt { .. }
                | ActorTask::Paralysis { .. }
                | ActorTask::Knockdown(_)
                | ActorTask::Stunned { .. }
        ) || matches!(
            mobility,
            Some(
                Mobility::Breakfall
                    | Mobility::Landing {
                        breakfall: true,
                        ..
                    }
            )
        );
        let delay = if self.pending_technique(id).is_some() {
            crate::decision::QUEUED_COMMAND_DELAY
        } else if matches!(actor.control, Control::Auto | Control::Enemy)
            && let Some(definition) = self.prepared.actor_setup[index].decision
        {
            let roll = if definition.idle_variation == 0 {
                0
            } else {
                self.random.next_u16()
            };
            definition.idle_delay(roll, after_hit)
        } else {
            0
        };
        let actor = &mut self.actors[index];
        actor.movement.reset_hover();
        actor.control_ex_state.recover();
        actor.proficiency = 0;
        actor.input = Default::default();
        actor.reaction.contact_recovery_command = false;
        actor.reaction.armor.reset();
        actor.reaction.stagger.received = 0;
        actor.reaction.stagger.initialized = false;
        actor.guard.reset_auto_chance(75, &mut self.random);
        actor.guard.pressure = 0;
        actor.guard.active = false;
        actor.guard.kind = crate::GuardKind::Normal;
        actor.guard.recovery = 0;
        self.ledger.record_combo_damage(actor.reaction.combo_damage);
        actor.reaction.contributor_hits = [0; crate::ACTOR_CAPACITY];
        actor.reaction.combo_hits = 0;
        actor.reaction.combo_damage = 0;
        actor.reaction.spell_revenge_used = false;
        actor.movement.forward = 0.;
        actor.movement.vertical = 0.;
        actor.movement.acceleration = 0.;
        actor.movement.gravity = if actor.movement.flying {
            0.
        } else {
            crate::movement::GRAVITY
        };
        actor.movement.braking = 0.55;
        actor.movement.locomotion = crate::Locomotion::Idle;
        let runtime = &mut self.runtime[index];
        runtime.combo = Default::default();
        runtime.idle_timer = delay;
        runtime.special_guard_pending = None;
        if let Some(control) = &mut runtime.control {
            control.cancel_action();
        }
        self.set_task(index, ActorTask::None);
        self.remember_idle_home(index);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "reaction/contact_ex_tests.rs"]
mod contact_ex_tests;
