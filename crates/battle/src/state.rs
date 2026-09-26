use crate::{
    ActionPhase, PreparedBattle, ProjectileDefinition, ProjectileFrame, ProjectileId,
    contact::Contacts,
    melee::HitCache,
    projectile::Projectile,
    script::{ResidentPhase, Sequence, step_sequence},
};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActorId(pub(crate) u8);

impl ActorId {
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// Never reused within a battle, including after interruption or completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActionId(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Party,
    Enemy,
}

/// Original actors have two independently occupied released-spell slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellSlot {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactSource {
    Melee {
        actor: ActorId,
        action: ActionId,
    },
    Weapon {
        actor: ActorId,
        slot: u8,
        action: ActionId,
    },
    Projectile(ProjectileId),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoveryTraits {
    pub lucky: bool,
    pub boost: bool,
    pub weak: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub side: Side,
    pub control: crate::Control,
    pub activity: crate::Activity,
    pub availability: crate::ActorAvailability,
    pub guard: crate::Guard,
    pub hp: i32,
    pub max_hp: i32,
    pub tp: u16,
    pub max_tp: u16,
    /// Original 10A8 gauge units; persistent member percentages use / 10.
    pub overlimit: u16,
    pub overlimit_active: bool,
    pub hud: crate::ActorHud,
    pub luck: u8,
    pub stats: crate::CombatStats,
    pub elements: crate::AttackElements,
    pub affinities: [crate::Affinity; 9],
    /// Current physical combo percentage; projectiles retain it at emission.
    pub attack_power: u16,
    /// Prepared physical-arte damage bonus (EX skill 134).
    pub physical_arte_boost: bool,
    pub recovery: RecoveryTraits,
    pub petrified: bool,
    pub position: [f32; 3],
    /// Original battle headings are degrees, increasing from +Z toward +X.
    pub heading: f32,
    /// Source18E4, distinct from movement18F0 and heading18D0. Actions and
    /// detached contacts copy this cache; only explicit facing updates rebuild it.
    pub facing_direction: [f32; 3],
    /// Profile scale for effects emitted on this actor, independent of body scale.
    pub effect_scale: f32,
    pub framing: crate::ActorFraming,
    pub body: crate::Body,
    pub movement: crate::Movement,
    pub reaction: crate::Reaction,
    /// Actor-local stop counter; common timers continue while commands are held.
    pub hit_stop: u8,
}

impl Actor {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.max_hp > 0 && (0..=self.max_hp).contains(&self.hp),
            "invalid battle HP"
        );
        ensure!(self.tp <= self.max_tp, "invalid battle TP");
        ensure!(self.overlimit <= 1000, "invalid Over Limit gauge");
        ensure!(self.hud.control_slot < 4, "invalid local control slot");
        ensure!(
            self.position
                .iter()
                .chain(&self.facing_direction)
                .all(|v| v.is_finite())
                && self.heading.is_finite(),
            "invalid battle position"
        );
        ensure!(
            self.effect_scale.is_finite() && self.effect_scale >= 0.,
            "invalid battle effect scale"
        );
        self.guard.validate()?;
        ensure!(
            self.framing.yaw_offset.is_finite()
                && self.framing.minimum_radius.is_finite()
                && self.framing.minimum_radius >= 0.,
            "invalid actor camera framing"
        );
        self.movement.validate()?;
        self.reaction.validate()?;
        self.body.validate()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ActionRequest {
    pub actor: ActorId,
    pub action: u16,
    pub target: ActorId,
}

