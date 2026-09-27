//! World scene ownership: travel, landmark services and the shared event VM.
use super::{
    Rules, collision,
    landmarks::{Contact, Locations, Progress},
    travel::{self, Mount, Travel},
};
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::{Guidepost, Landmarks, MovementParameters};
use resonance_events::{EventRuntime, PersistentState, ResourceLibrary, SavedProgress};
use std::sync::Arc;
use symphonia_script::{Program, Width};

/// Prepared immutable dependencies. Construction performs no runtime file reads.
#[derive(Clone)]
pub struct Assets {
    pub world: super::World,
    pub terrain: Arc<collision::Terrain>,
    pub rules: Arc<Rules>,
    pub movement: Arc<MovementParameters>,
    pub landmarks: Arc<Landmarks>,
    pub guideposts: Arc<Vec<Guidepost>>,
    pub program: Arc<Program>,
    pub story_rules: Arc<super::scripts::Rules>,
    pub resources: Arc<ResourceLibrary>,
    pub skits: Arc<std::collections::BTreeMap<u16, crate::skit::Prepared>>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Input {
    pub travel: travel::Input,
    pub menu: crate::field::FieldInput,
    pub confirm: bool,
    pub cancel: bool,
    pub skit: bool,
    pub skip_skit: bool,
    pub accelerate_dialogue: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    ChangeWorld {
        destination: super::World,
    },
    Enter {
        location: u16,
        name: String,
        direction: u8,
    },
    Item {
        item: u16,
        received: bool,
    },
    Guidepost {
        name: String,
    },
}
/// A dedicated story battle request, to be resolved by the combat owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialEncounter {
    Sandworm,
}

struct PendingBattle {
    request: resonance_events::battle::Request,
    special: Option<SpecialEncounter>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub state: travel::State,
    pub progress: SavedProgress,
    pub played_ticks: u64,
}

