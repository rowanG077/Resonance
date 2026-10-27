//! Independent cosmetic timelines; emitted particles may outlive their emitter.
use crate::{ActorId, BattleFrame, Cue, EffectAppearance, ParticleId, Random, ResourceId};
use anyhow::{Context as _, Result, ensure};
pub use resonance_content::battle_effect::EffectTint;
use resonance_content::diagnostics::Diagnostics;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
const HOLY_SONG_INTERVAL: u16 = 180;
const HOLY_SONG_PHASE: u16 = 120;
const ACTOR_EFFECT_CYCLE: u16 = 360;
const OVERLIMIT_AURA_INTERVAL: u16 = 4;

#[cfg(feature = "test-support")]
mod recorded;
mod scheduler;
#[cfg(feature = "test-support")]
pub use recorded::FixtureEffectBirth;
mod stun;
pub use scheduler::EffectDefinition;

#[derive(Debug, Clone)]
pub struct EffectBank {
    pub(crate) resource: ResourceId,
    pub(crate) models: BTreeMap<u8, crate::PreparedEffectModel>,
    members: BTreeMap<u16, Arc<crate::EffectDefinition>>,
    weapon_model: Option<WeaponModel>,
}

#[derive(Debug, Clone)]
struct WeaponModel {
    owner: ActorId,
    weapon_slot: crate::WeaponSlot,
    model_slot: u8,
    variants: BTreeMap<ResourceId, crate::PreparedEffectModel>,
}