/// Input for one simulation update. A paused menu advances neither action ages nor tasks.
#[derive(Debug, Default)]
pub struct BattleInput {
    pub controllers: Vec<crate::ControlInput>,
    pub actions: Vec<ActionRequest>,
    pub interrupt: Vec<ActionId>,
    pub menu_open: bool,
    /// Source command-strip pause: HUD/camera continue, gameplay callbacks hold.
    pub command_pause: bool,
    /// Audio completions are observed even while a menu holds actor callbacks.
    pub voices_finished: Vec<crate::VoiceId>,
    /// Actors whose struggle buttons were pressed on this update.
    pub stun_struggle: Vec<ActorId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    BattleEnding,
    Busy,
    Defeated,
    Petrified,
    InsufficientTp,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cue {
    Started {
        action: ActionId,
        actor: ActorId,
    },
    Rejected {
        actor: ActorId,
        reason: Rejection,
    },
    Completed {
        action: ActionId,
    },
    Released {
        action: ActionId,
        parent: ActionId,
        actor: ActorId,
        slot: SpellSlot,
    },
    Interrupted {
        action: ActionId,
    },
    ProjectileStarted {
        projectile: ProjectileId,
        action: ActionId,
    },
    ProjectileExpired {
        projectile: ProjectileId,
    },
    ParticleStarted {
        particle: crate::ParticleId,
        /// None for a particle retained by its actor's reaction controller.
        action: Option<ActionId>,
    },
    ParticleExpired {
        particle: crate::ParticleId,
    },
    ProjectileClashed {
        projectile: ProjectileId,
        other: ContactSource,
        position: [f32; 3],
    },
    Hit {
        source: ContactSource,
        actor: ActorId,
        hurt_point: u8,
        result: crate::HitResult,
    },
    /// The host resolves text from the prepared action. Presentation retains
    /// the independent window lifetime after interruption or completion.
    Notice {
        actor: ActorId,
        action: u16,
        duration: u16,
        kind: u8,
    },
    /// Contact-time values before defeat/recovery clears the victim's counters.
    Combo {
        actor: ActorId,
        hits: i32,
        damage: i32,
    },
    /// Presentation owns playback, never recovery arithmetic or action completion.
    Effect {
        action: ActionId,
        resource: u32,
        member: u16,
        position: [f32; 3],
        heading: f32,
    },
    Recovered {
        actor: ActorId,
        nominal: i16,
        applied: i32,
    },
    Voice {
        actor: ActorId,
        playback: crate::VoiceId,
        sound: crate::SoundBinding,
        position: [f32; 3],
        /// Source actor flag4: pan64 and no distance attenuation.
        centered: bool,
    },
    VoiceStopped {
        playback: crate::VoiceId,
    },
    Sound {
        actor: ActorId,
        sound: crate::SoundBinding,
        position: [f32; 3],
        priority: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleResult {
    Escaped,
    Victory,
    Defeat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BattleOutcome {
    pub result: BattleResult,
    pub actors: Vec<Actor>,
    pub random_state: u32,
}

/// One coherent state for drawing. No renderer can mutate simulation actors.
#[derive(Debug, Clone, PartialEq)]
pub struct BattleFrame {
    pub target_markers: Vec<crate::TargetMarkerFrame>,
    pub stun_markers: Vec<crate::StunMarkerFrame>,
    pub update: u64,
    /// Source156FC advances before selector holds; actor/combat update does not.
    pub hud_update: u32,
    pub hud_holds: crate::HudHolds,
    pub targets: Vec<Option<ActorId>>,
    pub target_selector: Option<ActorId>,
    pub actors: Vec<Actor>,
    pub models: Vec<crate::ModelFrame>,
    pub trails: Vec<crate::TrailFrame>,
    pub weapons: Vec<crate::WeaponFrame>,
    pub actions: Vec<(ActionId, ActorId, u32)>,
    pub projectiles: Vec<ProjectileFrame>,
    pub particles: Vec<crate::ParticleFrame>,
    pub scenes: Vec<crate::SceneFrame>,
    pub stage_colors: [Option<[u8; 4]>; 4],
    pub camera: Option<crate::CameraPose>,
    pub cues: Vec<Cue>,
    /// Recognition precedes the shared world visit; results still keep stepping.
    pub recognized_result: Option<BattleResult>,
    /// Delivered once. Subsequent calls after an outcome are rejected.
    pub outcome: Option<BattleOutcome>,
}

/// Battle-local LCG. Unsigned and signed consumers advance the same stream.
/// Source: Battle REL fn_1_4DE40 / fn_1_4DE7C.
#[derive(Debug, Clone, Copy)]
pub struct Random(pub(crate) u32);
impl Random {
    pub fn from_state(state: u32) -> Self {
        Self(state)
    }
    pub fn state(self) -> u32 {
        self.0
    }

    // A source RNG draw is infallible, unlike Iterator's optional next item.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u16 {
        self.0 = self.0.wrapping_mul(0x41c6_4e6d).wrapping_add(0x12_d687);
        (self.0 >> 16) as u16
    }
}

pub struct Battle {
    pub(crate) diagnostics: resonance_content::diagnostics::Diagnostics,
    pub(crate) diagnostic: bool,
    pub(crate) target_markers: Vec<crate::markers::TargetMarker>,
    pub(crate) drawn_target_markers: Vec<crate::TargetMarkerFrame>,
    pub(crate) approaches: Vec<Option<crate::approach::Approach>>,
    pub(crate) decisions: Vec<Option<crate::decision::Decision>>,
    pub(crate) idle_timers: Vec<i16>,
    pub(crate) fidget_timers: Vec<i16>,
    pub(crate) eye_expressions: Vec<u8>,
    pub(crate) targets: Vec<ActorId>,
    pub(crate) target_positions: Vec<[f32; 3]>,
    pub(crate) target_gaps: Vec<f32>,
    pub(crate) entry_linked: Option<Vec<bool>>,
    pub(crate) enemy_selected: Vec<Option<usize>>,
    pub(crate) ledger: crate::Ledger,
    pub(crate) weapon_flights: BTreeMap<(ActorId, u8), crate::weapon_flight::Flight>,
    pub(crate) trails: Vec<Vec<crate::trail::Trail>>,
    pub(crate) trail_timers: Vec<[u8; 8]>,
    pub(crate) controls: Vec<Option<crate::control::Controller>>,
    pub(crate) target_selector: Option<ActorId>,
    pub(crate) prepared: Arc<PreparedBattle>,
    pub(crate) last_defeated: Option<ActorId>,
    pub(crate) death_wait_consumed: bool,
    pub(crate) actors: Vec<Actor>,
    pub(crate) sequences: BTreeMap<ActionId, Sequence>,
    pub(crate) random: Random,
    pub(crate) melee: Vec<HitCache>,
    pub(crate) models: Vec<Option<crate::model::Model>>,
    pub(crate) projectiles: BTreeMap<ProjectileId, Projectile>,
    pub(crate) particles: BTreeMap<crate::ParticleId, crate::particle::Particle>,
    pub(crate) scenes: [Option<crate::scene::Scene>; 2],
    pub(crate) actors_visible: bool,
    pub(crate) stage_colors: crate::stage::StageColors,
    pub(crate) camera: Option<crate::camera::Camera>,
    update: u64,
    pub(crate) hud_update: u32,
    pub(crate) hud_holds: crate::HudHolds,
    pub(crate) next_action: u64,
    pub(crate) voices: Vec<crate::voice::Voice>,
    pub(crate) contact_feedback: Vec<crate::contact_feedback::State>,
    pub(crate) contact_audio_actors: Vec<crate::contact_audio::ActorState>,
    pub(crate) contact_audio_affinity: u16,
    pub(crate) next_voice: u64,
    next_projectile: u64,
    pub(crate) next_particle: i32,
    pub(crate) effects_in_flight: usize,
    pub(crate) terminal: crate::outcome::Terminal,
    pub(crate) ended: bool,
}

impl Battle {
    pub fn new(prepared: Arc<PreparedBattle>) -> Self {
        let mut actors = prepared.actors.clone();
        for actor in &mut actors {
            actor.hud = crate::ActorHud::initialize(actor);
        }
        let mut battle = Self {
            diagnostics: resonance_content::diagnostics::Diagnostics::new(true),
            diagnostic: false,
            target_markers: prepared
                .targets
                .iter()
                .map(|id| crate::markers::TargetMarker::new(&actors[id.index()]))
                .collect(),
            drawn_target_markers: Vec::new(),
            approaches: vec![None; prepared.actors.len()],
            decisions: prepared
                .decisions
                .iter()
                .map(|definition| definition.map(crate::decision::Decision::new))
                .collect(),
            idle_timers: vec![0; prepared.actors.len()],
            fidget_timers: prepared
                .entry_timers
                .iter()
                .map(|timer| timer.fidget_ticks as i16)
                .collect(),
            eye_expressions: vec![0; prepared.actors.len()],
            targets: prepared.targets.clone(),
            target_positions: prepared.actors.iter().map(|actor| actor.position).collect(),
            target_gaps: vec![0.; prepared.actors.len()],
            entry_linked: None,
            enemy_selected: prepared.enemy_selected.clone(),
            ledger: crate::Ledger::new(prepared.actors.len(), prepared.grade_rank),
            weapon_flights: BTreeMap::new(),
            trails: prepared.trails.clone(),
            trail_timers: vec![[0; 8]; prepared.actors.len()],
            controls: prepared
                .controls
                .iter()
                .map(|definition| definition.as_ref().map(crate::control::Controller::new))
                .collect(),
            target_selector: None,
            last_defeated: None,
            death_wait_consumed: false,
            voices: prepared.voices.clone(),
            contact_feedback: vec![Default::default(); prepared.actors.len()],
            contact_audio_actors: vec![Default::default(); prepared.actors.len()],
            contact_audio_affinity: 0,
            next_voice: 1,
            actors,
            melee: vec![HitCache::default(); prepared.actors.len()],
            random: Random(prepared.random_seed),
            models: prepared.models.clone(),
            stage_colors: prepared.stage_colors.clone(),
            camera: prepared.camera.clone(),
            prepared,
            sequences: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            particles: BTreeMap::new(),
            scenes: [None, None],
            actors_visible: true,
            update: 0,
            hud_update: 0,
            hud_holds: Default::default(),
            next_action: 1,
            next_projectile: 1,
            next_particle: 1,
            effects_in_flight: 0,
            terminal: Default::default(),
            ended: false,
        };
        battle.drawn_target_markers = battle.target_marker_frames();
        battle
    }

    pub fn actors(&self) -> &[Actor] {
        &self.actors
    }

    /// Share the startup error policy and diagnostic history with the host.
    /// Standalone battles retain strict validation unless configured explicitly.
    pub fn set_diagnostics(&mut self, diagnostics: resonance_content::diagnostics::Diagnostics) {
        self.diagnostics = diagnostics;
    }

    /// A skipped simulation fault makes this run unsuitable for persistent
    /// results. Unrelated host or presentation diagnostics do not set this flag.
    pub fn is_diagnostic(&self) -> bool {
        self.diagnostic
    }

    /// Present the held initial/current state without consuming a simulation visit.
    pub fn snapshot(&self) -> BattleFrame {
        self.frame(vec![], None)
    }
    /// Ordinary camera lead-in; all entry visits use the battle's update clock.
    pub fn entry_pending(&self) -> bool {
        self.camera
            .as_ref()
            .is_some_and(crate::camera::Camera::entry_pending)
    }

    /// 381C/2108 excludes entry/results and stored-scene pause bits1|2.
    /// The admitted zero-override route has no scripted restriction owner or
    /// screen-shake writer; game preparation verifies its action block bits.
    pub fn command_admission_allowed(&self) -> bool {
        !self.entry_pending()
            && self.phase() == crate::BattlePhase::Combat
            && self.scenes.iter().all(Option::is_none)
    }

    pub fn random_state(&self) -> u32 {
        self.random.0
    }
    pub(crate) fn projectile_count(&self) -> usize {
        self.projectiles.len()
    }

    pub fn action_age(&self, handle: ActionId) -> Option<u32> {
        self.sequences.get(&handle).map(|s| s.age)
    }

    pub(crate) fn interrupt_actor(&mut self, actor: ActorId, cues: &mut Vec<Cue>) {
        self.actors[actor.index()].hud.cast_released = false;
        self.sequences.retain(|&action, sequence| {
            let interrupted = sequence.actor == actor && sequence.definition.phase.is_actor();
            if interrupted {
                cues.push(Cue::Interrupted { action });
            }
            !interrupted
        });
    }

    /// Source callbacks observe age before the inclusive duration check and increment
    /// (fn_1_37E48). Task return and action expiry remain distinct events.
    pub fn step(&mut self, input: BattleInput) -> Result<BattleFrame> {
        ensure!(!self.ended, "battle has ended or faulted");
        // Reject stale/external input before mutating any state.
        self.validate_controls(&input.controllers)?;
        for request in &input.actions {
            self.actor(request.actor)?;
            self.actor(request.target)?;
            ensure!(
                self.prepared.actions.iter().any(|a| a.id == request.action
                    && !matches!(a.phase, ActionPhase::Controller | ActionPhase::Decision)),
                "unknown battle action {}",
                request.action
            );
        }
        for action in &input.interrupt {
            ensure!(
                self.sequences.contains_key(action),
                "stale battle action handle"
            );
        }
        for &actor in &input.stun_struggle {
            self.actor(actor)?;
        }
        self.complete_voices(&input.voices_finished)?;
        if input.menu_open {
            return Ok(self.frame(Vec::new(), None));
        }
        if input.command_pause {
            let result = self.advance_command_pause();
            if result.is_err() {
                self.terminal_fault();
            }
            return result;
        }
        if self.target_selector.is_some() {
            let result = self
                .control_target_selector(&input.controllers)
                .map(|()| self.frame(Vec::new(), None));
            if result.is_err() {
                self.terminal_fault();
            }
            return result;
        }
        let result = self.advance(input);
        if result.is_err() {
            self.terminal_fault();
        }
        result
    }

    /// A fault is terminal; partially executed gameplay cannot continue or
    /// apply an outcome. Dropping every VM invalidates pending completions.
    fn terminal_fault(&mut self) {
        self.sequences.clear();
        self.projectiles.clear();
        self.retire_weapon_flights();
        self.particles.clear();
        self.voices.fill(Default::default());
        self.scenes = [None, None];
        self.actors_visible = true;
        self.ended = true;
    }

    /// Source 1C40/381C command visit: shared HUD/camera clocks advance while
    /// actor callbacks, actions, contacts, objects, and result recognition hold.
    fn advance_command_pause(&mut self) -> Result<BattleFrame> {
        if let Some(camera) = &mut self.camera {
            camera.step_command_pause(&self.actors)?;
        }
        // 1C40 draws through 145C before its command pause return. 5200C
        // selects mode2/extra0: retain tracks and prior placement, but still
        // compose bodies/weapons and consume queued secondary acceleration.
        for (index, (model, actor)) in self.models.iter_mut().zip(&mut self.actors).enumerate() {
            if let Some(model) = model {
                model.step(actor, false, crate::model::PlacementUpdate::Held)?;
                let mut tint = model.shown.tint;
                self.contact_feedback[index]
                    .appearance(&mut tint, Some(self.prepared.ambient_color));
                model.sample_tint(tint);
            }
        }
        self.advance_hud(true);
        self.update += 1;
        Ok(self.frame(Vec::new(), None))
    }

    fn advance(&mut self, input: BattleInput) -> Result<BattleFrame> {
        // 1C40 fills both banks of center distances before actor dispatch.
        self.target_positions = self.actors.iter().map(|actor| actor.position).collect();
        let ordinary_entry = self.entry_pending();
        if !ordinary_entry && self.phase() == crate::BattlePhase::Combat {
            // 381C calls E78 before outcome recognition and actor callbacks.
            self.contact_audio_affinity = self.contact_audio_affinity.saturating_sub(1);
        }
        let mut cues = Vec::new();
        self.recognize_result();
        // Model evaluation precedes actor callbacks. Bindings made below become
        // observable on the following visit; `shown` retains the drawing sample.
        let transition = self.transition_owner();
        // The outer battle callback updates framing before model/actor dispatch.
        if let Some(camera) = &mut self.camera {
            camera.step(&self.actors, transition)?;
        }
        // 3EA4 enters state 3 on camera arrival before its 1C40(0) visit.
        self.ledger
            .advance(!self.entry_pending() && self.phase() == crate::BattlePhase::Combat);
        if ordinary_entry && !self.entry_pending() {
            // 3EA4 first enables state 3 with C908, then overwrites idle delays
            // before this same visit's object/actor dispatch.
            self.initialize_entry_delays();
        }
        let ordinary_entry = self.entry_pending();
        for (index, (model, actor)) in self.models.iter_mut().zip(&mut self.actors).enumerate() {
            if let Some(model) = model {
                // Local hit-stop and petrification still place the body;
                // transition non-owners retain the preceding placement.
                let placement = match transition {
                    None => crate::model::PlacementUpdate::Actor,
                    Some(owner) if owner.index() == index => {
                        crate::model::PlacementUpdate::TransitionOwner
                    }
                    Some(_) => crate::model::PlacementUpdate::Held,
                };
                model.step(
                    actor,
                    !actor.petrified && transition.is_none_or(|owner| owner.index() == index),
                    placement,
                )?;
                let mut tint = model.shown.tint;
                self.contact_feedback[index]
                    .appearance(&mut tint, transition.map(|_| self.prepared.ambient_color));
                model.sample_tint(tint);
            }
        }
        // 1C40 draws current bodies through145C before70AE4 follows their
        // target anchors. HP/TP still sample before all actor callbacks.
        self.advance_hud(false);
        for action in input.interrupt {
            if let Some(sequence) = self.sequences.remove(&action) {
                if sequence.definition.phase.is_actor() {
                    self.actors[sequence.actor.index()].hud.cast_released = false;
                    self.actors[sequence.actor.index()].activity = crate::Activity::Idle;
                    self.actors[sequence.actor.index()].reaction.armor.reset();
                }
                cues.push(Cue::Interrupted { action });
            }
        }
        self.clean_scenes();
        for request in input.actions {
            self.start(request, &mut cues)?;
        }
        let mut contacts = Contacts::default();
        if !ordinary_entry && self.transition_owner().is_none() {
            self.advance_sequences(
                ActionPhase::Actor,
                &input.stun_struggle,
                &input.controllers,
                &mut contacts,
                &mut cues,
            )?;
        } else if ordinary_entry && self.transition_owner().is_none() {
            for index in 0..self.actors.len() {
                self.advance_actor_common(index, &mut cues)?;
            }
        }
        if self.transition_owner().is_none() {
            for (model, actor) in self.models.iter_mut().zip(&self.actors) {
                if let Some(model) = model {
                    model.sample_shadow(actor);
                }
            }
        }
        // Persistent weapon ribbons run in group two using the sampled pose.
        if self.transition_owner().is_none() {
            for (index, trails) in self.trails.iter_mut().enumerate() {
                if trails.is_empty() {
                    continue;
                }
                let Some(model) = self.models[index].as_ref() else {
                    self.diagnostics.report(
                        "battle weapon trails",
                        anyhow::anyhow!("weapon trails require actor model"),
                    )?;
                    self.diagnostic = true;
                    trails.clear();
                    self.trail_timers[index] = [0; 8];
                    continue;
                };
                let weapons = model.weapon_frames();
                let pose = (self.actors[index].hit_stop == 0)
                    .then_some((&model.shown, weapons.as_slice()));
                let mut trail_index = 0;
                while trail_index < trails.len() {
                    let trail = &mut trails[trail_index];
                    let slot = usize::from(trail.slot());
                    if let Err(error) = trail.tick(self.trail_timers[index][slot], pose) {
                        self.diagnostics.report("battle weapon trail", error)?;
                        self.diagnostic = true;
                        self.trail_timers[index][slot] = 0;
                        trails.remove(trail_index);
                    } else {
                        trail_index += 1;
                    }
                }
            }
        }
        // fn_1_12470 prepends to group 3; fn_1_11E8C walks newest first.
        // Resident spell callbacks follow the object groups (fn_1_1C40).
        // Their new projectiles remain pending until this phase next runs.
        if self.transition_owner().is_none() {
            let ids: Vec<_> = self.projectiles.keys().rev().copied().collect();
            for id in ids {
                let shadow_available = self.object_available();
                if self.projectiles[&id].retiring {
                    self.expire_projectile(id, &mut cues);
                } else {
                    let result = (|| {
                        let projectile = self.projectiles.get_mut(&id).unwrap();
                        if !projectile.initialized {
                            // 13B8C allocates its shadow before the birth effect.
                            projectile.frame.shadow = projectile
                                .definition
                                .effects
                                .shadow
                                .filter(|_| shadow_available);
                            let birth = projectile.initialize(&mut cues);
                            if let Some(birth) = birth {
                                self.show_effect(birth, &mut cues)?;
                            }
                        }
                        let projectile = self.projectiles.get_mut(&id).unwrap();
                        projectile.step()?;
                        contacts
                            .submit(projectile, self.actors[projectile.frame.owner.index()].side)?;
                        let effects = projectile.effects();
                        for effect in effects {
                            self.show_effect(effect, &mut cues)?;
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        self.diagnostics.report("battle projectile", error)?;
                        self.diagnostic = true;
                        for list in &mut contacts.0 {
                            list.retain(|contact| contact.source != ContactSource::Projectile(id));
                        }
                        self.expire_projectile(id, &mut cues);
                    }
                }
            }
            self.advance_effects(false, &mut cues)?;
            self.advance_particles(false, &mut cues)?;
        }
        self.advance_effects(true, &mut cues)?;
        self.advance_particles(true, &mut cues)?;
        // 1C40 visits the stage after object groups, before contacts/residents.
        self.stage_colors.step();
        // fn_1_1C40: contacts resolve after object groups, before residents.
        if !ordinary_entry && self.transition_owner().is_none() {
            contacts.resolve(self, &mut cues)?;
            let defeated: Vec<_> = self
                .sequences
                .iter()
                .filter(|(_, s)| {
                    s.definition.phase.is_actor()
                        && s.definition.phase != ActionPhase::Controller
                        && !self.actors[s.actor.index()].available()
                })
                .map(|(&id, _)| id)
                .collect();
            for action in defeated {
                self.sequences.remove(&action);
                cues.push(Cue::Interrupted { action });
            }
            self.advance_sequences(ActionPhase::Resident, &[], &[], &mut contacts, &mut cues)?;
        }
        self.clean_scenes();
        self.advance_scene(&mut cues)?;
        // Model particles are evaluated by the original drawing pass, after
        // their motion callback. Keep that visit on simulation time.
        self.advance_effect_models(&mut cues)?;
        self.update += 1;
        Ok(self.frame(cues, None))
    }

    fn advance_sequences(
        &mut self,
        phase: ActionPhase,
        struggle: &[ActorId],
        input: &[crate::ControlInput],
        contacts: &mut Contacts,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if phase == ActionPhase::Resident {
            // 3A978 visits party before enemies, primary before secondary. Check
            // occupancy at each visit, rather than sorting by creation time.
            for side in [Side::Party, Side::Enemy] {
                for index in 0..self.actors.len() {
                    if self.actors[index].side != side {
                        continue;
                    }
                    for slot in [SpellSlot::Primary, SpellSlot::Secondary] {
                        let id = self
                            .sequences
                            .iter()
                            .find(|(_, s)| {
                                s.actor.index() == index
                                    && s.resident.as_ref().is_some_and(|r| r.slot == slot)
                            })
                            .map(|(&id, _)| id);
                        if let Some(id) = id {
                            self.advance_sequence(id, contacts, cues)?;
                        }
                    }
                }
            }
            return Ok(());
        }
        for index in 0..self.actors.len() {
            // 11E8C continues the list after a transition begins, but each
            // remaining 31C88 actor callback returns before sampling/dispatch.
            if self.transition_owner().is_some() {
                break;
            }
            let actor = ActorId(index as u8);
            // 31C88 -> 4DA50 reads both retained197C ground centers before
            // refreshing this actor's195C/197C. Later actors see earlier visits.
            if let Some(target) = self.target(actor) {
                self.actors[index].movement.target_direction = crate::distance::planar_direction(
                    self.actors[target.index()].body.center,
                    self.actors[index].body.center,
                    self.actors[index].movement.target_direction,
                );
            }
            self.actors[index].sample_center()?;
            self.initialize_dead_controller(actor, cues)?;
            if self.actors[index].petrified {
                continue;
            }
            let callback_sequence = self.sequences.iter().find_map(|(&id, sequence)| {
                (sequence.actor == actor
                    && sequence.definition.phase == ActionPhase::Actor
                    && sequence.recovery.is_none()
                    && !sequence.action_recovery
                    && sequence.attached.is_none())
                .then_some((id, sequence.age, sequence.command_age))
            });
            let (action_callback_ready, facing_return) = self.action_callback_ready(actor);
            self.control_actor(
                actor,
                input
                    .iter()
                    .find(|input| input.actor == actor)
                    .copied()
                    .unwrap_or_else(|| crate::ControlInput::neutral(actor)),
                action_callback_ready,
                cues,
            )?;
            self.initialize_decision(actor)?;
            let decisions: Vec<_> = self
                .sequences
                .iter()
                .filter(|(_, s)| s.actor == actor && s.definition.phase == ActionPhase::Decision)
                .map(|(&id, _)| id)
                .collect();
            for id in decisions {
                self.advance_sequence(id, contacts, cues)?;
            }
            // Jump landing selects callback2 after this visit's control phase;
            // 31B68 must first run on the following visit.
            if !self.controls[index].as_ref().is_some_and(|control| {
                control.mobility == Some(crate::mobility::Mobility::Finished)
            }) {
                self.initialize_unhandled_idle(actor)?;
            }
            // 31C88 refreshes actor+18ac after its control callback. Eligibility
            // above consequently observes the previous visit's body gap.
            let body_pair = if let Some(target) = self.target(actor) {
                self.target_gaps[index] =
                    crate::control::body_gap(&self.actors[index], &self.actors[target.index()]);
                !self.actors[index].body.approach_points.is_empty()
                    && !self.actors[target.index()].body.approach_points.is_empty()
            } else {
                false
            };
            self.advance_recovery_return(actor)?;
            self.advance_approach(actor, cues)?;
            let activity = self.actors[index].activity;
            // Retained death integrates independently of Controller age,
            // hit-stop and rest-motion blends. Its operand uses this visit's
            // sampled body yaw and the pair actually evaluated above.
            let death_return = if activity == crate::Activity::Defeated
                && self.actors[index].availability == crate::ActorAvailability::Dead
                && self.prepared.deaths[index].is_some_and(|death| death.integrate)
                && self.actors[index].side == Side::Party
                && self.actors[index].control == crate::Control::Auto
                && self
                    .actors
                    .iter()
                    .any(|actor| actor.side == Side::Party && actor.control != crate::Control::Auto)
                && body_pair
            {
                self.models[index].as_ref().and_then(|model| {
                    crate::weapon_flight::ReturnSteering::retained_death(model.sampled_heading())
                })
            } else {
                None
            };
            let (normal, normal_entry) = self.normal_visit(index);
            let local_hit_stop = self.actors[index].hit_stop != 0;
            let ids: Vec<_> = self
                .sequences
                .iter()
                .filter(|(_, s)| s.actor == actor && s.definition.phase.is_actor())
                .map(|(&id, _)| id)
                .collect();
            for id in ids {
                if matches!(
                    activity,
                    crate::Activity::Hurt
                        | crate::Activity::KnockedDown
                        | crate::Activity::GettingUp
                        | crate::Activity::Stunned
                ) {
                    self.sequences.remove(&id);
                    cues.push(Cue::Interrupted { action: id });
                } else if !action_callback_ready
                    && self.sequences[&id].definition.phase == ActionPhase::Actor
                {
                    // 30B4C still reaches floor/common/detached-weapon work,
                    // but does not dispatch the normal/martial actor callback.
                    continue;
                } else {
                    self.advance_sequence(id, contacts, cues)?;
                }
            }
            if phase == ActionPhase::Actor {
                if activity == crate::Activity::Defeated {
                    self.advance_dead_motion(index);
                }
                if matches!(
                    activity,
                    crate::Activity::KnockedDown | crate::Activity::GettingUp
                ) && !self.actors[index].petrified
                {
                    self.advance_knockdown(actor)?;
                }
                if activity == crate::Activity::Stunned && !self.actors[index].petrified {
                    self.advance_stun(actor, struggle.contains(&actor), cues)?;
                }
                // The callback still integrates while the model blend holds its
                // commands. Recovery has no local-hit-stop gate (301A4).
                let control_holds_movement =
                    !action_callback_ready || self.control_holds_movement(index);
                let guard_turn_step = self.prepared.controls[index]
                    .as_ref()
                    .map(|d| d.turn_ticks)
                    .or_else(|| {
                        self.prepared.enemy_decisions[index]
                            .as_ref()
                            .map(|d| d.turn_ticks)
                    })
                    .map(|ticks| 180. / f32::from(ticks));
                let actor = &mut self.actors[index];
                if !actor.petrified && activity == crate::Activity::Hurt {
                    crate::reaction::advance_hurt(actor, &mut self.random, &mut self.ledger);
                } else if !actor.petrified
                    && activity == crate::Activity::Guarding
                    && actor.guard.kind == crate::GuardKind::Normal
                    && matches!(actor.control, crate::Control::Auto | crate::Control::Enemy)
                {
                    if let Some(airborne) =
                        crate::reaction::advance_guard(actor, &mut self.random, &mut self.ledger)
                        && let Some(model) = &mut self.models[index]
                    {
                        model.guard(airborne)?;
                    }
                    if let Some(step) = guard_turn_step {
                        let direction = actor.facing_direction;
                        crate::control::face_cached(actor, direction, step);
                    }
                } else if !actor.petrified
                    && !control_holds_movement
                    && !matches!(
                        activity,
                        crate::Activity::Stunned
                            | crate::Activity::Defeated
                            | crate::Activity::KnockedDown
                            | crate::Activity::GettingUp
                    )
                    && (actor.hit_stop == 0 || !matches!(activity, crate::Activity::Action { .. }))
                {
                    actor.movement.integrate(&mut actor.position, [0.; 2]);
                    if matches!(
                        activity,
                        crate::Activity::Action { .. } | crate::Activity::Recovering
                    ) {
                        actor
                            .movement
                            .brake(actor.position[1], activity, false, false);
                    }
                }
                let hover = self
                    .recovery_return_hover(index)
                    .or_else(|| self.approach_hover(index))
                    .or_else(|| (activity == crate::Activity::Idle).then_some(true));
                if !control_holds_movement && let Some(bob) = hover {
                    self.advance_hover(index, bob)?;
                }
                self.after_recovery_return_integration(index)?;
                let normal_landing = action_callback_ready
                    && normal
                    && self.actors[index].movement.airborne_action
                    && self.actors[index].position[1] <= 0.1;
                if action_callback_ready {
                    self.brake_control(index)?;
                }
                if activity != crate::Activity::Idle
                    && self.actors[index].activity == crate::Activity::Idle
                {
                    self.idle_timers[index] = self.actors[index].reaction.remaining;
                    self.restore_idle_expression(index);
                }
                let boundary = self.constrain_actor(index);
                // These are semantic values written by the original callback
                // chain, not an emulation of addresses or a last-motion fallback.
                let outbound_point = match activity {
                    crate::Activity::Hurt => Some([
                        boundary
                            .translation
                            .map_or(0., |translation| translation[2]),
                        0.,
                        0.,
                    ]),
                    crate::Activity::Defeated => self.models[index].as_ref().map(|model| {
                        crate::weapon_flight::sampled_heading_point(model.sampled_heading())
                    }),
                    crate::Activity::Action { .. } if !action_callback_ready => {
                        crate::weapon_flight::facing_point(self.actors[index].movement.direction)
                    }
                    crate::Activity::Action { .. } if normal => {
                        Some(if normal_entry || normal_landing || local_hit_stop {
                            [0.; 3]
                        } else {
                            self.actors[index].movement.translation
                        })
                    }
                    crate::Activity::Action { .. } if !local_hit_stop => {
                        self.models[index].as_ref().map(|model| {
                            crate::weapon_flight::sampled_heading_point(model.sampled_heading())
                        })
                    }
                    crate::Activity::Action { .. } => {
                        // 37FD4 -> 2C5B4 -> 63D84 retains the preceding
                        // grounded 24D24 input across its local-hit-stop return.
                        crate::weapon_flight::facing_point(self.actors[index].movement.direction)
                    }
                    _ => None,
                };
                let outbound_point = if boundary.leader_outside
                    && matches!(
                        activity,
                        crate::Activity::Action { .. } | crate::Activity::Defeated
                    ) {
                    Some([0.; 3])
                } else {
                    outbound_point
                };
                // These are proved only for the prepared opening actions. A
                // non-Auto party member proves this Auto actor is not the
                // source-selected arena leader.
                let tiny_return = death_return.or_else(|| {
                    callback_sequence.and_then(|(id, age, command_age)| {
                        if self.actors[index].side != Side::Party
                            || self.actors[index].control != crate::Control::Auto
                            || !self.actors.iter().any(|actor| {
                                actor.side == Side::Party && actor.control != crate::Control::Auto
                            })
                            || !matches!(activity, crate::Activity::Action { .. })
                            || normal_landing
                        {
                            return None;
                        }
                        let sequence = self.sequences.get(&id)?;
                        if sequence.recovery.is_some()
                            || sequence.action_recovery
                            || sequence.attached.is_some()
                        {
                            return None;
                        }
                        if sequence.age == age
                            && (!action_callback_ready
                                || local_hit_stop && age > 0 && !normal_entry)
                        {
                            facing_return
                        } else if action_callback_ready
                            && !local_hit_stop
                            && age > 0
                            && !normal_entry
                            && sequence.command_age == command_age.wrapping_add(1)
                        {
                            // Opening normal rows disable the root-delta writer,
                            // leaving zero; advancing martial callbacks leave a
                            // zero or unordered desired length. Both retain the
                            // old direction instead of the earlier facing input.
                            // The same sequence's clock excludes model holds,
                            // phase zero, completion and action replacement.
                            Some(crate::weapon_flight::ReturnSteering::KeepDirection)
                        } else {
                            None
                        }
                    })
                });
                self.landing_effect(index, cues)?;
                self.finish_recovery_return(index)?;
                let actor = &mut self.actors[index];
                ensure!(
                    actor.position.iter().all(|v| v.is_finite()),
                    "battle movement overflow"
                );
                actor.movement.validate()?;
                self.advance_actor_common(index, cues)?;
                self.finish_approach(index)?;
                // 31C88 always visits 21E94 after the common actor update;
                // local hit-stop holds the owner action, not its detached slots.
                self.advance_weapon_flights(
                    ActorId(index as u8),
                    outbound_point,
                    tiny_return,
                    contacts,
                )?;
            }
        }
        Ok(())
    }

    fn advance_sequence(
        &mut self,
        id: ActionId,
        contacts: &mut Contacts,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let sequence = &self.sequences[&id];
        let (actor, phase) = (sequence.actor, sequence.definition.phase);
        if let Err(error) = self.advance_sequence_inner(id, contacts, cues) {
            self.diagnostics.report("battle action", error)?;
            self.diagnostic = true;
            // The VM may already have consumed commands. Cancel its lifetime;
            // never retry a partially executed task or keep its owned objects.
            self.discard_action(id, actor, phase, cues);
            self.clean_scenes();
            for list in &mut contacts.0 {
                list.retain(|contact| match contact.source {
                    ContactSource::Melee { action, .. } | ContactSource::Weapon { action, .. } => {
                        action != id
                    }
                    ContactSource::Projectile(projectile) => {
                        self.projectiles.contains_key(&projectile)
                    }
                });
            }
        }
        Ok(())
    }

    pub(crate) fn discard_action(
        &mut self,
        id: ActionId,
        actor: ActorId,
        phase: ActionPhase,
        cues: &mut Vec<Cue>,
    ) {
        self.sequences.remove(&id);
        if phase.is_actor() && phase != ActionPhase::Controller {
            self.actors[actor.index()].activity = crate::Activity::Idle;
            self.actors[actor.index()].reaction.armor.reset();
            self.actors[actor.index()].hud.cast_released = false;
            self.melee[actor.index()].clear();
        }
        let flights: Vec<_> = self
            .weapon_flights
            .iter()
            .filter(|(_, flight)| flight.action == id)
            .map(|(&key, _)| key)
            .collect();
        for (owner, slot) in flights {
            self.retire_weapon_flight(owner, slot);
        }
        let projectiles: Vec<_> = self
            .projectiles
            .iter()
            .filter(|(_, projectile)| projectile.action == id)
            .map(|(&id, _)| id)
            .collect();
        for projectile in projectiles {
            self.expire_projectile(projectile, cues);
        }
        self.particles.retain(|particle_id, particle| {
            if particle.action == Some(id) {
                cues.push(Cue::ParticleExpired {
                    particle: *particle_id,
                });
                false
            } else {
                true
            }
        });
        cues.push(Cue::Interrupted { action: id });
    }

    fn advance_sequence_inner(
        &mut self,
        id: ActionId,
        contacts: &mut Contacts,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let mut sequence = self.sequences.remove(&id).unwrap();
        let phase = sequence.definition.phase;
        if phase == ActionPhase::Actor {
            self.face_normal_entry(id, sequence.actor)?;
        }
        if sequence
            .resident
            .as_ref()
            .is_some_and(|r| r.phase == ResidentPhase::Retiring)
        {
            // 37E48 retains occupancy at the inclusive final callback. 37DD8
            // completes on the following dispatch without running that callback.
            cues.push(Cue::Completed { action: id });
            return Ok(());
        }
        if phase.is_actor()
            && phase != ActionPhase::Controller
            && !self.actor(sequence.actor)?.available()
        {
            cues.push(Cue::Interrupted { action: id });
            return Ok(());
        }
        let recovering = sequence.recovery.is_some();
        if let Some(remaining) = sequence.recovery.as_mut() {
            let actor = self.actor(sequence.actor)?;
            if *remaining <= 0
                && (actor.position[1] <= 0.1
                    || actor.movement.hover_height != 0.
                    || actor.movement.fixed_height)
            {
                sequence.recovery = None;
            } else {
                *remaining = remaining.wrapping_sub(1);
            }
        }
        let held = sequence.recovery.is_some()
            || phase == ActionPhase::Actor
                && !recovering
                && (self.actors[sequence.actor.index()].hit_stop != 0
                    || self.models[sequence.actor.index()]
                        .as_ref()
                        .is_some_and(|m| m.blending()));
        if !held {
            sequence.hit_row = false;
            sequence.animation_row = false;
            sequence.animation_held = false;
            step_sequence(self, id, &mut sequence, cues)?;
        }
        if !held
            && !recovering
            && sequence.recovery.is_some()
            && phase == ActionPhase::Actor
            && self.chain_normal(id, &sequence, cues)?
        {
            return Ok(());
        }
        let held = held
            || sequence.recovery.is_some()
            || phase == ActionPhase::Actor
                && !recovering
                && !sequence.animation_row
                && self.models[sequence.actor.index()]
                    .as_ref()
                    .is_some_and(|m| m.blending());
        // The embedded actor effect runs during model blends, after commands,
        // and before movement/completion; recovery no longer visits 2C5B4.
        if held && !recovering && phase == ActionPhase::Actor {
            self.advance_attached(id, &mut sequence, cues)?;
        }
        if !held {
            if let Some(resident) = &mut sequence.resident
                && resident.phase == ResidentPhase::Initializing
                && !sequence.finished
            {
                // The initializer binds state and leaves age zero. The first
                // active callback runs on the next resident dispatch.
                resident.phase = ResidentPhase::Active;
                self.sequences.insert(id, sequence);
                return Ok(());
            }
            if let Some(launch) = &sequence.weapon_launch {
                if sequence.hit_age >= launch.start {
                    self.throw_weapon(sequence.actor, id, Arc::clone(&launch.definition))?;
                    sequence.weapon_launch = None;
                }
                // Negative emissions consume their row and increment on the
                // launch visit, unlike an attached window's inclusive end.
                sequence.hit_age = sequence.hit_age.wrapping_add(1);
            } else if let Some(window) = &sequence.melee {
                let clock = sequence.hit_age;
                if clock == window.start {
                    self.melee[sequence.actor.index()].clear();
                    if let Some(slot) = window.definition.trail {
                        let timer =
                            &mut self.trail_timers[sequence.actor.index()][usize::from(slot)];
                        // 2D564 extends only the first source attachment's timer.
                        let duration = (i32::from(window.end) - i32::from(window.start)) * 4;
                        if i32::from(*timer) < duration {
                            *timer = duration as u8;
                        }
                    }
                }
                if (window.start..=window.end).contains(&clock) {
                    contacts.melee(
                        sequence.actor,
                        id,
                        self.actor(sequence.actor)?,
                        self.models[sequence.actor.index()].as_ref(),
                        &window.definition,
                    )?;
                }
                // Advancing a row holds its inclusive end clock (2D564).
                if clock == window.end {
                    sequence.melee = None;
                } else {
                    sequence.hit_age = sequence.hit_age.wrapping_add(1);
                }
            } else if sequence.hit_waiting || sequence.hit_row {
                sequence.hit_age = sequence.hit_age.wrapping_add(1);
            }
            if phase == ActionPhase::Actor && self.chain_normal(id, &sequence, cues)? {
                return Ok(());
            }
            if !recovering && phase == ActionPhase::Actor {
                self.advance_attached(id, &mut sequence, cues)?;
            }
            if phase == ActionPhase::Actor && !recovering && sequence.action_end_ready() {
                crate::script::step_action_end(self, id, &mut sequence, cues)?;
                if sequence.recovery.is_some() {
                    self.sequences.insert(id, sequence);
                    return Ok(());
                }
            }
            if !sequence.finished
                && sequence.age >= u32::from(sequence.definition.duration)
                && let Some(resident) = &mut sequence.resident
                && resident.retained
            {
                resident.phase = ResidentPhase::Retiring;
            } else if sequence.finished
                || !matches!(
                    phase,
                    ActionPhase::Casting | ActionPhase::Controller | ActionPhase::Decision
                ) && sequence.age >= u32::from(sequence.definition.duration)
            {
                if phase.is_actor() && phase != ActionPhase::Controller {
                    if sequence.action_recovery {
                        self.complete_ordinary_action(sequence.actor, cues)?;
                    } else {
                        // Task completion/expiry releases the actor slot. Only
                        // an authored Recover enters 301A4 -> 2B18C; an effect
                        // launcher finishing must not consume its guard RNG.
                        let actor = &mut self.actors[sequence.actor.index()];
                        actor.activity = crate::Activity::Idle;
                        actor.reaction.armor.reset();
                        actor.hud.cast_released = false;
                    }
                }
                cues.push(Cue::Completed { action: id });
                return Ok(());
            }
            sequence.age += 1;
            if phase == ActionPhase::Actor {
                sequence.command_age = sequence.command_age.wrapping_add(1);
                if !sequence.animation_held && !sequence.animation_ended {
                    sequence.animation_age = sequence.animation_age.wrapping_add(1);
                }
            }
            if let crate::Activity::Action { clock, .. } =
                &mut self.actors[sequence.actor.index()].activity
                && phase == ActionPhase::Actor
            {
                *clock = sequence.age as i16;
            }
        }
        self.sequences.insert(id, sequence);
        Ok(())
    }

    pub(crate) fn frame(&self, cues: Vec<Cue>, outcome: Option<BattleOutcome>) -> BattleFrame {
        BattleFrame {
            target_markers: self.drawn_target_markers.clone(),
            stun_markers: self.stun_marker_frames(),
            update: self.update,
            hud_update: self.hud_update,
            hud_holds: self.hud_holds,
            targets: (0..self.actors.len())
                .map(|index| self.target(ActorId(index as u8)))
                .collect(),
            target_selector: self.target_selector,
            actors: self.actors.clone(),
            models: self
                .models
                .iter()
                .flatten()
                .map(|m| {
                    let mut frame = m.shown.clone();
                    frame.visible &= self.actors_visible;
                    self.death_appearance(frame.actor, &mut frame.visible, &mut frame.tint);
                    frame
                })
                .collect(),
            weapons: self
                .models
                .iter()
                .flatten()
                .flat_map(|model| model.weapon_frames())
                .map(|mut frame| {
                    frame.visible &= self.actors_visible;
                    self.death_appearance(frame.owner, &mut frame.visible, &mut frame.tint);
                    frame
                })
                .collect(),
            actions: self
                .sequences
                .iter()
                .filter(|(_, s)| s.effect.is_none())
                .map(|(&id, s)| (id, s.actor, s.age))
                .collect(),
            cues,
            recognized_result: self.terminal.result,
            trails: self
                .trails
                .iter()
                .enumerate()
                .flat_map(|(index, trails)| {
                    trails
                        .iter()
                        .filter_map(move |trail| trail.frame(ActorId(index as u8)))
                })
                .collect(),
            particles: self.particle_frames(),
            scenes: self.scene_frames(),
            stage_colors: self.stage_colors.models,
            camera: self.camera.as_ref().map(|camera| camera.pose),
            projectiles: self
                .projectiles
                .values()
                .filter(|p| p.initialized)
                .map(|p| p.frame.clone())
                .collect(),
            outcome,
        }
    }

    pub(crate) fn start(&mut self, request: ActionRequest, cues: &mut Vec<Cue>) -> Result<()> {
        let definition = self
            .prepared
            .actions
            .iter()
            .position(|a| a.id == request.action)
            .unwrap();
        let busy = if self.prepared.actions[definition].phase == ActionPhase::Resident {
            self.spell_active(request.actor, SpellSlot::Primary)
        } else {
            matches!(
                self.actor(request.actor)?.activity,
                crate::Activity::Hurt
                    | crate::Activity::Guarding
                    | crate::Activity::KnockedDown
                    | crate::Activity::GettingUp
                    | crate::Activity::Stunned
            ) || self
                .sequences
                .values()
                .any(|s| s.actor == request.actor && s.definition.phase.is_actor())
        };
        let actor = self.actor(request.actor)?;
        let reason = if self.terminal.result.is_some() {
            Some(Rejection::BattleEnding)
        } else if actor.availability == crate::ActorAvailability::Petrified {
            Some(Rejection::Petrified)
        } else if !actor.available() {
            Some(Rejection::Defeated)
        } else if busy || self.transition_owner().is_some() {
            Some(Rejection::Busy)
        } else if actor.tp < self.prepared.actions[definition].tp_cost {
            Some(Rejection::InsufficientTp)
        } else {
            None
        };
        if let Some(reason) = reason {
            cues.push(Cue::Rejected {
                actor: request.actor,
                reason,
            });
            return Ok(());
        }
        let (id, sequence) = self.allocate_sequence(definition, request.actor, request.target)?;
        if self.prepared.actions[definition].phase.is_actor() {
            self.actors[request.actor.index()].hud.cast_released = false;
            self.melee[request.actor.index()].clear();
            self.actors[request.actor.index()].activity =
                if self.prepared.actions[definition].phase == ActionPhase::Casting {
                    crate::Activity::Casting {
                        clock: 0,
                        guard_window: [0, 0],
                    }
                } else {
                    crate::Activity::Action {
                        clock: 0,
                        guard_window: [0, 0],
                    }
                };
        }
        if let Some(&color) = self.prepared.admission_flashes.get(&request.action) {
            // 1E12C/26980 runs on admission, before the same actor's common
            // timer decrement. The next model visit observes the held flash.
            self.contact_feedback[request.actor.index()].flash(color);
        }
        self.sequences.insert(id, sequence);
        cues.push(Cue::Started {
            action: id,
            actor: request.actor,
        });
        Ok(())
    }

    pub(crate) fn allocate_sequence(
        &mut self,
        definition: usize,
        actor: ActorId,
        target: ActorId,
    ) -> Result<(ActionId, Sequence)> {
        let id = ActionId(self.next_action);
        let next = self
            .next_action
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("battle action handle exhausted"))?;
        let sequence = Sequence::new(&self.prepared.actions[definition], actor, target)?;
        self.next_action = next;
        Ok((id, sequence))
    }

    pub(crate) fn spell_active(&self, actor: ActorId, slot: SpellSlot) -> bool {
        self.sequences
            .values()
            .any(|s| s.actor == actor && s.resident.as_ref().is_some_and(|r| r.slot == slot))
    }

    pub(crate) fn release(
        &mut self,
        spell: u16,
        actor: ActorId,
        target: ActorId,
        slot: SpellSlot,
        parent: ActionId,
        cues: &mut Vec<Cue>,
    ) -> Result<Option<ActionId>> {
        if self.spell_active(actor, slot) {
            return Ok(None);
        }
        let definition = self
            .prepared
            .actions
            .iter()
            .position(|a| a.id == spell)
            .unwrap();
        let (id, mut sequence) = self.allocate_sequence(definition, actor, target)?;
        sequence.resident.as_mut().unwrap().slot = slot;
        self.sequences.insert(id, sequence);
        cues.push(Cue::Released {
            action: id,
            parent,
            actor,
            slot,
        });
        Ok(Some(id))
    }

    pub(crate) fn actor(&self, id: ActorId) -> Result<&Actor> {
        self.actors
            .get(id.index())
            .ok_or_else(|| anyhow::anyhow!("invalid battle actor handle"))
    }

    pub(crate) fn emit(
        &mut self,
        definition: Arc<ProjectileDefinition>,
        action: ActionId,
        owner: ActorId,
        target: ActorId,
        position: [f32; 3],
    ) -> Result<()> {
        self.emit_with_velocity(definition, action, owner, target, (position, None))
    }

    pub(crate) fn expire_projectile(&mut self, id: ProjectileId, cues: &mut Vec<Cue>) {
        // 418B4 registers followed particles in the projectile child list;
        // 14748 removes those children before the projectile itself.
        self.particles.retain(|particle_id, particle| {
            if matches!(particle.follow, Some(crate::effect::Follow::Projectile(parent)) if parent == id) {
                cues.push(Cue::ParticleExpired { particle: *particle_id });
                false
            } else { true }
        });
        self.projectiles.remove(&id);
        cues.push(Cue::ProjectileExpired { projectile: id });
    }

    pub(crate) fn emit_with_velocity(
        &mut self,
        definition: Arc<ProjectileDefinition>,
        action: ActionId,
        owner: ActorId,
        target: ActorId,
        (position, velocity): ([f32; 3], Option<[f32; 3]>),
    ) -> Result<()> {
        ensure!(
            position
                .iter()
                .chain(velocity.iter().flatten())
                .all(|v| v.is_finite()),
            "invalid projectile emission transform"
        );
        if !self.object_available() {
            return Ok(());
        }
        let id = ProjectileId(self.next_projectile);
        self.next_projectile = self
            .next_projectile
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("battle projectile handle exhausted"))?;
        let frame = ProjectileFrame {
            id,
            owner,
            target,
            position,
            heading: self.actor(owner)?.heading,
            age: 0,
            contact_active: false,
            disarmed: false,
            shadow: None,
        };
        let mut projectile = Projectile::new(definition, action, frame);
        projectile.attack_power = self.actor(owner)?.attack_power;
        projectile.target_point = self.actor(target)?.body.center;
        if let Some(velocity) = velocity {
            projectile.set_velocity(velocity);
        }
        self.projectiles.insert(id, projectile);
        Ok(())
    }

    /// fn_1_1DE04 + fn_1_1F814: boost the percent before multiplying, retain
    /// signed 16-bit narrowing and report the nominal amount even at the HP cap.
    pub(crate) fn recover(&mut self, id: ActorId, percent: i16, cues: &mut Vec<Cue>) -> Result<()> {
        let actor = self.actor(id)?;
        if actor.hp <= 0 {
            return Ok(());
        }
        self.recover_vitals(id, percent, true, cues)
    }

    pub(crate) fn recover_vitals(
        &mut self,
        id: ActorId,
        percent: i16,
        lucky: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let actor = self.actor(id)?;
        let (lucky, luck) = (lucky && actor.recovery.lucky, actor.luck);
        let mut value = percent;
        if lucky && self.random.next() % 100 < u16::from(luck) / 20 + 5 {
            value = value.wrapping_add(value >> 1);
        }
        let actor = &mut self.actors[id.index()];
        if actor.recovery.boost {
            value = value.wrapping_add((i32::from(value) * 20 / 100) as i16);
        }
        let mut nominal = (actor.max_hp.wrapping_mul(i32::from(value)) / 100) as i16;
        if nominal == 0 {
            nominal = 1;
        }
        let cap = if actor.recovery.weak {
            actor.max_hp >> 1
        } else {
            actor.max_hp
        };
        let before = actor.hp;
        if !actor.recovery.weak || actor.hp < cap {
            actor.hp = actor.hp.wrapping_add(i32::from(nominal)).min(cap);
        }
        cues.push(Cue::Recovered {
            actor: id,
            nominal,
            applied: actor.hp.wrapping_sub(before),
        });
        Ok(())
    }
}

#[cfg(test)]
mod melee_tests;

#[cfg(test)]
mod spell_tests;
