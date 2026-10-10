//! Independent cosmetic timelines; emitted particles may outlive their emitter.
use crate::{ActorId, BattleFrame, Cue, EffectAppearance, ParticleId, Random};
use anyhow::{Context as _, Result, ensure};
pub use resonance_content::battle_effect::EffectTint;
use resonance_content::diagnostics::Diagnostics;
use std::{collections::BTreeMap, sync::Arc};
/// Shared animation and rendering budget. Excess cosmetic births are dropped;
/// existing particles keep their lifetime and gameplay is unaffected.
pub const MAX_PARTICLES: usize = 256;

mod scheduler;
pub use scheduler::EffectDefinition;

#[derive(Debug, Clone)]
pub struct EffectBank {
    pub(crate) resource: u32,
    pub(crate) models: BTreeMap<u8, crate::PreparedEffectModel>,
    members: BTreeMap<u16, Arc<crate::EffectDefinition>>,
}

impl EffectBank {
    /// Admit selected effects once, after their model bindings are available.
    pub fn new(
        resource: u32,
        models: BTreeMap<u8, crate::PreparedEffectModel>,
        members: BTreeMap<u16, Arc<crate::EffectDefinition>>,
        diagnostics: &resonance_content::diagnostics::Diagnostics,
    ) -> Result<Self> {
        let mut admitted = BTreeMap::new();
        for (member, definition) in members {
            if diagnostics
                .attempt(
                    &format!("battle effect {resource}:{member}"),
                    definition.validate(&models),
                )?
                .is_some()
            {
                admitted.insert(member, definition);
            }
        }
        Ok(Self {
            resource,
            models,
            members: admitted,
        })
    }

    pub fn member(&self, member: u16) -> Option<&Arc<crate::EffectDefinition>> {
        self.members.get(&member)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectFollow {
    Actor(ActorId),
    Center(ActorId),
    Projectile(crate::ProjectileId),
}
#[derive(Debug, Clone, PartialEq)]
pub struct EffectRequest {
    pub owner: ActorId,
    pub target: ActorId,
    pub appearance: EffectAppearance,
    pub origin: [f32; 3],
    pub heading: f32,
    pub follow: Option<EffectFollow>,
    pub scale: f32,
    pub tint: EffectTint,
}
pub(crate) struct Active {
    definition: Arc<EffectDefinition>,
    actor: ActorId,
    target: ActorId,
    age: u32,
    next: usize,
    origin: [f32; 3],
    heading: f32,
    pub(crate) follow: Option<EffectFollow>,
    scale: f32,
    tint: EffectTint,
}
impl Active {
    fn new(definition: Arc<EffectDefinition>, spawn: &EffectRequest) -> Self {
        Self {
            definition,
            actor: spawn.owner,
            target: spawn.target,
            age: 0,
            next: 0,
            origin: spawn.origin,
            heading: spawn.heading,
            follow: spawn.follow,
            scale: spawn.scale,
            tint: spawn.tint,
        }
    }
}

/// Cosmetic animation owned by the scene, independent of mutable gameplay state.
pub struct Effects {
    pub(crate) banks: BTreeMap<u32, EffectBank>,
    poison: Option<EffectAppearance>,
    pub(crate) emitters: Vec<Active>,
    pub(crate) particles: BTreeMap<ParticleId, crate::particle::Particle>,
    pub(crate) random: Random,
    pub(crate) diagnostics: Diagnostics,
    pub(crate) next_particle: i32,
}

impl EffectFollow {
    pub(crate) fn position(self, frame: &BattleFrame) -> Option<[f32; 3]> {
        match self {
            Self::Actor(id) => frame.actors.get(id.index()).map(|a| a.position),
            Self::Center(id) => frame.actors.get(id.index()).map(|a| a.effect_origin()),
            Self::Projectile(id) => frame
                .projectiles
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.position),
        }
    }
}

impl Effects {
    pub fn new(
        banks: Vec<EffectBank>,
        poison: Option<EffectAppearance>,
        seed: u64,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let mut prepared = BTreeMap::new();
        for bank in banks {
            ensure!(
                prepared.insert(bank.resource, bank).is_none(),
                "duplicate battle effect bank"
            );
        }
        Ok(Self {
            banks: prepared,
            poison,
            emitters: Vec::new(),
            particles: BTreeMap::new(),
            random: Random::new(seed),
            diagnostics,
            next_particle: 1,
        })
    }

    fn poison(&mut self, id: ActorId, frame: &BattleFrame) -> Result<()> {
        let Some(appearance) = self.poison else {
            return Ok(());
        };
        let actor = frame
            .actors
            .get(id.index())
            .context("unknown poison actor")?;
        self.start(
            &EffectRequest {
                owner: id,
                target: id,
                appearance,
                origin: [actor.position[0], actor.body_top(), actor.position[2]],
                heading: actor.heading,
                follow: None,
                scale: 1.,
                tint: Default::default(),
            },
            frame,
        )
    }

    fn start(&mut self, spawn: &EffectRequest, frame: &BattleFrame) -> Result<()> {
        ensure!(
            frame.actors.get(spawn.owner.index()).is_some()
                && frame.actors.get(spawn.target.index()).is_some(),
            "unknown effect actor"
        );
        ensure!(
            spawn.origin.iter().all(|v| v.is_finite())
                && spawn.heading.is_finite()
                && spawn.scale.is_finite()
                && spawn.scale >= 0.,
            "invalid effect placement"
        );
        // A projectile can finish during the same update that requested its effect.
        if spawn
            .follow
            .is_some_and(|follow| follow.position(frame).is_none())
        {
            return Ok(());
        }
        let bank = self
            .banks
            .get(&spawn.appearance.resource)
            .context("unprepared battle effect bank")?;
        let definition = bank
            .member(spawn.appearance.member)
            .context("unprepared battle effect")?;
        ensure!(
            spawn.follow.is_some() || definition.particles.values().all(|p| !p.data.follow_origin),
            "particle needs an origin attachment"
        );
        self.emitters.push(Active::new(definition.clone(), spawn));
        Ok(())
    }

    /// Consume one completed world update. Drawing only reads the resulting particles.
    /// Sounds and impacts join the scene's other presentation events.
    pub fn advance(&mut self, frame: &BattleFrame, paused: bool) -> Result<Vec<Cue>> {
        for cue in &frame.cues {
            match cue {
                Cue::CombatRetired => {
                    self.emitters.clear();
                    self.particles.clear();
                }
                Cue::Effect(spawn) => {
                    let result = self.start(spawn, frame);
                    self.diagnostics.attempt("battle effect request", result)?;
                }
                Cue::PoisonPulse { actor } => {
                    let result = self.poison(*actor, frame);
                    self.diagnostics.attempt("battle poison feedback", result)?;
                }
                _ => {}
            }
        }
        self.emitters
            .retain(|effect| effect.follow.is_none_or(|f| f.position(frame).is_some()));
        self.particles
            .retain(|_, particle| particle.follow.is_none_or(|f| f.position(frame).is_some()));
        let mut cues = Vec::new();
        if !paused {
            for mut active in std::mem::take(&mut self.emitters) {
                if let Some(origin) = active.follow.and_then(|follow| follow.position(frame)) {
                    active.origin = origin;
                }
                let result = active.step(self, &mut cues);
                if self.diagnostics.attempt("battle effect", result)? == Some(true) {
                    self.emitters.push(active);
                }
            }
            self.advance_particles(frame)?;
        }
        Ok(cues)
    }
}

#[cfg(test)]
mod tests;