impl EffectBank {
    /// Admit selected effects once, after their model bindings are available.
    pub fn new(
        resource: ResourceId,
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

    pub fn resource(&self) -> ResourceId {
        self.resource
    }

    /// Select a thrown weapon from the same equipped models used by its owner.
    pub fn bind_weapon_models(
        &mut self,
        model_slot: u8,
        owner: ActorId,
        weapon_slot: crate::WeaponSlot,
        variants: BTreeMap<ResourceId, crate::PreparedEffectModel>,
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
    Bone {
        actor: ActorId,
        bone: u8,
    },
    Weapon {
        actor: ActorId,
        resource: ResourceId,
        bone: Option<u8>,
    },
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
#[derive(Clone)]
pub(crate) struct Active {
    appearance: EffectAppearance,
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
    passive: bool,
    during_pause: bool,
    paused_by_spell: bool,
    after_particles: bool,
    samples: Vec<f32>,
    spawned: Vec<Option<ParticleId>>,
}
impl Active {
    fn new(
        definition: Arc<EffectDefinition>,
        spawn: &EffectRequest,
        action: Option<crate::ActionId>,
    ) -> Self {
        let (spawns, samples) =
            definition
                .events
                .iter()
                .fold((0, 0), |(spawns, samples), event| match &event.operation {
                    resonance_content::battle_effect::EffectOperation::Spawn { birth, .. } => {
                        (spawns + 1, samples + birth.random_draws as usize)
                    }
                    _ => (spawns, samples),
                });
        Self {
            appearance: spawn.appearance,
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
            passive: false,
            during_pause: false,
            paused_by_spell: false,
            after_particles: false,
            samples: Vec::with_capacity(samples),
            spawned: Vec::with_capacity(spawns),
        }
    }
}

/// Cosmetic animation owned by the scene, independent of mutable gameplay state.
#[derive(Clone)]
pub struct Effects {
    pub(crate) banks: BTreeMap<ResourceId, EffectBank>,
    poison: Option<EffectAppearance>,
    holy_song: Option<EffectAppearance>,
    overlimit_aura: Option<OverLimitAura>,
    actor_effect_ticks: Vec<u16>,
    pub(crate) emitters: Vec<Active>,
    spare_emitters: Vec<Active>,
    hold_remaining: u16,
    pub(crate) particles: BTreeMap<ParticleId, crate::particle::Particle>,
    pub(crate) particle_visits: VecDeque<ParticleId>,
    pub(crate) random: Random,
    pub(crate) diagnostics: Diagnostics,
    pub(crate) next_particle: i32,
    ambient: Option<Ambient>,
    passive: Vec<Passive>,
    stun_particle: Option<Arc<crate::ParticleDefinition>>,
    head_anchors: Vec<resonance_content::battle_profile::HeadAnchor>,
    stunned: BTreeMap<ActorId, ParticleId>,
    #[cfg(feature = "test-support")]
    started_programs: Vec<EffectRequest>,
    #[cfg(feature = "test-support")]
    recorded_births: recorded::Births,
}

#[derive(Clone)]
pub struct PassiveEffect {
    pub actor: ActorId,
    pub appearance: EffectAppearance,
    pub interval: u16,
    /// Suspend the recurrence outside an unblended idle pose in combat.
    pub idle_only: bool,
    /// Primary carried-model resource; absent for an actor aura.
    pub weapon: Option<ResourceId>,
}

#[derive(Clone)]
struct Passive {
    effect: PassiveEffect,
    remaining: u16,
    visible: bool,
    was_idle: bool,
}

#[derive(Clone)]
struct OverLimitAura {
    appearance: EffectAppearance,
    anchors: Vec<Option<(u8, f32)>>,
}

#[derive(Clone)]
struct Ambient {
    appearance: EffectAppearance,
    interval: u16,
    remaining: u16,
}

impl EffectFollow {
    fn direction(self, frame: &BattleFrame) -> Option<[f32; 3]> {
        match self {
            Self::Projectile(id) => frame
                .cues
                .iter()
                .find_map(|cue| match cue {
                    Cue::ProjectileStarted {
                        projectile,
                        velocity,
                        ..
                    } if *projectile == id => Some(*velocity),
                    _ => None,
                })
                .or_else(|| {
                    frame
                        .projectiles
                        .iter()
                        .find(|p| p.id == id)
                        .map(|p| p.velocity)
                }),
            _ => None,
        }
    }
    pub(crate) fn particle_position(self, frame: &BattleFrame) -> Option<[f32; 3]> {
        let Self::Bone { actor, bone } = self else {
            return self.position(frame);
        };
        use resonance_content::animation::transform_point;
        let model = frame.models.iter().find(|model| model.actor == actor)?;
        let bone = model.bones.get(usize::from(bone))?;
        Some(transform_point(
            model.particle_world,
            transform_point(*bone, [0.; 3]),
        ))
    }

    pub(crate) fn position(self, frame: &BattleFrame) -> Option<[f32; 3]> {
        match self {
            Self::Actor(id) => frame.actors.get(id.index()).map(|a| a.position),
            Self::Center(id) => frame.actors.get(id.index()).map(|a| a.effect_center),
            Self::Bone { actor, bone } => {
                use resonance_content::animation::transform_point;
                let model = frame.models.iter().find(|model| model.actor == actor)?;
                let bone = model.bones.get(usize::from(bone))?;
                Some(transform_point(
                    model.world,
                    transform_point(*bone, [0.; 3]),
                ))
            }
            Self::Weapon {
                actor,
                resource,
                bone,
            } => {
                use resonance_content::animation::transform_point;
                let weapon = frame.weapons.iter().find(|weapon| {
                    weapon.owner == actor && weapon.resource == resource && weapon.visible
                })?;
                let point = if let Some(bone) = bone {
                    transform_point(*weapon.bones.get(usize::from(bone))?, [0.; 3])
                } else {
                    [0.; 3]
                };
                Some(transform_point(weapon.world, point))
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

    #[cfg(feature = "test-support")]
    pub fn particle_age(&self, id: ParticleId) -> Option<u32> {
        self.particles.get(&id).map(|particle| particle.age)
    }

    #[cfg(feature = "test-support")]
    pub fn particle_model_slot(&self, id: ParticleId) -> Option<u8> {
        let particle = self.particles.get(&id)?;
        let resource = particle.model.as_ref()?.resource();
        let mut slots = self
            .banks
            .get(&particle.frame.resource)?
            .models
            .iter()
            .filter_map(|(&slot, model)| (model.resource() == resource).then_some(slot));
        let slot = slots.next()?;
        slots.next().is_none().then_some(slot)
    }

    /// Actual accepted starts in the last update, including passive and child effects.
    #[cfg(feature = "test-support")]
    pub fn started_programs(&self) -> &[EffectRequest] {
        &self.started_programs
    }

    /// Live emitters, including waits before a later scheduled birth.
    #[cfg(feature = "test-support")]
    pub fn active_programs(&self) -> impl Iterator<Item = (ActorId, ResourceId, u16)> + '_ {
        self.emitters.iter().map(|active| {
            (
                active.actor,
                active.appearance.resource,
                active.appearance.member,
            )
        })
    }

    /// Allocated particles, including those awaiting their first visible update.
    #[cfg(feature = "test-support")]
    pub fn live_particles(&self) -> impl Iterator<Item = (ActorId, ResourceId, u16)> + '_ {
        self.particles.values().map(|particle| {
            (
                particle.frame.owner,
                particle.frame.resource,
                particle.frame.member,
            )
        })
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
            holy_song: None,
            overlimit_aura: None,
            actor_effect_ticks: Vec::new(),
            emitters: Vec::new(),
            spare_emitters: Vec::new(),
            hold_remaining: 0,
            particles: BTreeMap::new(),
            particle_visits: VecDeque::new(),
            random: Random::new(seed),
            diagnostics,
            next_particle: 1,
            ambient: None,
            passive: Vec::new(),
            stun_particle: None,
            head_anchors: Vec::new(),
            stunned: BTreeMap::new(),
            #[cfg(feature = "test-support")]
            started_programs: Vec::new(),
            #[cfg(feature = "test-support")]
            recorded_births: Default::default(),
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

    /// Restore the observed scenery phase without advancing actor effects.
    #[cfg(feature = "test-support")]
    pub fn restore_fixture_ambient_phase(
        &mut self,
        appearance: EffectAppearance,
        interval: u16,
        elapsed_updates: u16,
        frame: &BattleFrame,
    ) -> Result<()> {
        ensure!(
            self.ambient.is_none()
                && interval > 0
                && frame.clock == crate::BattleClock::Running
                && frame.unison.is_none()
                && frame.cues.is_empty(),
            "ambient fixture is not an isolated prelude"
        );
        let bank = self
            .banks
            .get(&appearance.resource)
            .context("observed ambient bank is not prepared")?
            .clone();
        let mut restored = Self::new(
            vec![bank],
            None,
            self.random.state(),
            self.diagnostics.clone(),
        )?
        .with_ambient(Some((appearance, interval)))?;
        restored.next_particle = self.next_particle;
        for _ in 0..elapsed_updates {
            ensure!(
                restored.advance(frame, false)?.is_empty(),
                "ambient prelude contains a non-particle outcome"
            );
        }
        self.emitters.append(&mut restored.emitters);
        self.particles.append(&mut restored.particles);
        self.ambient = restored.ambient;
        self.next_particle = restored.next_particle;
        self.random = restored.random;
        Ok(())
    }

    pub fn with_holy_song(mut self, appearance: EffectAppearance) -> Result<Self> {
        ensure!(
            self.banks
                .get(&appearance.resource)
                .and_then(|bank| bank.member(appearance.member))
                .is_some(),
            "Holy Song artwork is not prepared"
        );
        self.holy_song = Some(appearance);
        Ok(self)
    }

    pub fn with_overlimit_aura(
        mut self,
        appearance: EffectAppearance,
        anchors: Vec<Option<(u8, f32)>>,
    ) -> Result<Self> {
        ensure!(
            self.banks
                .get(&appearance.resource)
                .and_then(|bank| bank.member(appearance.member))
                .is_some(),
            "Over Limit aura is not prepared"
        );
        self.overlimit_aura = Some(OverLimitAura {
            appearance,
            anchors,
        });
        Ok(self)
    }

    /// Next cosmetic update for each actor; held actors keep their previous clock.
    pub fn actor_effect_ticks(&self) -> &[u16] {
        &self.actor_effect_ticks
    }

    #[cfg(feature = "test-support")]
    pub fn restore_fixture_actor_clock(&mut self, actor: ActorId, tick: u16) -> Result<()> {
        ensure!(
            tick < ACTOR_EFFECT_CYCLE,
            "observed actor effect clock exceeds its cycle"
        );
        let current = self
            .actor_effect_ticks
            .get_mut(actor.index())
            .context("observed actor has no initialized effect clock")?;
        *current = tick;
        Ok(())
    }

    fn advance_conditions(&mut self, frame: &BattleFrame) -> Result<()> {
        self.actor_effect_ticks.resize(frame.actors.len(), 0);
        let opening = frame.cues.iter().find_map(|cue| match cue {
            Cue::Casting {
                actor,
                phase: crate::CastPhase::SceneOpening,
                ..
            } => Some(actor.index()),
            _ => None,
        });
        for (index, actor) in frame.actors.iter().enumerate() {
            let id = ActorId::from_index(index)?;
            if !actor.available()
                || actor.hp <= 0
                || frame.paused_actors.contains(&id)
                || opening.is_some_and(|last| index > last)
            {
                continue;
            }
            let tick = self.actor_effect_ticks[index];
            self.actor_effect_ticks[index] = (tick + 1) % ACTOR_EFFECT_CYCLE;
            if opening == Some(index) || frame.recognized_result.is_some() {
                continue;
            }
            if (actor.overlimit.is_active()
                || frame.cues.contains(&Cue::OverLimitExpired { actor: id }))
                && tick.is_multiple_of(OVERLIMIT_AURA_INTERVAL)
                && let Some(aura) = &self.overlimit_aura
                && let Some(Some((bone, scale))) = aura.anchors.get(index)
            {
                let origin = (EffectFollow::Bone {
                    actor: id,
                    bone: *bone,
                })
                .position(frame)
                .context("Over Limit aura attachment is unavailable")?;
                self.start(
                    &EffectRequest {
                        owner: id,
                        target: id,
                        appearance: aura.appearance,
                        origin,
                        heading: actor.heading,
                        follow: None,
                        scale: *scale,
                        tint: Default::default(),
                    },
                    frame,
                    None,
                )?;
            }
            if tick % HOLY_SONG_INTERVAL != HOLY_SONG_PHASE
                || !actor
                    .conditions
                    .effective()
                    .contains(crate::conditions::Condition::HolySong)
            {
                continue;
            }
            if let Some(appearance) = self.holy_song {
                self.start(
                    &EffectRequest {
                        owner: id,
                        target: id,
                        appearance,
                        origin: actor.effect_origin(),
                        heading: actor.heading,
                        follow: Some(EffectFollow::Center(id)),
                        scale: actor.effect_scale,
                        tint: Default::default(),
                    },
                    frame,
                    None,
                )?;
            }
        }
        Ok(())
    }

    pub fn with_passive(mut self, requests: Vec<PassiveEffect>) -> Result<Self> {
        for effect in requests {
            let PassiveEffect {
                appearance,
                interval,
                weapon,
                ..
            } = effect;
            ensure!(
                interval > 0
                    && self
                        .banks
                        .get(&appearance.resource)
                        .and_then(|bank| bank.member(appearance.member))
                        .is_some(),
                "unprepared passive effect"
            );
            if weapon.is_some() {
                ensure!(
                    self.banks[&appearance.resource].members[&appearance.member]
                        .particles
                        .values()
                        .all(|particle| particle.data.owner_bone.is_none()),
                    "carried effect requires detached or position-following particles"
                );
            }
            self.passive.push(Passive {
                remaining: if weapon.is_some() { interval } else { 0 },
                effect,
                visible: true,
                was_idle: true,
            });
        }
        Ok(self)
    }

    /// Restore the ready clock; unobserved particles must expire before comparison.
    #[cfg(feature = "test-support")]
    pub fn restore_fixture_passive_phase(
        &mut self,
        actor: ActorId,
        weapon: Option<ResourceId>,
        recurrence: resonance_content::battle_profile::RecurringEffect,
        counter: u16,
        updates_before_comparison: u32,
        initial_idle: bool,
    ) -> Result<()> {
        let resonance_content::battle_profile::RecurringEffect { program, interval } = recurrence;
        let passive = self
            .passive
            .iter_mut()
            .find(|passive| {
                passive.effect.actor == actor
                    && passive.effect.weapon == weapon
                    && passive.effect.appearance.member == program
            })
            .context("observed passive emitter is not prepared")?;
        ensure!(
            interval == passive.effect.interval
                && counter <= interval
                && (!initial_idle || passive.effect.idle_only && counter == 0),
            "observed passive effect clock differs from its declaration"
        );
        let appearance = passive.effect.appearance;
        let definition = &self.banks[&appearance.resource].members[&appearance.member];
        let final_birth = definition.events.last().map_or(0, |event| event.at);
        // Body emitters count elapsed updates since their previous emission.
        let elapsed_at_ready = if weapon.is_none() {
            u32::from(counter)
        } else {
            0
        };
        let retirement_updates = updates_before_comparison
            .checked_add(elapsed_at_ready)
            .context("passive retirement interval overflow")?;
        // Before combat, idle emitters have no earlier particles to retire.
        for particle in definition.particles.values() {
            let lifetime = particle
                .data
                .lifetime
                .context("passive prelude has an unbounded particle")?;
            ensure!(
                (initial_idle || final_birth + lifetime <= retirement_updates)
                    && particle.data.bounce_effect.is_none()
                    && particle.data.floor_effect.is_none()
                    && particle.data.periodic_effect.is_none(),
                "ready-to-comparison interval cannot retire all unobserved passive particles"
            );
        }
        passive.remaining = if weapon.is_some() {
            interval - counter
        } else {
            (interval - counter) % interval
        };
        Ok(())
    }

    fn advance_passive(&mut self, frame: &BattleFrame, cues: &mut Vec<Cue>) -> Result<()> {
        let opening = frame
            .cues
            .iter()
            .filter_map(|cue| match cue {
                Cue::Casting {
                    actor,
                    phase: crate::CastPhase::SceneOpening,
                    ..
                } => Some(actor.index()),
                _ => None,
            })
            .min();
        let mut requests = Vec::new();
        for passive in &mut self.passive {
            let effect = &passive.effect;
            if opening.is_some_and(|owner| effect.actor.index() > owner) {
                continue;
            }
            let actor = frame
                .actors
                .get(effect.actor.index())
                .context("missing passive effect actor")?;
            // Actor emissions precede the model update that completes its fade.
            let was_visible = passive.visible;
            passive.visible = frame
                .models
                .iter()
                .find(|model| model.actor == effect.actor)
                .is_none_or(|model| model.visible);
            if actor.availability == crate::ActorAvailability::Absent
                || (actor.availability == crate::ActorAvailability::Dead && !was_visible)
            {
                continue;
            }
            if effect.idle_only {
                if !frame.combat_active {
                    continue;
                }
                let entering_idle = actor.activity == crate::Activity::Idle && !passive.was_idle;
                passive.was_idle = actor.activity == crate::Activity::Idle;
                if entering_idle {
                    passive.remaining = 0;
                }
                if entering_idle
                    || !passive.was_idle
                    || frame
                        .models
                        .iter()
                        .any(|model| model.actor == effect.actor && model.blend_weight < 1.)
                {
                    continue;
                }
            }
            let follow = effect
                .weapon
                .map_or(EffectFollow::Actor(effect.actor), |resource| {
                    EffectFollow::Weapon {
                        actor: effect.actor,
                        resource,
                        bone: None,
                    }
                });
            let Some(origin) = follow.position(frame) else {
                continue;
            };
            if effect.weapon.is_some() && frame.paused_actors.contains(&effect.actor) {
                continue;
            }
            if passive.remaining == 0 {
                requests.push(EffectRequest {
                    owner: effect.actor,
                    target: effect.actor,
                    appearance: effect.appearance,
                    origin,
                    heading: actor.heading,
                    follow: Some(follow),
                    scale: actor.body.scale,
                    tint: Default::default(),
                });
                passive.remaining = effect.interval;
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
            active.passive = true;
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

    /// Sample the prepared head attachment from the currently posed model.
    pub fn head_position(&self, id: ActorId, frame: &BattleFrame) -> Result<Option<[f32; 3]>> {
        let actor = frame.actors.get(id.index()).context("unknown head actor")?;
        let anchor = self
            .head_anchors
            .get(id.index())
            .context("head attachment is not prepared")?;
        let Some(mut origin) = (EffectFollow::Bone {
            actor: id,
            bone: anchor.bone,
        })
        .position(frame) else {
            return Ok(None);
        };
        let [x, y, z] = anchor.offset.map(|value| value * actor.body.scale);
        let (sin, cos) = actor.heading.to_radians().sin_cos();
        origin[0] += x * cos + z * sin;
        origin[1] += y;
        origin[2] += z * cos - x * sin;
        Ok(Some(origin))
    }

    fn poison(&mut self, id: ActorId, frame: &BattleFrame) -> Result<()> {
        let Some(appearance) = self.poison else {
            return Ok(());
        };
        let actor = frame
            .actors
            .get(id.index())
            .context("unknown poison actor")?;
        let Some(origin) = self.head_position(id, frame)? else {
            return Ok(());
        };
        self.start(
            &EffectRequest {
                owner: id,
                target: id,
                appearance,
                origin,
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
        after_particles: bool,
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
        active.after_particles = after_particles;
        if active.step(self, frame, cues)? {
            self.emitters.push(active);
        }
        Ok(())
    }

    /// Retire one actor's scene artwork without touching common or unrelated effects.
    pub fn clear_bank(&mut self, actor: ActorId, resource: ResourceId) {
        self.emitters
            .retain(|effect| effect.actor != actor || effect.appearance.resource != resource);
        self.particles.retain(|_, particle| {
            particle.frame.owner != actor || particle.frame.resource != resource
        });
    }

    /// Consume one completed world update. Drawing only reads the resulting particles.
    /// Sounds and impacts join the scene's other presentation events.
    pub fn advance(&mut self, frame: &BattleFrame, paused: bool) -> Result<Vec<Cue>> {
        // A held particle keeps its world origin even if its owner's pose changes.
        // Advancing particles publish their active binding again in Particle::step.
        for particle in self.particles.values_mut() {
            particle.frame.origin_attachment = None;
        }
        #[cfg(feature = "test-support")]
        self.started_programs.clear();
        let opening = frame.cues.iter().any(|cue| {
            matches!(
                cue,
                Cue::Casting {
                    phase: crate::CastPhase::SceneOpening,
                    ..
                }
            )
        });
        if opening {
            // Existing effects wait with combat. The opening's new emitters
            // still run, and join combat when the transition finishes.
            for active in &mut self.emitters {
                active.paused_by_spell = true;
            }
        }
        for bank in self.banks.values_mut() {
            let Some(binding) = &bank.weapon_model else {
                continue;
            };
            let weapon = frame
                .weapons
                .iter()
                .find(|weapon| {
                    weapon.owner == binding.owner && weapon.slot == Some(binding.weapon_slot)
                })
                .context("effect weapon owner has no equipped model")?;
            let model = binding
                .variants
                .get(&weapon.resource)
                .context("equipped effect weapon was not prepared")?;
            bank.models.insert(binding.model_slot, model.clone());
        }
        let mut cues = Vec::new();
        for cue in &frame.cues {
            match cue {
                Cue::ClearEffects => {
                    self.hold_remaining = 0;
                    self.emitters.clear();
                    self.particles.clear();
                    self.stunned.clear();
                }
                Cue::ClearEffectBank { actor, resource } => self.clear_bank(*actor, *resource),
                Cue::CombatRetired => {
                    self.hold_remaining = 0;
                    self.passive.clear();
                    self.stunned.clear();
                    self.emitters.retain(|effect| effect.ambient);
                    let ambient = self
                        .ambient
                        .as_ref()
                        .map(|effect| effect.appearance.resource);
                    self.particles
                        .retain(|_, particle| ambient == Some(particle.frame.resource));
                }
                Cue::HoldEffects(duration) => {
                    self.hold_remaining = self.hold_remaining.max(*duration);
                }
                Cue::Effect(spawn)
                | Cue::InitializedEffect(spawn)
                | Cue::InitializedActionEffect { request: spawn, .. }
                | Cue::EffectDuringPause(spawn) => {
                    let action = if let Cue::InitializedActionEffect { action, .. } = cue {
                        if !frame.actions.iter().any(|(id, _, _)| id == action)
                            || frame.cues.contains(&Cue::RetireActionEffects(*action))
                        {
                            continue;
                        }
                        Some(*action)
                    } else {
                        None
                    };
                    let first = self.emitters.len();
                    let result = self.start(spawn, frame, action).with_context(|| {
                        format!(
                            "effect {}:{} owned by actor {}",
                            spawn.appearance.resource,
                            spawn.appearance.member,
                            spawn.owner.index()
                        )
                    });
                    self.diagnostics.attempt("battle effect request", result)?;
                    if matches!(cue, Cue::EffectDuringPause(_))
                        && let Some(active) = self.emitters.get_mut(first)
                    {
                        active.during_pause = true;
                    }
                    // These emissions initialize at creation before the ordinary emitter
                    // visit. Particle integration still runs once.
                    if self.emitters.len() > first
                        && matches!(
                            cue,
                            Cue::InitializedEffect(_) | Cue::InitializedActionEffect { .. }
                        )
                    {
                        let mut active = self.emitters.pop().unwrap();
                        // Detached particles keep the requested creation origin.
                        let result = active.step(self, frame, &mut cues);
                        for handle in active.spawned.iter().flatten() {
                            if let Some(particle) = self.particles.get_mut(handle)
                                && particle.follow == spawn.follow
                                && let Some(origin) = particle
                                    .follow
                                    .and_then(|follow| follow.particle_position(frame))
                            {
                                particle.frame.origin = origin;
                                // Dormant model orientation keeps the emission point.
                                particle.orientation_origin = if particle.model.is_some()
                                    && !particle.definition.data.follow_orientation
                                {
                                    spawn.origin
                                } else {
                                    origin
                                };
                            }
                        }
                        if self.diagnostics.attempt("effect initialization", result)? == Some(true)
                        {
                            self.emitters.push(active);
                        }
                    }
                }
                Cue::ProjectileDeparture {
                    projectile,
                    animation,
                } => {
                    for particle in self.particles.values_mut().filter(|particle| {
                        particle.follow == Some(EffectFollow::Projectile(*projectile))
                    }) {
                        if let Some(model) = &mut particle.model {
                            let result = model.play(*animation);
                            self.diagnostics.attempt("projectile animation", result)?;
                        }
                    }
                }
                Cue::ProjectileStopped { projectile } => {
                    for particle in self.particles.values_mut().filter(|particle| {
                        particle.follow == Some(EffectFollow::Projectile(*projectile))
                            && particle.definition.data.follow_orientation
                    }) {
                        Arc::make_mut(&mut particle.definition)
                            .data
                            .follow_orientation = false;
                    }
                }
                Cue::ProjectileRebound {
                    projectile,
                    first,
                    pitch,
                } => {
                    for particle in self.particles.values_mut().filter(|particle| {
                        particle.follow == Some(EffectFollow::Projectile(*projectile))
                            && particle.model.is_some()
                    }) {
                        if *first {
                            Arc::make_mut(&mut particle.definition)
                                .data
                                .follow_orientation = false;
                        }
                        if let Some(pitch) = pitch {
                            particle.frame.state.angles[0] = *pitch;
                        }
                        let spin = &mut particle.frame.state.angular_velocity[0];
                        *spin = if *first { -6. } else { *spin * 0.5 };
                    }
                }
                Cue::PoisonPulse { actor } => {
                    let result = self.poison(*actor, frame);
                    self.diagnostics.attempt("battle poison feedback", result)?;
                }
                _ => {}
            }
        }
        self.emitters.retain_mut(|effect| {
            if matches!(effect.follow, Some(EffectFollow::Projectile(_)))
                && effect.follow.and_then(|f| f.position(frame)).is_none()
                && effect.definition.particles.values().all(|particle| {
                    !particle.data.follow_origin && particle.data.lifetime.is_some()
                })
            {
                // A finite burst owns its emission schedule. Keep its last sampled
                // anchor after the contact projectile ends.
                effect.follow = None;
            }
            effect.follow.is_none_or(|f| f.position(frame).is_some())
                && effect
                    .action
                    .is_none_or(|id| frame.actions.iter().any(|(action, _, _)| *action == id))
        });
        self.particles
            .retain(|_, particle| particle.follow.is_none_or(|f| f.position(frame).is_some()));
        let phase = frame.unison.as_ref().map(|unison| unison.phase);
        let cinematic = matches!(
            phase,
            Some(crate::UnisonPhase::Windup | crate::UnisonPhase::CombinationPose)
        );
        let timed_hold = matches!(
            frame.clock,
            crate::BattleClock::TimedHold | crate::BattleClock::Rescue(_)
        );
        let held = self.hold_remaining != 0;
        if !paused && phase != Some(crate::UnisonPhase::Selecting) {
            self.hold_remaining = self.hold_remaining.saturating_sub(1);
        }
        let only_pause_effects = paused || cinematic || held;
        // Opening emitters keep running while ordinary particles wait for the scene.
        let transition = frame.clock == crate::BattleClock::SpellTransition || opening;
        let only_pause_particles = only_pause_effects || transition;
        self.update_stun(frame, only_pause_particles, stun::StunPhase::Existing)?;
        if (!paused || timed_hold) && phase != Some(crate::UnisonPhase::Selecting) {
            if !only_pause_effects && frame.clock != crate::BattleClock::SpellTransition {
                self.advance_conditions(frame)?;
            }
            // Actor timers run before an action opens its scene; particles wait
            // from that opening update onward.
            if !only_pause_effects && frame.clock != crate::BattleClock::SpellTransition {
                self.advance_passive(frame, &mut cues)?;
            }
            for after_particles in [false, true] {
                if after_particles {
                    cues.extend(self.advance_particles_filtered(frame, only_pause_particles)?);
                }
                let mut emitters = std::mem::take(&mut self.spare_emitters);
                std::mem::swap(&mut emitters, &mut self.emitters);
                for mut active in emitters.drain(..) {
                    if active.after_particles != after_particles
                        || (only_pause_effects && !active.during_pause)
                        || (transition && (active.passive || active.ambient))
                        || (frame.clock == crate::BattleClock::SpellTransition
                            && active.paused_by_spell)
                    {
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
                    let result = if active.action.is_some_and(|action| {
                        frame.clock == crate::BattleClock::SpellTransition
                            || frame.paused_actors.contains(&active.actor)
                            || frame.cues.contains(&Cue::PauseActionEffects(action))
                    }) {
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
                self.spare_emitters = emitters;
            }
            // Scenery births follow the particle visit, so their first frame
            // retains the authored offset, color and size at age zero.
            if !only_pause_particles {
                self.advance_ambient(frame, &mut cues)?;
            }
        }
        self.update_stun(frame, only_pause_particles, stun::StunPhase::Contacts)?;
        for cue in &frame.cues {
            match cue {
                Cue::RetireActionEffects(action) => {
                    self.emitters
                        .retain(|effect| effect.action != Some(*action));
                }
                Cue::ProjectileDeparture { projectile, .. } => {
                    for particle in self.particles.values_mut().filter(|particle| {
                        particle.follow == Some(EffectFollow::Projectile(*projectile))
                            && particle.model.is_some()
                    }) {
                        Arc::make_mut(&mut particle.definition)
                            .data
                            .follow_orientation = true;
                    }
                }
                Cue::ActionEffect { action, request }
                    if frame.actions.iter().any(|(id, _, _)| id == action)
                        && !frame.cues.contains(&Cue::RetireActionEffects(*action)) =>
                {
                    let result = self.start(request, frame, Some(*action));
                    self.diagnostics.attempt("battle action effect", result)?;
                }
                Cue::DeferredEffect(request) | Cue::AfterParticlesEffect(request) => {
                    // Start after this update's particle visit. Some emitters
                    // retain that phase for every subsequent birth.
                    let result = self.emit_child(
                        request,
                        frame,
                        &mut cues,
                        matches!(cue, Cue::AfterParticlesEffect(_)),
                    );
                    self.diagnostics.attempt("battle deferred effect", result)?;
                }
                _ => {}
            }
        }
        Ok(cues)
    }
}

#[cfg(test)]
mod tests;