pub struct Session {
    pub events: EventRuntime,
    pub travel: Travel,
    pub camera: super::camera::Camera,
    pub enemies: super::enemies::Symbols,
    pub locations: Locations,
    pub play_time: crate::clock::PlayTime,
    pub active_skit: Option<crate::skit::Playback>,
    pub menu: Option<crate::menu::Menu>,
    skits: crate::field::skit::Skits,
    pub(super) assets: Arc<Assets>,
    pub cinematic: Option<super::cinematic::Playback>,
    prompt: Option<Prompt>,
    discovery: Option<(u16, u32)>,
    battle: Option<PendingBattle>,
    world_destination: Option<super::World>,
    portal_contact: bool,
}
impl Session {
    pub fn enter(
        assets: Arc<Assets>,
        state: travel::State,
        mut persistent: PersistentState,
        play_time: crate::clock::PlayTime,
    ) -> Result<Self> {
        assets.story_rules.on_enter(state.world, &mut persistent)?;
        Self::construct(assets, state, persistent, play_time, true)
    }
    pub(super) fn construct(
        assets: Arc<Assets>,
        mut state: travel::State,
        persistent: PersistentState,
        play_time: crate::clock::PlayTime,
        resolve_floor: bool,
    ) -> Result<Self> {
        ensure!(state.world == assets.world, "wrong prepared world terrain");
        let data = assets
            .resources
            .session_data
            .as_ref()
            .context("world session definitions are missing")?;
        persistent
            .party
            .as_ref()
            .context("world party is missing")?
            .validate(data)?;
        // Field exits specify a horizontal landmark position. Resolve its floor
        // before exposing the first frame or allowing a contact prompt to pause
        // movement; otherwise the player can remain buried at the supplied 0.
        if resolve_floor {
            let floor = assets
                .terrain
                .query(state.position, assets.movement.collision_radius)?
                .motion(
                    0.,
                    state.heading,
                    if state.mount == Mount::Ship {
                        collision::Mode::Ship
                    } else {
                        collision::Mode::Ground
                    },
                )?;
            state.position = state.position.translated([0., 0., floor.delta[2]])?;
            if !state.mount.airborne() {
                state.altitude = state.position.map()[2];
            }
        }
        let travel = Travel::new(state, assets.movement.clone())?;
        let locations = Locations::new(
            assets.landmarks.clone(),
            assets.guideposts.clone(),
            assets.story_rules.clone(),
        )?;
        let (mut world, memory) = persistent.into_world();
        world.current_field = Some(3000);
        world.input_enabled = true;
        world.external_encounter_clock = true;
        // Entry camera commands configure the next field while world presentation
        // observes Travel's camera. No field actor needs to exist on the world.
        world.field_camera = Some(Default::default());
        let events = EventRuntime::with_state(
            assets.program.clone(),
            assets.resources.clone(),
            world,
            memory,
        )?;
        let mut session = Self {
            events,
            travel,
            camera: Default::default(),
            enemies: Default::default(),
            locations,
            skits: crate::field::skit::Skits::new(assets.resources.skits.clone()),
            active_skit: None,
            cinematic: None,
            menu: None,
            assets,
            play_time,
            prompt: None,
            discovery: None,
            battle: None,
            world_destination: None,
            portal_contact: false,
        };
        session.refresh_locations()?;
        session.travel_audio(session.travel.state().mount)?;
        Ok(session)
    }
    pub fn restore(assets: Arc<Assets>, checkpoint: Checkpoint) -> Result<Self> {
        let data = assets
            .resources
            .session_data
            .as_ref()
            .context("world session definitions are missing")?;
        ensure!(
            checkpoint.progress.party.travel.overworld.as_ref() == Some(&checkpoint.state),
            "inconsistent world checkpoint"
        );
        Self::construct(
            assets.clone(),
            checkpoint.state,
            checkpoint.progress.into_state(data)?,
            crate::clock::PlayTime::resume(checkpoint.played_ticks),
            false,
        )
    }
    /// The field owner keeps its live VM until this prepared return succeeds.
    pub fn return_from_field(
        assets: Arc<Assets>,
        request: &resonance_events::WorldTransition,
        mut persistent: PersistentState,
        play_time: crate::clock::PlayTime,
    ) -> Result<Self> {
        let mut locations = Locations::new(
            assets.landmarks.clone(),
            assets.guideposts.clone(),
            assets.story_rules.clone(),
        )?;
        let party = persistent
            .party
            .as_ref()
            .context("world return party is missing")?;
        locations.refresh(&Progress::new(
            &persistent.memory,
            party,
            &persistent.event_flags,
            &persistent.script_state,
        ))?;
        let mut state = super::entry::resolve(
            &locations,
            &assets.movement,
            party.travel.overworld.as_ref(),
            request.location,
            request.direction,
        )?;
        state.position = locations.push_out(state.world, state.position, state.mount)?;
        // Script global 4 selects the base world/music bank. Preserve the late
        // Tethe'alla bank when returning from a field.
        let bank = if state.world == super::World::Sylvarant {
            0
        } else if resonance_events::script_global(&persistent.memory, 4)? == 2 {
            2
        } else {
            1
        };
        persistent.memory.write(0x50, Width::S32, bank)?;
        Self::enter(assets, state, persistent, play_time)
    }
    pub fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }
    /// Keep the discovered chest visible while its one-shot opening is shown.
    pub fn discovery(&self) -> Option<(u16, u32)> {
        self.discovery
    }
    /// Advance only the view, also used by stationary renderer observations.
    pub fn update_camera(&mut self) -> Result<()> {
        self.camera.step(&self.travel, &self.assets.terrain)
    }
    pub fn special_encounter(&self) -> Option<SpecialEncounter> {
        self.battle.as_ref().and_then(|battle| battle.special)
    }
    pub fn battle_request(&self) -> Option<&resonance_events::battle::Request> {
        self.battle.as_ref().map(|battle| &battle.request)
    }
    /// The contacting symbol selects the region/formation variant while the
    /// player's floor selects terrain. Flying and sailing suppress encounters.
    pub fn encounter_symbol(&mut self, position: super::Position, variant: u8) -> Result<bool> {
        ensure!(variant < 2, "invalid world enemy variant");
        if !self.player_has_control()
            || !matches!(self.travel.state().mount, Mount::Foot | Mount::Noishe)
        {
            return Ok(false);
        }
        let Some(encounter) = self.assets.rules.lookup(
            self.travel.state().world,
            position,
            self.travel.response(),
            variant,
            self.events.memory().read(0x40, Width::S32)?,
        ) else {
            return Ok(false);
        };
        self.begin_battle(
            resonance_events::battle::Encounter::Pool(encounter.group),
            u16::from(encounter.arena),
            None,
        )?;
        Ok(true)
    }
    fn begin_battle(
        &mut self,
        encounter: resonance_events::battle::Encounter,
        arena: u16,
        special: Option<SpecialEncounter>,
    ) -> Result<()> {
        ensure!(self.battle.is_none(), "nested world encounter");
        let state = self.travel.checkpoint()?;
        let request = self
            .events
            .world
            .request_battle(resonance_events::battle::Setup {
                encounter,
                arena,
                defeat: resonance_events::battle::DefeatPolicy::GameOver,
                music: None,
                route: [0; 5],
            })
            .map_err(anyhow::Error::msg)?;
        self.events.world.party.as_mut().unwrap().travel.overworld = Some(state);
        self.battle = Some(PendingBattle { request, special });
        Ok(())
    }
    fn poll_battle(&mut self) -> Result<()> {
        let battle = self.battle.as_ref().context("missing world encounter")?;
        if battle
            .request
            .result()
            .map_err(anyhow::Error::msg)?
            .is_none()
        {
            return Ok(());
        }
        if self
            .events
            .world
            .battle_request
            .as_ref()
            .is_some_and(|request| request.id() == battle.request.id())
        {
            self.events.world.battle_request = None;
        }
        self.battle = None;
        self.enemies = Default::default();
        self.refresh_locations()?;
        self.travel_audio(self.travel.state().mount)
    }
    pub fn world_destination(&self) -> Option<super::World> {
        self.world_destination
    }
    /// Prepare a portal arrival without consuming the current scene. The owner
    /// retires it only after the destination's terrain and presentation succeed.
    pub fn change_world(&self, assets: Arc<Assets>) -> Result<Self> {
        let destination = self.world_destination.context("no world portal request")?;
        ensure!(
            assets.world == destination,
            "wrong portal destination terrain"
        );
        let id = if destination == super::World::Sylvarant {
            15
        } else {
            271
        };
        let portal = self
            .locations
            .definition(id)
            .context("missing world portal")?;
        let mut state = self.travel.checkpoint()?;
        state.world = destination;
        state.position = super::Position::from_map([portal.position[0], portal.position[1], 0.])?;
        state.heading = 0.;
        state.camera_yaw = 0.;
        state.altitude = assets.movement.flight_clearance;
        let mut persistent = self.events.persistent_state()?;
        persistent
            .memory
            .write(0x50, Width::S32, destination.index() as i32)?;
        persistent
            .party
            .as_mut()
            .context("world party is missing")?
            .travel
            .overworld = Some(state.clone());
        let mut session = Self::enter(assets, state, persistent, self.play_time)?;
        session.portal_contact = true;
        Ok(session)
    }
    pub fn player_has_control(&self) -> bool {
        self.cinematic.is_none()
            && self.menu.is_none()
            && self.prompt.is_none()
            && self.active_skit.is_none()
            && self.battle.is_none()
            && self.world_destination.is_none()
            && self.events.world.skit_request.is_none()
            && self.events.world.field_transition.is_none()
            && self.events.world.world_transition.is_none()
            && self.events.player_has_control()
            && self.travel.player_has_control()
    }
    pub fn checkpoint(&self) -> Result<Checkpoint> {
        ensure!(
            self.player_has_control(),
            "cannot save while a world service owns control"
        );
        let state = self.travel.checkpoint()?;
        let mut progress = self.events.save_progress()?;
        progress.party.travel.overworld = Some(state.clone());
        Ok(Checkpoint {
            state,
            progress,
            played_ticks: self.play_time.total(),
        })
    }
    /// The shared menu edits a progress snapshot. Its field-shaped view model
    /// is never persisted as a field save: world saves retain their typed pose.
    pub fn menu_checkpoint(&self) -> Result<Checkpoint> {
        let snapshot = self
            .menu
            .as_ref()
            .and_then(|menu| menu.checkpoint.as_ref())
            .context("world menu is closed")?;
        let state = self.travel.checkpoint()?;
        ensure!(
            snapshot.progress.party.travel.overworld.as_ref() == Some(&state),
            "world menu lost its travel pose"
        );
        Ok(Checkpoint {
            state,
            progress: snapshot.progress.clone(),
            played_ticks: self.play_time.total(),
        })
    }

    fn open_menu(&mut self) -> Result<()> {
        let checkpoint = self.checkpoint()?;
        let resources = Arc::new(crate::menu::Resources {
            session: self
                .assets
                .resources
                .session_data
                .clone()
                .context("world menu party data missing")?,
            data: self
                .assets
                .resources
                .menu_data
                .clone()
                .context("world menu data missing")?,
        });
        let mut menu = crate::menu::Menu::new(
            crate::menu::Page::Main,
            Some(checkpoint.menu_snapshot()),
            true,
        );
        menu.resources = Some(resources);
        menu.world_map.world = checkpoint.state.world.index() as u8;
        menu.set_play_time(self.play_time);
        menu.begin_opening();
        self.menu = Some(menu);
        self.events.world.input_enabled = false;
        self.events
            .world
            .audio_commands
            .push(resonance_events::AudioCommand::StopSound(15));
        self.menu_sound(Some(
            resonance_content::field_audio::ServiceCue::MenuOpen as i16,
        ))
    }

    fn menu_sound(&mut self, cue: Option<i16>) -> Result<()> {
        if let Some(id) = cue {
            ensure!(
                resonance_content::field_audio::ServiceCue::from_id(id).is_some(),
                "world menu emitted undeclared service cue {id}"
            );
            self.events
                .world
                .audio_commands
                .push(resonance_events::AudioCommand::Sound {
                    id,
                    volume: 127,
                    pan: 64,
                    slot: None,
                });
        }
        Ok(())
    }
    /// Destination preparation receives a copy. It can fail without retiring the
    /// source VM, consuming its operation, or losing the saved world position.
    pub fn field_entry(&self) -> Result<crate::field::FieldEntry> {
        let request = self
            .events
            .world
            .field_transition
            .as_ref()
            .context("world has no field destination")?;
        let mut persistent = self.events.persistent_state()?;
        if self.cinematic.is_none() {
            persistent
                .party
                .as_mut()
                .context("world party is missing")?
                .travel
                .overworld = Some(self.travel.checkpoint()?);
        }
        Ok(crate::field::FieldEntry {
            persistent,
            play_time: self.play_time,
            data: self.assets.resources.session_data.clone(),
            menu_data: self.assets.resources.menu_data.clone(),
            skits: self.assets.resources.skits.clone(),
            text: self.assets.resources.text.clone(),
            available_fields: self.assets.resources.fields.clone(),
            position: request.position,
            heading: request.heading,
            // The overworld VM has no field actor (its controlled ID is zero).
            // Rebind the next-field follow camera to that field's party leader.
            camera: request.camera.clone().map(|mut entry| {
                if entry.camera.actor == 0 {
                    entry.camera.actor =
                        i32::from(self.events.world.party.as_ref().unwrap().field_leader);
                }
                entry
            }),
            ..Default::default()
        })
    }
    pub fn step(&mut self, input: Input) -> Result<Vec<travel::Cue>> {
        input.travel.validate()?;
        if self.battle.is_some() {
            self.poll_battle()?;
            return Ok(Vec::new());
        }
        if self.events.world.field_transition.is_some()
            || self.events.world.world_transition.is_some()
            || self.world_destination.is_some()
        {
            return Ok(Vec::new());
        }
        if self.cinematic.is_some() {
            self.step_cinematic()?;
            return Ok(Vec::new());
        }
        if let Some(menu) = &mut self.menu {
            self.play_time.advance();
            menu.set_play_time(self.play_time);
            let cue = menu.step(input.menu);
            if let Some((party, random)) = menu.take_party_changes() {
                self.events.world.party = Some(party);
                self.events.world.gameplay_random = random;
            }
            if menu.closed {
                self.menu = None;
                self.events.world.input_enabled = true;
                self.travel_audio(self.travel.state().mount)?;
            }
            self.menu_sound(cue)?;
            return Ok(Vec::new());
        }
        if input.menu.menu && self.player_has_control() {
            self.open_menu()?;
            return Ok(Vec::new());
        }
        if let Some(skit) = &mut self.active_skit {
            self.play_time.advance();
            if skit.step(
                &mut self.events,
                crate::skit::Input {
                    confirm: input.confirm,
                    cancel: input.cancel,
                    direction: if input.travel.stick[1] > 0.5 {
                        -1
                    } else if input.travel.stick[1] < -0.5 {
                        1
                    } else {
                        0
                    },
                    accelerate: input.accelerate_dialogue,
                    skip: input.skip_skit,
                },
            )? {
                self.active_skit = None;
                self.skits.reset();
                self.refresh_locations()?;
            }
            return Ok(Vec::new());
        }
        if let Some(request) = self.events.world.skit_request.clone() {
            self.start_skit(
                request.id,
                request.skippable,
                request.preview,
                Some(request.operation),
            )?;
            self.events.world.skit_request = None;
            self.play_time.advance();
            return Ok(Vec::new());
        }
        self.skits
            .step(&self.events, 3000, self.player_has_control())?;
        if input.skit
            && self.player_has_control()
            && let Some(id) = self.skits.open()
        {
            self.start_skit(id, true, false, None)?;
            self.play_time.advance();
            return Ok(Vec::new());
        }
        self.refresh_locations()?;
        let mut cues = Vec::new();
        if let Some(prompt) = self.prompt.clone() {
            self.travel.stop();
            if input.confirm || input.cancel {
                self.prompt = None;
                self.discovery = None;
                if input.confirm {
                    match prompt {
                        Prompt::Enter {
                            location,
                            direction,
                            ..
                        } => self.start_event(Contact {
                            id: location,
                            direction,
                            blocked: false,
                        })?,
                        Prompt::ChangeWorld { destination } => {
                            self.world_destination = Some(destination)
                        }
                        _ => {}
                    }
                }
            }
        } else if self.events.player_has_control() {
            let state = self.travel.state();
            let previous = state.position;
            // Landing uses the ordinary ground landmark check before requesting
            // descent. Long-range mode still suppresses field-point discoveries.
            let landing = if input.travel.vehicle
                && state.mount == Mount::Rheairds
                && self.travel.player_has_control()
            {
                self.locations
                    .contact(state.world, state.position, Mount::Noishe, state.altitude)
            } else {
                None
            };
            if let Some(contact) = landing {
                self.contact(contact, false)?;
            } else {
                let party = self
                    .events
                    .world
                    .party
                    .as_ref()
                    .context("world party is missing")?;
                let context = travel::Context {
                    event_flags: &self.events.world.event_flags,
                    rheairds_owned: party.items.get(&58).copied().unwrap_or(0) > 0,
                    landing_clear: true,
                };
                cues = self.travel.step(
                    input.travel,
                    &context,
                    &self.assets.terrain,
                    &self.assets.rules,
                )?;
                if self.travel.player_has_control() {
                    let state = self.travel.state();
                    if let Some(contact) = self.locations.contact(
                        state.world,
                        state.position,
                        state.mount,
                        state.altitude,
                    ) {
                        let airborne = state.mount.airborne();
                        self.travel.reject_displacement(previous);
                        self.contact(contact, airborne)?;
                    }
                    if self.player_has_control()
                        && let Some((position, variant)) =
                            self.enemies.contact(self.travel.state().position)
                    {
                        self.travel.reject_displacement(previous);
                        self.encounter_symbol(position, variant)?;
                    }
                }
            }
        }
        self.step_enemies()?;
        self.portal_prompt()?;
        self.update_camera()?;
        for cue in &cues {
            match *cue {
                travel::Cue::Started {
                    to: Mount::Rheairds | Mount::Ship,
                    ..
                } => self.travel_audio(self.travel.state().mount)?,
                travel::Cue::Finished(Mount::Foot) => self.travel_audio(Mount::Foot)?,
                travel::Cue::Denied => {
                    self.events
                        .world
                        .audio_commands
                        .push(resonance_events::AudioCommand::Sound {
                            id: resonance_content::field_audio::ServiceCue::Error as i16,
                            pan: 64,
                            volume: 127,
                            slot: None,
                        })
                }
                _ => {}
            }
        }
        if matches!(self.travel.displayed_mount(), Mount::Ship | Mount::Rheairds) {
            self.events
                .world
                .audio_commands
                .push(resonance_events::AudioCommand::SoundVolume {
                    slot: 15,
                    volume: 64 + (63. * self.travel.speed_fraction()) as u8,
                });
        }
        self.play_time.advance();
        if self.battle.is_none() {
            self.events.step()?;
        }
        Ok(cues)
    }
    fn step_enemies(&mut self) -> Result<()> {
        let state = self.travel.state();
        if !matches!(state.mount, Mount::Foot | Mount::Noishe) {
            self.enemies.clear();
            return Ok(());
        }
        if !self.player_has_control() {
            return Ok(());
        }
        let modifier = self
            .events
            .world
            .party
            .as_mut()
            .unwrap()
            .encounter_modifier
            .as_mut();
        let rate = modifier.as_ref().map_or(0, |m| m.rate);
        self.enemies.step(
            super::enemies::Context {
                world: state.world,
                position: state.position,
                mount: state.mount,
                speed: self.travel.speed(),
                modifier: rate,
                terrain: &self.assets.terrain,
                locations: &self.locations,
                parameters: &self.assets.movement,
            },
            &mut self.events.world.random_state,
        )?;
        if self.travel.speed() != 0.
            && let Some(modifier) = modifier
        {
            modifier.remaining = modifier.remaining.saturating_sub(1);
            if modifier.remaining == 0 {
                self.events.world.party.as_mut().unwrap().encounter_modifier = None;
            }
        }
        if let Some((position, variant)) = self.enemies.contact(self.travel.state().position) {
            self.encounter_symbol(position, variant)?;
        }
        Ok(())
    }
    fn portal_prompt(&mut self) -> Result<()> {
        let state = self.travel.state();
        if state.mount != Mount::Rheairds {
            return Ok(());
        }
        let id = if state.world == super::World::Sylvarant {
            15
        } else {
            271
        };
        if self
            .locations
            .appearance(id)
            .is_none_or(|a| a.marker == resonance_content::overworld::Marker::None)
        {
            return Ok(());
        }
        let portal = self
            .locations
            .definition(id)
            .context("missing world portal")?;
        let target = super::Position::from_map([
            portal.position[0],
            portal.position[1],
            state.position.map()[2],
        ])?;
        let delta = state.position.displacement_to(target);
        let distance = delta[0].hypot(delta[1]);
        // Gate contact latches at 250 and rearms beyond 300, preventing a
        // dismissed prompt or an arrival from immediately opening it again.
        if distance > 300. {
            self.portal_contact = false;
        }
        if distance <= 250. && !self.portal_contact && self.player_has_control() {
            self.portal_contact = true;
            self.prompt = Some(Prompt::ChangeWorld {
                destination: if state.world == super::World::Sylvarant {
                    super::World::TetheAlla
                } else {
                    super::World::Sylvarant
                },
            });
        }
        Ok(())
    }
    fn travel_audio(&mut self, mount: Mount) -> Result<()> {
        use resonance_events::AudioCommand;
        let bank = self.events.memory().read(0x50, Width::S32)?;
        let music = self.assets.story_rules.music(mount, bank)?;
        self.events.world.audio_commands.push(AudioCommand::Music(
            resonance_events::MusicCommand::Play(music),
        ));
        self.events
            .world
            .audio_commands
            .push(AudioCommand::StopSound(15));
        let sound = match mount {
            Mount::Ship => Some(25),
            Mount::Rheairds => Some(24),
            _ => None,
        };
        if let Some(id) = sound {
            self.events
                .world
                .audio_commands
                .push(AudioCommand::RepeatSound {
                    id,
                    pan: 64,
                    volume: 64,
                    slot: 15,
                });
        }
        Ok(())
    }

    pub fn skit_prompt(&self) -> Option<crate::field::SkitPrompt<'_>> {
        let mut prompt = self.skits.prompt()?;
        prompt.title_visible = self
            .events
            .world
            .party
            .as_ref()
            .is_none_or(|p| p.settings.preferences.skit_notifications);
        Some(prompt)
    }
    fn start_skit(
        &mut self,
        id: u16,
        skippable: bool,
        preview: bool,
        completion: Option<resonance_events::Operation>,
    ) -> Result<()> {
        let prepared = self
            .assets
            .skits
            .get(&id)
            .with_context(|| format!("world skit {id} was not prepared"))?;
        self.active_skit = Some(crate::skit::Playback::start(
            prepared,
            &mut self.events,
            skippable,
            preview,
            completion,
        )?);
        Ok(())
    }
    fn refresh_locations(&mut self) -> Result<()> {
        let party = self
            .events
            .world
            .party
            .as_ref()
            .context("world party is missing")?;
        self.locations.refresh(&Progress::new(
            self.events.memory(),
            party,
            &self.events.world.event_flags,
            &self.events.world.script_state,
        ))?;
        Ok(())
    }
    fn contact(&mut self, contact: Contact, airborne: bool) -> Result<()> {
        self.travel.stop();
        if !airborne {
            if let Some(item) = self.locations.reward(contact.id) {
                let party = self
                    .events
                    .world
                    .party
                    .as_mut()
                    .context("world party is missing")?;
                let data = self
                    .assets
                    .resources
                    .session_data
                    .as_ref()
                    .context("world session definitions are missing")?;
                let received = party
                    .change_item(data, item, 1)
                    .map_err(anyhow::Error::msg)?;
                if received {
                    party.travel.visited_locations.insert(contact.id);
                    self.discovery = Some((contact.id, self.events.tick()));
                }
                self.prompt = Some(Prompt::Item { item, received });
                self.refresh_locations()?;
                return Ok(());
            }
            if contact.id == 94 {
                // Sandworm uses encounter group 0x60 and desert arena 5.
                self.begin_battle(
                    resonance_events::battle::Encounter::Formation(96),
                    5,
                    Some(SpecialEncounter::Sandworm),
                )?;
                self.events
                    .world
                    .party
                    .as_mut()
                    .context("world party is missing")?
                    .travel
                    .visited_locations
                    .insert(94);
                return Ok(());
            }
            if let Some(post) = self.locations.guidepost(contact.id)
                && !self
                    .events
                    .world
                    .event_flags
                    .contains(&post.event_flags[0].unwrap().get())
            {
                self.events
                    .world
                    .event_flags
                    .extend(post.event_flags.iter().flatten().map(|v| v.get()));
                self.prompt = Some(Prompt::Guidepost {
                    name: post.name.clone(),
                });
                self.refresh_locations()?;
                return Ok(());
            }
            if contact.blocked {
                return Ok(());
            }
        }
        let definition = self
            .locations
            .definition(contact.id)
            .context("unknown contacted landmark")?;
        if definition.automatic {
            self.start_event(contact)?;
        } else if !airborne {
            self.prompt = Some(Prompt::Enter {
                location: contact.id,
                name: definition.name.clone(),
                direction: contact.direction,
            });
        }
        Ok(())
    }
    fn start_event(&mut self, contact: Contact) -> Result<()> {
        ensure!(
            self.events.enter_landmark(contact.id, contact.direction)?,
            "world landmark event is unavailable"
        );
        if self.assets.story_rules.records_visit(contact.id)? {
            self.events
                .world
                .party
                .as_mut()
                .context("world party is missing")?
                .travel
                .visited_locations
                .insert(contact.id);
        }
        Ok(())
    }
}

impl Checkpoint {
    /// Shared menu/slot display data; only the typed world checkpoint is loaded.
    pub fn menu_snapshot(&self) -> crate::field::FieldCheckpoint {
        crate::field::FieldCheckpoint {
            allow_incomplete_scripts: false,
            map_id: 3000,
            position: self.state.position.plane(),
            heading: self.state.heading.to_degrees(),
            camera: None,
            progress: self.progress.clone(),
            played_ticks: Some(self.played_ticks),
        }
    }
}

#[cfg(test)]
mod tests;
