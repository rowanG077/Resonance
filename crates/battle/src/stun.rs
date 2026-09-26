//! Shared stun contact selection, entry and recovery (3BDF8 / 29330 / 2E848).
use crate::{
    Activity, Actor, ActorId, Battle, Control, Cue, HitResult, ParticleDefinition, ParticleId,
    SoundBinding,
};
use anyhow::{Context, Result};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stun {
    pub chance_bonus: u8,
    pub ex_bonus: bool,
    pub resistance: u8,
    pub immune: bool,
    pub shortened: bool,
    pub(crate) pulse: u8,
    pub(crate) particle: Option<ParticleId>,
}

/// Ready resources selected from the actor profile and common effect bank.
#[derive(Debug, Clone)]
pub struct StunBinding {
    pub particle: Arc<ParticleDefinition>,
    pub head: u16,
    /// Actor-space offset, independent of the head bone's rotation.
    pub offset: [f32; 3],
    pub loop_motion: u16,
    pub down_motion: u16,
    pub recovery_motion: u16,
    pub sound: SoundBinding,
}

pub(crate) fn roll(
    owner: &Actor,
    target: &Actor,
    chance: u8,
    hit: HitResult,
    random: &mut crate::state::Random,
) -> bool {
    if target.hp == 0
        || target.reaction.unflinching
        || hit.armored
        || hit.protection != crate::HitProtection::None
        || matches!(
            hit.affinity,
            crate::Affinity::Absorb | crate::Affinity::Immune
        )
        || matches!(hit.guard, crate::GuardResult::Blocked { .. })
    {
        return false;
    }
    let chance = (i16::from(chance) + i16::from(owner.reaction.stun.chance_bonus)
        - i16::from(target.reaction.stun.resistance))
    .max(0)
        + if owner.reaction.stun.ex_bonus { 5 } else { 0 };
    // Even zero chance and an immune profile consume this unsigned roll.
    random.next() % 100 < chance as u16 && !target.reaction.stun.immune
}

impl Battle {
    pub(crate) fn clear_actor_particles(&mut self, actor: ActorId, cues: &mut Vec<Cue>) {
        self.particles.retain(|&id, particle| {
            let remove = particle.action.is_none() && particle.frame.owner == actor;
            if remove {
                cues.push(Cue::ParticleExpired { particle: id });
            }
            !remove
        });
        self.actors[actor.index()].reaction.stun.particle = None;
    }

    pub(crate) fn enter_stun(&mut self, id: ActorId) -> Result<()> {
        let index = id.index();
        let actor = &mut self.actors[index];
        actor.activity = Activity::Stunned;
        actor.reaction.protection = Default::default();
        actor.reaction.armor.threshold = 0;
        actor.reaction.stun.pulse = 0;
        actor.reaction.remaining = if actor.reaction.stun.shortened {
            90
        } else {
            180
        };
        let model = self.models[index].as_ref().context("missing stun model")?;
        let binding = model
            .definition
            .stun
            .as_ref()
            .context("missing stun resources")?;
        let origin = model.bone_position(binding.head);
        let offset =
            crate::geometry::rotate(binding.offset.map(|v| v * actor.body.scale), actor.heading);
        let particle = self.spawn_particle(binding.particle.clone(), None, id, id, origin, 0.)?;
        if let Some(id) = particle {
            self.particles
                .get_mut(&id)
                .context("missing stun particle")?
                .frame
                .state
                .offset = offset;
        }
        self.actors[index].reaction.stun.particle = particle;
        Ok(())
    }

    pub(crate) fn advance_stun(
        &mut self,
        id: ActorId,
        struggle: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = id.index();
        let actor = &mut self.actors[index];
        let grounded = actor.position[1] <= 0.1;
        if grounded {
            actor.reaction.remaining = actor.reaction.remaining.wrapping_sub(1);
            if struggle && matches!(actor.control, Control::Manual | Control::SemiAuto) {
                actor.reaction.stun.pulse = 2;
            }
        }
        let spin = if actor.reaction.stun.pulse != 0 {
            if actor.reaction.remaining != 0 {
                actor.reaction.remaining = actor.reaction.remaining.wrapping_sub(1);
            }
            actor.reaction.stun.pulse -= 1;
            -7.5
        } else {
            -2.5
        };
        let remaining = actor.reaction.remaining;
        let model = self.models[index].as_ref().context("missing stun model")?;
        let binding = model
            .definition
            .stun
            .as_ref()
            .context("missing stun resources")?;
        let sound = binding.sound;
        if let Some(particle) = actor
            .reaction
            .stun
            .particle
            .and_then(|id| self.particles.get_mut(&id))
        {
            particle.frame.origin = model.bone_position(binding.head);
            let state = &mut particle.frame.state;
            state.offset = crate::geometry::rotate(
                binding.offset.map(|v| v * actor.body.scale),
                actor.heading,
            );
            state.palettes[0] = 17;
            state.angular_velocity[2] = spin;
            let count = (remaining / 45 + 1) as u8;
            state.geometry_count = if count == 3 || count >= 5 { 4 } else { count };
        }
        if remaining == 0 {
            self.clear_actor_particles(id, cues);
        }
        if remaining != 0 && remaining % 40 == 0 {
            cues.push(Cue::Sound {
                actor: id,
                sound,
                position: self.actors[index].position,
                priority: 1,
            });
        }
        let model = self.models[index].as_mut().context("missing stun model")?;
        model.stun_motion(remaining, grounded)?;
        let actor = &mut self.actors[index];
        if remaining <= 0 && model.finished() {
            crate::reaction::recover(actor, &mut self.random, &mut self.ledger);
        }
        // 244D0 and floor correction still run on the recovery visit and during hit-stop.
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
