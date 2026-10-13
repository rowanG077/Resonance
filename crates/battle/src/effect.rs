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
    weapon_model: Option<WeaponModel>,
}

#[derive(Debug, Clone)]
struct WeaponModel {
    owner: ActorId,
    weapon_slot: u8,
    model_slot: u8,
    variants: BTreeMap<u32, crate::PreparedEffectModel>,
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
        loop {
            let missing: Vec<_> = admitted
                .iter()
                .filter_map(|(&member, definition)| {
                    definition
                        .particles
                        .values()
                        .flat_map(|particle| {
                            [
                                particle.data.bounce_effect,
                                particle.data.floor_effect,
                                particle.data.periodic_effect.map(|effect| effect.member),
                            ]
                            .into_iter()
                            .flatten()
                        })
                        .find(|child| !admitted.contains_key(child))
                        .map(|child| (member, child))
                })
                .collect();
            if missing.is_empty() {
                break;
            }
            for (member, child) in missing {
                diagnostics.report(
                    "battle child effect",
                    anyhow::anyhow!(
                        "effect {resource}:{member} requires unprepared child effect {child}"
                    ),
                )?;
                admitted.remove(&member);
            }
        }
        Ok(Self {
            resource,
            models,
            members: admitted,
            weapon_model: None,
        })
    }

    pub fn resource(&self) -> u32 {
        self.resource
    }

    /// Select a thrown weapon from the same equipped models used by its owner.
    pub fn bind_weapon_models(
        &mut self,
        model_slot: u8,
        owner: ActorId,
        weapon_slot: u8,
        variants: BTreeMap<u32, crate::PreparedEffectModel>,
    ) -> Result<()> {
        ensure!(
            !variants.is_empty(),
            "effect weapon has no prepared variants"
        );
        let mut models = self.models.clone();
        for model in variants.values() {
            models.insert(model_slot, model.clone());
            for definition in self.members.values() {
                definition.validate(&models)?;
            }
        }
        self.weapon_model = Some(WeaponModel {
            owner,
            weapon_slot,
            model_slot,
            variants,
        });
        Ok(())
    }

    pub fn member(&self, member: u16) -> Option<&Arc<crate::EffectDefinition>> {
        self.members.get(&member)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectFollow {
    Actor(ActorId),
    Center(ActorId),
    Bone { actor: ActorId, bone: u8 },
    Projectile(crate::ProjectileId),
    Spell(crate::ActionId),
}
#[derive(Debug, Clone, PartialEq)]
pub struct EffectRequest {
    pub owner: ActorId,
    pub target: ActorId,
    pub appearance: EffectAppearance,
    pub origin: [f32; 3],
    pub heading: f32,
    /// Omit for an emitter anchored at the supplied world position.
    pub follow: Option<EffectFollow>,
    pub scale: f32,
    pub tint: EffectTint,
}
pub(crate) struct Active {
    resource: u32,
    definition: Arc<EffectDefinition>,
    action: Option<crate::ActionId>,
    actor: ActorId,
    target: ActorId,
    age: u32,
    next: usize,
    origin: [f32; 3],
    heading: f32,
    direction: Option<[f32; 3]>,
    pub(crate) follow: Option<EffectFollow>,
    scale: f32,
    tint: EffectTint,
    ambient: bool,
    during_pause: bool,
    samples: Vec<f32>,
    spawned: Vec<Option<ParticleId>>,
}
impl Active {
    fn new(
        definition: Arc<EffectDefinition>,
        spawn: &EffectRequest,
        action: Option<crate::ActionId>,
    ) -> Self {
        Self {
            resource: spawn.appearance.resource,
            definition,
            action,
            actor: spawn.owner,
            target: spawn.target,
            age: 0,
            next: 0,
            origin: spawn.origin,
            heading: spawn.heading,
            direction: None,
            follow: spawn.follow,
            scale: spawn.scale,
            tint: spawn.tint,
            ambient: false,
            during_pause: false,
            samples: Vec::new(),
            spawned: Vec::new(),
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
    ambient: Option<Ambient>,
    passive: Vec<Passive>,
    #[cfg(feature = "test-support")]
    started_programs: Vec<EffectRequest>,
}

struct Passive {
    actor: ActorId,
    appearance: EffectAppearance,
    interval: u16,
    remaining: u16,
}

struct Ambient {
    appearance: EffectAppearance,
    interval: u16,
    remaining: u16,
}

impl EffectFollow {
    fn direction(self, frame: &BattleFrame) -> Option<[f32; 3]> {
        match self {
            Self::Projectile(id) => frame
                .projectiles
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.velocity),
            _ => None,
        }
    }
    pub(crate) fn position(self, frame: &BattleFrame) -> Option<[f32; 3]> {
        match self {
            Self::Actor(id) => frame.actors.get(id.index()).map(|a| a.position),
            Self::Center(id) => frame.actors.get(id.index()).map(|a| a.effect_origin()),
            Self::Bone { actor, bone } => {
                use resonance_content::animation::transform_point;
                let model = frame.models.iter().find(|model| model.actor == actor)?;
                let bone = model.bones.get(usize::from(bone))?;
                Some(transform_point(
                    model.world,
                    transform_point(*bone, [0.; 3]),
                ))
            }
            Self::Projectile(id) => frame
                .projectiles
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.position),
            Self::Spell(id) => frame
                .spell_origins
                .iter()
                .find_map(|&(action, point)| (action == id).then_some(point)),
        }
    }
}

impl Effects {
    /// Inspect the live particle attachment without exposing mutable particles.
    #[cfg(feature = "test-support")]
    pub fn particle_attachment(&self, id: ParticleId) -> Option<EffectFollow> {
        self.particles.get(&id).and_then(|particle| particle.follow)
    }

    /// Actual accepted starts in the last update, including passive and child effects.
    #[cfg(feature = "test-support")]
    pub fn started_programs(&self) -> &[EffectRequest] {
        &self.started_programs
    }

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
            ambient: None,
            passive: Vec::new(),
            #[cfg(feature = "test-support")]
            started_programs: Vec::new(),
        })
    }

    pub fn with_ambient(mut self, effect: Option<(EffectAppearance, u16)>) -> Result<Self> {
        if let Some((appearance, interval)) = effect {
            ensure!(interval > 0, "zero ambient effect interval");
            ensure!(
                self.banks
                    .get(&appearance.resource)
                    .and_then(|bank| bank.member(appearance.member))
                    .is_some(),
                "unprepared ambient effect"
            );
            self.ambient = Some(Ambient {
                appearance,
                interval,
                remaining: 0,
            });
        }
        Ok(self)
    }

    pub fn with_passive(mut self, requests: Vec<(ActorId, EffectAppearance, u16)>) -> Result<Self> {
        for (actor, appearance, interval) in requests {
            ensure!(
                interval > 0
                    && self
                        .banks
                        .get(&appearance.resource)
                        .and_then(|bank| bank.member(appearance.member))
                        .is_some(),
                "unprepared passive effect"
            );
            self.passive.push(Passive {
                actor,
                appearance,
                interval,
                remaining: 0,
            });
        }
        Ok(self)
    }

    fn advance_passive(&mut self, frame: &BattleFrame, cues: &mut Vec<Cue>) -> Result<()> {
        let mut requests = Vec::new();
        for passive in &mut self.passive {
            let actor = frame
                .actors
                .get(passive.actor.index())
                .context("missing passive effect actor")?;
            if actor.hp <= 0 || actor.availability == crate::ActorAvailability::Absent {
                continue;
            }
            if passive.remaining == 0 {
                requests.push(EffectRequest {
                    owner: passive.actor,
                    target: passive.actor,
                    appearance: passive.appearance,
                    origin: actor.position,
                    heading: actor.heading,
                    follow: Some(EffectFollow::Actor(passive.actor)),
                    scale: actor.body.scale,
                    tint: Default::default(),
                });
                passive.remaining = passive.interval;
            }
            passive.remaining -= 1;
        }
        for request in requests {
            let definition = self.banks[&request.appearance.resource]
                .member(request.appearance.member)
                .context("unprepared passive effect")?
                .clone();
            #[cfg(feature = "test-support")]
            self.started_programs.push(request.clone());
            let mut active = Active::new(definition, &request, None);
            // Passive creation executes age zero before the ordinary emitter
            // visit. Particles still integrate only once below.
            let result = active.step(self, frame, cues);
            if self.diagnostics.attempt("battle effect", result)? == Some(true) {
                self.emitters.push(active);
            }
        }
        Ok(())
    }

    fn advance_ambient(&mut self, frame: &BattleFrame, cues: &mut Vec<Cue>) -> Result<()> {
        let Some(mut ambient) = self.ambient.take() else {
            return Ok(());
        };
        if ambient.remaining == 0 {
            let definition = self.banks[&ambient.appearance.resource]
                .member(ambient.appearance.member)
                .context("unprepared ambient effect")?;
            let request = EffectRequest {
                owner: ActorId::from_index(0)?,
                target: ActorId::from_index(0)?,
                appearance: ambient.appearance,
                origin: [0.; 3],
                heading: 0.,
                follow: None,
                scale: 1.,
                tint: Default::default(),
            };
            let mut emitter = Active::new(definition.clone(), &request, None);
            #[cfg(feature = "test-support")]
            self.started_programs.push(request);
            emitter.ambient = true;
            let result = emitter.step(self, frame, cues);
            if self.diagnostics.attempt("ambient battle effect", result)? == Some(true) {
                self.emitters.push(emitter);
            }
            ambient.remaining = ambient.interval;
        }
        ambient.remaining -= 1;
        self.ambient = Some(ambient);
        Ok(())
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
            None,
        )
    }

    fn start(
        &mut self,
        spawn: &EffectRequest,
        frame: &BattleFrame,
        action: Option<crate::ActionId>,
    ) -> Result<()> {
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
        let mut active = Active::new(definition.clone(), spawn, action);
        active.direction = spawn.follow.and_then(|follow| follow.direction(frame));
        if action.is_some()
            && let Some(origin) = spawn.follow.and_then(|follow| follow.position(frame))
        {
            active.origin = origin;
            active.heading = frame.actors[spawn.owner.index()].heading;
        }
        #[cfg(feature = "test-support")]
        self.started_programs.push(spawn.clone());
        self.emitters.push(active);
        Ok(())
    }

    pub(crate) fn emit_child(
        &mut self,
        spawn: &EffectRequest,
        frame: &BattleFrame,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let definition = self
            .banks
            .get(&spawn.appearance.resource)
            .and_then(|bank| bank.member(spawn.appearance.member))
            .context("unprepared child effect")?
            .clone();
        #[cfg(feature = "test-support")]
        self.started_programs.push(spawn.clone());
        let mut active = Active::new(definition, spawn, None);
        if active.step(self, frame, cues)? {
            self.emitters.push(active);
        }
        Ok(())
    }

    /// Retire one actor's scene artwork without touching common or unrelated effects.
    pub fn clear_bank(&mut self, actor: ActorId, resource: u32) {
        self.emitters
            .retain(|effect| effect.actor != actor || effect.resource != resource);
        self.particles.retain(|_, particle| {
            particle.frame.owner != actor || particle.frame.resource != resource
        });
    }

    /// Consume one completed world update. Drawing only reads the resulting particles.
    /// Sounds and impacts join the scene's other presentation events.
    pub fn advance(&mut self, frame: &BattleFrame, paused: bool) -> Result<Vec<Cue>> {
        #[cfg(feature = "test-support")]
        self.started_programs.clear();
        for bank in self.banks.values_mut() {
            let Some(binding) = &bank.weapon_model else {
                continue;
            };
            let weapon = frame
                .weapons
                .iter()
                .find(|weapon| weapon.owner == binding.owner && weapon.slot == binding.weapon_slot)
                .context("effect weapon owner has no equipped model")?;
            let model = binding
                .variants
                .get(&weapon.resource)
                .context("equipped effect weapon was not prepared")?;
            bank.models.insert(binding.model_slot, model.clone());
        }
        for cue in &frame.cues {
            match cue {
                Cue::ClearEffects => {
                    self.emitters.clear();
                    self.particles.clear();
                }
                Cue::ClearEffectBank { actor, resource } => self.clear_bank(*actor, *resource),
                Cue::CombatRetired => {
                    self.passive.clear();
                    self.emitters.retain(|effect| effect.ambient);
                    let ambient = self
                        .ambient
                        .as_ref()
                        .map(|effect| effect.appearance.resource);
                    self.particles
                        .retain(|_, particle| ambient == Some(particle.frame.resource));
                }
                Cue::Effect(spawn) | Cue::CastingEffect(spawn) | Cue::EffectDuringPause(spawn) => {
                    let first = self.emitters.len();
                    let result = self.start(spawn, frame, None).with_context(|| {
                        format!(
                            "effect {}:{} owned by actor {}",
                            spawn.appearance.resource,
                            spawn.appearance.member,
                            spawn.owner.index()
                        )
                    });
                    self.diagnostics.attempt("battle effect request", result)?;
                    if matches!(cue, Cue::CastingEffect(_))
                        && let Some(active) = self.emitters.get_mut(first)
                    {
                        active.age = 1;
                    }
                    if matches!(cue, Cue::EffectDuringPause(_))
                        && let Some(active) = self.emitters.get_mut(first)
                    {
                        active.during_pause = true;
                    }
                }
                Cue::ProjectileRebound { projectile, first } => {
                    for particle in self.particles.values_mut().filter(|particle| {
                        particle.follow == Some(EffectFollow::Projectile(*projectile))
                            && particle.model.is_some()
                    }) {
                        if *first {
                            Arc::make_mut(&mut particle.definition)
                                .data
                                .follow_orientation = false;
                        }
                        let pitch = &mut particle.frame.state.angular_velocity[0];
                        *pitch = if *first { -6. } else { *pitch * 0.5 };
                    }
                }
                Cue::PoisonPulse { actor } => {
                    let result = self.poison(*actor, frame);
                    self.diagnostics.attempt("battle poison feedback", result)?;
                }
                _ => {}
            }
        }
        self.emitters.retain(|effect| {
            effect.follow.is_none_or(|f| f.position(frame).is_some())
                && effect
                    .action
                    .is_none_or(|id| frame.actions.iter().any(|(action, _, _)| *action == id))
        });
        self.particles
            .retain(|_, particle| particle.follow.is_none_or(|f| f.position(frame).is_some()));
        let mut cues = Vec::new();
        let phase = frame.unison.as_ref().map(|unison| unison.phase);
        let cinematic = matches!(
            phase,
            Some(crate::UnisonPhase::Windup | crate::UnisonPhase::CombinationPose)
        );
        let timed_hold = matches!(
            frame.clock,
            crate::BattleClock::TimedHold | crate::BattleClock::Rescue(_)
        );
        let only_pause_effects = paused || cinematic;
        if (!paused || timed_hold) && phase != Some(crate::UnisonPhase::Selecting) {
            if !only_pause_effects {
                self.advance_passive(frame, &mut cues)?;
            }
            for mut active in std::mem::take(&mut self.emitters) {
                if only_pause_effects && !active.during_pause {
                    self.emitters.push(active);
                    continue;
                }
                let origin = active.follow.and_then(|follow| follow.position(frame));
                active.direction = active.follow.and_then(|follow| follow.direction(frame));
                if active.action.is_none()
                    && let Some(origin) = origin
                {
                    active.origin = origin;
                }
                let result =
                    if active.action.is_some() && frame.paused_actors.contains(&active.actor) {
                        Ok(true)
                    } else {
                        active.step(self, frame, &mut cues)
                    };
                // Action emission precedes movement. Retain this update's root
                // for the next emission without moving particles already born.
                if active.action.is_some() {
                    if let Some(origin) = origin {
                        active.origin = origin;
                    }
                    active.heading = frame.actors[active.actor.index()].heading;
                }
                if self.diagnostics.attempt("battle effect", result)? == Some(true) {
                    self.emitters.push(active);
                }
            }
            cues.extend(self.advance_particles_filtered(frame, only_pause_effects)?);
            // Scenery births follow the particle visit, so their first frame
            // retains the authored offset, color and size at age zero.
            if !only_pause_effects {
                self.advance_ambient(frame, &mut cues)?;
            }
        }
        for cue in &frame.cues {
            if let Cue::ActionEffect { action, request } = cue
                && frame.actions.iter().any(|(id, _, _)| id == action)
            {
                let result = self.start(request, frame, Some(*action));
                self.diagnostics.attempt("battle action effect", result)?;
            }
        }
        Ok(cues)
    }
}

#[cfg(test)]
mod tests;
