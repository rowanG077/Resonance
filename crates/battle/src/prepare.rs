use crate::{Actor, ActorId};
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use symphonia_script::Program;
use symphonia_script_vm::Vm;

/// Runtime bindings into the presentation resources prepared by the game host.
/// Resource identifiers have no path or original-address semantics in combat.
#[derive(Debug, Clone)]
pub struct EffectBank {
    pub resource: u32,
    pub models: BTreeMap<u8, crate::PreparedEffectModel>,
    pub members: BTreeMap<u16, Arc<ActionDefinition>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundBinding {
    pub resource: u32,
    pub index: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceLine {
    pub sound: SoundBinding,
    /// Authored casting observes this source duration, not a guessed playback tail.
    pub duration: u16,
}

#[derive(Debug, Clone)]
pub enum ResourceBinding {
    ActorTints([[u8; 4]; 12]),
    /// One verified spoken line per actor; None means the original actor has no line.
    Voice(Vec<Option<VoiceLine>>),
    Sound(SoundBinding),
    Effect(u32),
    Particle(Arc<crate::ParticleDefinition>),
    Projectile(Arc<crate::ProjectileDefinition>),
    Melee(Arc<crate::MeleeDefinition>),
    WeaponFlight(Arc<crate::WeaponFlightDefinition>),
    Motion(crate::MotionBinding),
    /// A verified absent profile clip is distinct from a missing resource.
    OptionalMotion(Vec<Option<crate::MotionBinding>>),
    EffectMotion(crate::EffectMotionBinding),
    Casting(Arc<crate::CastingDefinition>),
    /// A released sequence prepared in this same battle generation.
    Spell(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionPhase {
    /// Actor commands and hit streams run before the other object groups/contacts.
    Actor,
    /// Casting callbacks run with actors, including during blends/local hit-stop.
    Casting,
    /// Long-lived actor callbacks use the same VM, including while incapacitated.
    Controller,
    /// Authored target/movement/action choices; does not occupy the actor action.
    Decision,
    /// Released spell/effect callbacks run after contact resolution.
    Resident,
    /// Independent effect programs run immediately, then before or after particles.
    Effect,
}

impl ActionPhase {
    pub(crate) fn is_actor(self) -> bool {
        matches!(self, Self::Actor | Self::Casting | Self::Controller)
    }
}

/// An in-memory executable and its verified, module-local asset bindings.
/// Original command records must also be decoded before constructing this value.
#[derive(Debug, Clone)]
pub struct ActionDefinition {
    pub id: u16,
    pub phase: ActionPhase,
    pub program: Arc<Program>,
    pub entry: u32,
    /// Inclusive lifetime for attack/resident sequences. Casting ends explicitly
    /// in source; an occupied spell slot can keep its countdown waiting.
    pub duration: u16,
    /// Admission cost. The authored action commits its actual debit with pay_tp.
    pub tp_cost: u16,
    pub resources: Vec<ResourceBinding>,
}

/// Validated input for one battle. Construction cannot modify the field session.
/// This type is deliberately not serializable: executables never enter cooked data.
#[derive(Debug)]
pub struct PreparedBattle {
    pub(crate) companions: Vec<Option<crate::CompanionDefinition>>,
    pub(crate) decisions: Vec<Option<crate::DecisionDefinition>>,
    pub(crate) enemy_decisions: Vec<Option<crate::EnemyDecisionDefinition>>,
    pub(crate) enemy_selected: Vec<Option<usize>>,
    pub(crate) entry_timers: Vec<crate::EntryTimers>,
    pub(crate) entry_voice: Option<crate::EntryVoiceDefinition>,
    pub(crate) voices: Vec<crate::voice::Voice>,
    pub(crate) blinking: Vec<bool>,
    pub(crate) idle_expressions: Vec<Option<[u8; 4]>>,
    pub(crate) targets: Vec<ActorId>,
    pub(crate) grade_rank: u8,
    pub(crate) trails: Vec<Vec<crate::trail::Trail>>,
    pub(crate) deaths: Vec<Option<crate::DeathBinding>>,
    pub(crate) death_feedback: Option<crate::DeathFeedback>,
    pub(crate) contact_feedback: Option<crate::ContactFeedback>,
    pub(crate) admission_flashes: BTreeMap<u16, [u8; 3]>,
    pub(crate) contact_audio: Option<crate::ContactAudio>,
    pub(crate) controls: Vec<Option<crate::ControlDefinition>>,
    pub(crate) ambient_color: [u8; 3],
    pub(crate) arena_boundary: bool,
    pub(crate) hover_sine: Option<Box<[f32; 360]>>,
    pub(crate) landing_effect: Option<crate::EffectAppearance>,
    pub(crate) recovery_returns: Vec<Option<crate::RecoveryReturnDefinition>>,
    pub(crate) voices_enabled: bool,
    pub(crate) stage_colors: crate::stage::StageColors,
    pub(crate) camera: Option<crate::camera::Camera>,
    pub(crate) actors: Vec<Actor>,
    pub(crate) actions: Vec<ActionDefinition>,
    pub(crate) random_seed: u32,
    pub(crate) effects: BTreeMap<u32, EffectBank>,
    pub(crate) models: Vec<Option<crate::model::Model>>,
}

impl PreparedBattle {
    /// Seed persistent blade histories from the same initial CPU poses as actors.
    pub fn with_trails(mut self, definitions: Vec<Vec<crate::TrailDefinition>>) -> Result<Self> {
        ensure!(
            definitions.len() == self.actors.len(),
            "trail/actor count differs"
        );
        let mut trails = Vec::with_capacity(definitions.len());
        for (index, definitions) in definitions.into_iter().enumerate() {
            let mut slots = BTreeSet::new();
            let mut actor_trails = Vec::new();
            if !definitions.is_empty() {
                let model = self.models[index]
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("weapon trail requires an actor model"))?;
                let weapons = model.weapon_frames();
                for definition in definitions {
                    ensure!(slots.insert(definition.slot), "duplicate weapon trail slot");
                    actor_trails.push(crate::trail::Trail::new(
                        definition,
                        &model.shown,
                        &weapons,
                    )?);
                }
            }
            trails.push(actor_trails);
        }
        self.trails = trails;
        Ok(self)
    }

    pub fn with_voices_enabled(mut self, enabled: bool) -> Self {
        self.voices_enabled = enabled;
        self
    }

    pub fn with_camera(mut self, definition: crate::CameraDefinition) -> Result<Self> {
        self.camera = Some(crate::camera::Camera::new(definition, &self.actors)?);
        Ok(self)
    }

    /// The ordinary entry camera owns the lead-in on the battle update clock.
    pub fn with_entry_camera(
        mut self,
        definition: crate::CameraDefinition,
        entry: crate::EntryCamera,
    ) -> Result<Self> {
        let mut camera = crate::camera::Camera::new(definition, &self.actors)?;
        camera.initialize_entry(&self.actors, entry)?;
        self.camera = Some(camera);
        Ok(self)
    }

    /// Supply the stage's default color and the initial colors of its present models.
    pub fn with_stage_colors(mut self, base: [u8; 4], models: [Option<[u8; 4]>; 4]) -> Self {
        self.stage_colors.base = base;
        self.stage_colors.models = models;
        self
    }

    /// Set the stage's ambient actor color before activation. The small kernel
    /// defaults to neutral lighting; stage preparation supplies other colors.
    pub fn with_ambient_color(mut self, color: [u8; 3]) -> Self {
        self.ambient_color = color;
        for (actor, model) in self.actors.iter_mut().zip(&mut self.models) {
            actor.body.tint[..3].copy_from_slice(&color);
            if let Some(model) = model {
                model.shown.tint = actor.body.tint;
            }
        }
        self
    }

    pub fn new(
        mut actors: Vec<Actor>,
        actions: Vec<ActionDefinition>,
        random_seed: u32,
        models: Vec<Option<Arc<crate::ModelDefinition>>>,
        effects: Vec<EffectBank>,
    ) -> Result<Self> {
        ensure!(
            !actors.is_empty()
                && actors
                    .iter()
                    .filter(|a| a.side == crate::Side::Party)
                    .count()
                    <= 4
                && actors
                    .iter()
                    .filter(|a| a.side == crate::Side::Enemy)
                    .count()
                    <= 8,
            "battle needs actors within the four-party/eight-enemy banks"
        );
        for actor in &mut actors {
            actor.validate()?;
            if actor.availability == crate::ActorAvailability::Active {
                if actor.hp == 0 {
                    actor.availability = crate::ActorAvailability::Dead;
                } else if actor.petrified {
                    actor.availability = crate::ActorAvailability::Petrified;
                }
            }
            actor.petrified = actor.availability == crate::ActorAvailability::Petrified;
            if actor.availability == crate::ActorAvailability::Dead {
                actor.activity = crate::Activity::Defeated;
            }
            actor.sample_center()?;
        }
        crate::steering::return_position::initialize(&mut actors);
        ensure!(
            models.is_empty() || models.len() == actors.len(),
            "battle model/actor count differs"
        );
        let models = if models.is_empty() {
            vec![None; actors.len()]
        } else {
            models
        };
        let models: Vec<_> = models
            .into_iter()
            .zip(&mut actors)
            .enumerate()
            .map(|(i, (model, actor))| {
                model
                    .map(|model| crate::model::Model::new(model, ActorId(i as u8), actor))
                    .transpose()
            })
            .collect::<Result<_>>()?;
        let mut banks = BTreeMap::new();
        for bank in effects {
            ensure!(
                banks.insert(bank.resource, bank).is_none(),
                "duplicate battle effect bank"
            );
        }
        let mut ids = BTreeSet::new();
        for action in &actions {
            ensure!(
                ids.insert(action.id),
                "duplicate battle action {}",
                action.id
            );
            ensure!(
                action.phase != ActionPhase::Effect,
                "effect must be bound to an effect bank"
            );
        }
        let mut pending: Vec<_> = actions.iter().collect();
        let mut needs_knockdown = actors.iter().any(|actor| {
            actor.reaction.stagger.received >= actor.reaction.stagger.threshold
                || matches!(
                    actor.activity,
                    crate::Activity::KnockedDown | crate::Activity::GettingUp
                )
        });
        let mut needs_stun = actors.iter().any(|actor| {
            actor.reaction.stun.chance_bonus != 0
                || actor.reaction.stun.ex_bonus
                || actor.activity == crate::Activity::Stunned
        });
        for bank in banks.values() {
            for effect in bank.members.values() {
                ensure!(
                    effect.phase == ActionPhase::Effect && effect.tp_cost == 0,
                    "invalid prepared effect sequence"
                );
                pending.push(effect);
            }
        }
        for action in pending {
            ensure!(
                action.duration <= i16::MAX as u16,
                "battle action duration exceeds signed clock"
            );
            let module = action
                .program
                .authored()
                .ok_or_else(|| anyhow::anyhow!("battle action requires an authored module"))?;
            let function = module
                .functions
                .iter()
                .find(|f| f.entry == action.entry)
                .ok_or_else(|| anyhow::anyhow!("battle action entry is missing"))?;
            ensure!(
                function.is_task && function.parameters == 0 && function.results == 0,
                "battle entry must be a parameterless task without a result"
            );
            ensure!(
                module.texts.is_empty(),
                "battle sequence cannot display messages yet"
            );
            let natives = crate::native_declarations();
            ensure!(
                module.natives.iter().all(|native| natives.contains(native)),
                "battle native declarations do not match this host"
            );
            Vm::validate_arguments(&action.program, action.entry, &[])?;
            for resource in &action.resources {
                match resource {
                    // Sounds are prepared by the host; tint entries are bytes.
                    ResourceBinding::Sound(_) | ResourceBinding::ActorTints(_) => {}
                    ResourceBinding::Voice(lines) => ensure!(
                        lines.len() == actors.len(),
                        "voice binding must cover the battle roster"
                    ),
                    ResourceBinding::Casting(definition) => {
                        definition.validate(&action.resources, action.tp_cost)?
                    }
                    ResourceBinding::Particle(definition) => {
                        definition.validate()?;
                        if let Some(model) = definition.model {
                            ensure!(
                                banks
                                    .get(&definition.resource)
                                    .is_some_and(|bank| bank.models.contains_key(&model.slot)),
                                "unprepared particle model"
                            );
                            if let Some(clip) = model.animation {
                                banks[&definition.resource].models[&model.slot]
                                    .validate_clip(clip)?;
                            }
                        }
                    }
                    ResourceBinding::EffectMotion(binding) => {
                        let model = banks
                            .get(&binding.bank)
                            .and_then(|bank| bank.models.get(&binding.model))
                            .ok_or_else(|| anyhow::anyhow!("unprepared effect model"))?;
                        model.validate_clip(binding.clip)?;
                    }
                    ResourceBinding::Projectile(definition) => {
                        definition.validate()?;
                        needs_knockdown |= definition
                            .contact
                            .as_ref()
                            .is_some_and(|contact| contact.hit.reaction.stagger != 0);
                        needs_stun |= definition
                            .contact
                            .as_ref()
                            .is_some_and(|contact| contact.hit.reaction.stun_chance != 0);
                        for appearance in definition
                            .birth
                            .into_iter()
                            .chain(definition.effects.ground)
                            .chain(definition.effects.trail.map(|(effect, _)| effect))
                            .chain(definition.contact.as_ref().and_then(|c| c.clash_effect))
                            .chain(
                                definition
                                    .contact
                                    .as_ref()
                                    .and_then(|c| c.hit.impact.map(|i| i.appearance)),
                            )
                        {
                            ensure!(
                                banks.get(&appearance.resource).is_some_and(|bank| bank
                                    .members
                                    .contains_key(&appearance.member)),
                                "unprepared projectile effect {}/{}",
                                appearance.resource,
                                appearance.member
                            );
                        }
                    }
                    ResourceBinding::Melee(definition) => {
                        definition.validate()?;
                        if let Some(impact) = definition.hit.impact {
                            ensure!(
                                banks
                                    .get(&impact.appearance.resource)
                                    .is_some_and(|bank| bank
                                        .members
                                        .contains_key(&impact.appearance.member)),
                                "unprepared contact impact effect"
                            );
                        }
                        needs_stun |= definition.hit.reaction.stun_chance != 0;
                        needs_knockdown |= definition.hit.reaction.stagger != 0;
                    }
                    ResourceBinding::WeaponFlight(definition) => {
                        definition.validate()?;
                        ensure!(
                            models
                                .iter()
                                .flatten()
                                .any(|model| model.weapon_attachment(definition.slot).is_ok()),
                            "unprepared detached weapon slot"
                        );
                        if let Some(impact) = definition.hit.impact {
                            ensure!(
                                banks
                                    .get(&impact.appearance.resource)
                                    .is_some_and(|bank| bank
                                        .members
                                        .contains_key(&impact.appearance.member)),
                                "unprepared weapon impact effect"
                            );
                        }
                        needs_stun |= definition.hit.reaction.stun_chance != 0;
                        needs_knockdown |= definition.hit.reaction.stagger != 0;
                    }
                    ResourceBinding::OptionalMotion(bindings) => {
                        ensure!(
                            bindings.len() == actors.len(),
                            "optional motion/actor count differs"
                        );
                        for (binding, model) in bindings.iter().zip(&models) {
                            if let Some(binding) = binding {
                                ensure!(
                                    model
                                        .as_ref()
                                        .is_some_and(|m| m.definition.resource == binding.model
                                            && m.definition.motions.contains_key(&binding.clip)),
                                    "unprepared optional actor motion"
                                );
                            }
                        }
                    }
                    ResourceBinding::Motion(binding) => ensure!(
                        models
                            .iter()
                            .flatten()
                            .any(|m| m.definition.resource == binding.model
                                && m.definition.motions.contains_key(&binding.clip)),
                        "unprepared battle motion binding"
                    ),
                    ResourceBinding::Spell(id) => ensure!(
                        actions.iter().any(|a| a.id == *id
                            && a.phase == ActionPhase::Resident
                            && a.tp_cost == 0),
                        "unprepared battle spell binding {id}"
                    ),
                    ResourceBinding::Effect(resource) => ensure!(
                        banks.contains_key(resource),
                        "unprepared battle effect bank {resource}"
                    ),
                }
            }
        }
        if needs_knockdown {
            ensure!(
                actors.iter().zip(&models).all(|(actor, model)| (!actor
                    .reaction
                    .profile
                    .can_knock_down
                    && !matches!(
                        actor.activity,
                        crate::Activity::KnockedDown | crate::Activity::GettingUp
                    ))
                    || model
                        .as_ref()
                        .is_some_and(|m| m.definition.knockdown.is_some())),
                "staggering actions require prepared actor knockdown resources"
            );
        }
        if needs_stun {
            ensure!(
                actors
                    .iter()
                    .zip(&models)
                    .all(|(actor, model)| (actor.reaction.stun.immune
                        && actor.activity != crate::Activity::Stunned)
                        || model.as_ref().is_some_and(|m| m.definition.stun.is_some())),
                "stunning actions require prepared actor stun resources"
            );
        }
        Ok(Self {
            decisions: vec![None; actors.len()],
            companions: vec![None; actors.len()],
            enemy_decisions: vec![None; actors.len()],
            enemy_selected: vec![None; actors.len()],
            entry_timers: vec![crate::EntryTimers::default(); actors.len()],
            blinking: vec![false; actors.len()],
            idle_expressions: vec![None; actors.len()],
            targets: actors
                .iter()
                .enumerate()
                .map(|(index, actor)| {
                    ActorId(
                        actors
                            .iter()
                            .position(|a| a.side != actor.side)
                            .unwrap_or(index) as u8,
                    )
                })
                .collect(),
            grade_rank: 0,
            trails: vec![Vec::new(); actors.len()],
            deaths: vec![None; actors.len()],
            death_feedback: None,
            contact_feedback: None,
            admission_flashes: BTreeMap::new(),
            contact_audio: None,
            controls: vec![None; actors.len()],
            ambient_color: [64; 3],
            arena_boundary: false,
            hover_sine: None,
            landing_effect: None,
            recovery_returns: vec![None; actors.len()],
            voices_enabled: true,
            entry_voice: None,
            voices: vec![Default::default(); actors.len()],
            stage_colors: Default::default(),
            camera: None,
            actors,
            actions,
            random_seed,
            models,
            effects: banks,
        })
    }

    /// Handles remain valid for the life of this encounter; actor slots are never reused.
    pub fn actor_ids(&self) -> impl Iterator<Item = ActorId> + '_ {
        (0..self.actors.len()).map(|index| ActorId(index as u8))
    }
}
