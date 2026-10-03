//! Scene setup supplies the original script with cooked resource bindings.
use anyhow::{Context, Result, ensure};
use resonance_content::field::SCENERY_RESOURCE_BASE;
use resonance_content::field::{CollisionQuery, FieldAssets};
use resonance_events::{
    ACTOR_CONTACT_HEIGHT, Actor, AnimationClip, EventRuntime, ModelResource, ResourceKind,
    ResourceLibrary,
};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::Program;
pub mod attachments;
mod blocks;
mod checkpoint;
mod conditions;
pub mod crafting;
pub mod navigation;
mod prompt;
pub mod replay;
mod ring;
mod save_point;
pub mod shop;
pub(crate) mod skit;
mod treasure;
pub use checkpoint::FieldCheckpoint;
pub use prompt::{ActionPrompt, FieldAction};
pub use skit::Playback as SkitPlayback;
pub use skit::SkitPrompt;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    #[default]
    Arrival,
    /// Rebuild the saved scene before resolving its final camera view.
    Restore,
}

/// Typed field-entry data; a transition carries state, never old scene handles.
#[derive(Default)]
pub struct FieldEntry {
    /// The disposable overworld test may fall back to walking after unsupported scripts.
    pub allow_incomplete_scripts: bool,
    pub kind: EntryKind,
    pub services: Option<Arc<crate::authored::FieldServices>>,
    pub attachments: attachments::Attachments,
    pub play_time: crate::clock::PlayTime,
    pub persistent: resonance_events::PersistentState,
    pub data: Option<Arc<resonance_content::session::SessionData>>,
    pub menu_data: Option<Arc<resonance_content::menu_data::MenuData>>,
    pub skits: Option<Arc<resonance_content::skit::SkitCatalog>>,
    pub text: Arc<resonance_content::session::GameText>,
    pub available_fields: std::collections::BTreeSet<u32>,
    /// Movie IDs from this field's verified preparation inventory.
    pub available_movies: std::collections::BTreeSet<u32>,
    pub position: [f32; 3],
    pub heading: f32,
    pub idle_animation: Option<u16>,
    pub camera: Option<resonance_events::camera::EntryCamera>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FieldInput {
    /// Physical held buttons; action edges below also support replayed input.
    pub held_buttons: resonance_events::input::Buttons,
    /// Camera-relative stick input: right and forward, in [-1, 1].
    pub direction: [f32; 2],
    pub run: bool,
    pub interact: bool,
    /// Held accept accelerates dialogue without repeating interaction/advance edges.
    pub accelerate_dialogue: bool,
    /// Open the currently announced skit (GameCube Z / keyboard Z).
    pub skit: bool,
    pub cancel: bool,
    pub menu: bool,
    pub start: bool,
    pub alternate: bool,
    pub previous_page: bool,
    pub next_page: bool,
    /// Held page-scroll direction: up +1, down -1 (right stick / Page Up/Down).
    pub scroll_direction: i8,
    /// Held model-viewer controls: rotation and zoom (right stick).
    pub preview_direction: [f32; 2],
}

/// Owns one field's gameplay and event lifetime. Presentation consumes the
/// resulting actors and operations without needing to understand bytecode.
pub struct FieldSession {
    pub allow_incomplete_scripts: bool,
    entered_control: bool,
    entry_wait_ticks: u32,
    pub play_time: crate::clock::PlayTime,
    /// Effect births keep their phase through menus; particle age uses field time.
    pub effect_clock: crate::clock::PresentationClock,
    pub map_id: u32,
    pub events: EventRuntime,
    authored_entry: Option<Arc<crate::authored::PreparedEvent>>,
    pub menu: Option<crate::menu::Menu>,
    pub shop: Option<shop::Shop>,
    pub crafting: Option<crafting::Crafting>,
    menu_operation: Option<resonance_events::Operation>,
    menu_resources: Option<Arc<crate::menu::Resources>>,
    pub dialogue: BTreeMap<u8, crate::dialogue::DialoguePlayer>,
    choices: crate::choice::ChoicePlayer,
    walkmesh: navigation::WalkMesh,
    player_fall: navigation::PlayerFall,
    light_regions: Option<navigation::WalkMesh>,
    conversation_facing: Option<(i32, f32, f32)>,
    save_points: save_point::SavePoints,
    treasures: treasure::Treasures,
    ring: ring::Ring,
    blocks: blocks::Blocks,
    action_hints: prompt::ActionHints,
    skits: skit::Skits,
    pub active_skit: Option<SkitPlayback>,
    skit_programs: BTreeMap<u16, skit::Prepared>,
    pub voice_durations: Arc<BTreeMap<u32, u32>>,
    pub voice_feedback: Option<Arc<dyn crate::dialogue::VoiceFeedback>>,
    /// Actor -> first update of its current continuous dialogue mouth cycle.
    pub talking: BTreeMap<i32, u32>,
}
impl FieldSession {
    pub fn dialogue_scene(
        &self,
    ) -> (
        &resonance_events::GameWorld,
        &BTreeMap<u8, crate::dialogue::DialoguePlayer>,
    ) {
        self.active_skit
            .as_ref()
            .map_or((&self.events.world, &self.dialogue), |s| {
                (&s.events.world, &s.dialogue)
            })
    }
    /// Whether field input belongs to the player. Transient UI notifications
    /// do not take control and are intentionally excluded from this query.
    pub fn player_has_control(&self) -> bool {
        self.authored_entry.is_none() && self.field_control_available()
    }
    pub fn menu_is_open(&self) -> bool {
        self.menu.is_some() || self.shop.is_some() || self.crafting.is_some()
    }
    fn field_control_available(&self) -> bool {
        !self.menu_is_open()
            && self.events.world.menu_request.is_none()
            && self.active_skit.is_none()
            && self.events.world.skit_request.is_none()
            && self.events.player_has_control()
    }
    pub fn skit_prompt(&self) -> Option<SkitPrompt<'_>> {
        if !self.player_has_control() {
            return None;
        }
        let mut prompt = self.skits.prompt()?;
        prompt.title_visible = self
            .events
            .world
            .party
            .as_ref()
            .is_none_or(|party| party.settings.preferences.skit_notifications);
        Some(prompt)
    }

    /// Preserve ambient clocks across field changes, dismissing the skit title.
    /// Quickloads restart ambient services through ordinary field initialization.
    pub fn continue_ambient(&mut self, previous: &Self) {
        self.skits = previous.skits.next_field();
        self.effect_clock = previous.effect_clock;
    }
    pub fn apply_skit_origin(
        &mut self,
        id: u16,
        control_ticks: u32,
        remaining: u16,
        opacity: u8,
        text_opacity: u8,
    ) -> Result<()> {
        self.skits
            .apply_origin(id, control_ticks, remaining, opacity, text_opacity)
    }
    pub fn action_prompt(&self) -> Option<ActionPrompt> {
        self.action_hints.prompt.filter(|_| !self.menu_is_open())
    }

    pub fn story_progress(&self) -> Result<i32> {
        Ok(self
            .events
            .memory()
            .read(0x40, symphonia_script::Width::S32)?)
    }
    pub fn new(
        script: &[u8],
        messages: Vec<symphonia_script::message::Message>,
        assets: &FieldAssets,
    ) -> Result<Self> {
        Self::enter(script, messages, assets, FieldEntry::default())
    }
    pub fn enter(
        script: &[u8],
        messages: Vec<symphonia_script::message::Message>,
        assets: &FieldAssets,
        entry: FieldEntry,
    ) -> Result<Self> {
        use sha2::{Digest, Sha256};
        ensure!(
            format!("{:x}", Sha256::digest(script)) == assets.script.sha256,
            "field script digest mismatch"
        );
        let skits = skit::Skits::new(entry.skits.clone());
        let menu_resources = entry
            .data
            .clone()
            .zip(entry.menu_data.clone())
            .map(|(session, data)| Arc::new(crate::menu::Resources { session, data }));
        if let Some(resources) = &menu_resources {
            resources.data.validate()?;
            ensure!(
                resources
                    .session
                    .characters
                    .iter()
                    .flat_map(|c| &c.allowed_techniques)
                    .all(|id| usize::from(*id) < resources.data.techniques.len()),
                "character references an uncooked technique"
            );
        }
        let services = entry.services.clone();
        let mut session = Self {
            allow_incomplete_scripts: entry.allow_incomplete_scripts,
            entered_control: false,
            entry_wait_ticks: 0,
            menu_resources,
            skits,
            active_skit: None,
            skit_programs: BTreeMap::new(),
            play_time: entry.play_time,
            effect_clock: crate::clock::PresentationClock::new(entry.persistent.tick),
            map_id: assets.map_id,
            events: start_with_entry(script, messages, assets, entry)?,
            authored_entry: None,
            menu: None,
            shop: None,
            crafting: None,
            menu_operation: None,
            dialogue: BTreeMap::new(),
            choices: Default::default(),
            conversation_facing: None,
            save_points: save_point::SavePoints::new(services.clone()),
            treasures: treasure::Treasures {
                event: services.as_ref().map(|s| s.treasure.clone()),
            },
            ring: ring::Ring {
                event: services.as_ref().map(|s| s.ring.clone()),
                ..Default::default()
            },
            blocks: Default::default(),
            action_hints: Default::default(),
            voice_durations: Default::default(),
            voice_feedback: None,
            talking: Default::default(),
            walkmesh: navigation::WalkMesh::new(&assets.ground)?,
            player_fall: Default::default(),
            light_regions: (!assets.regions.is_empty())
                .then(|| navigation::WalkMesh::new(&assets.regions))
                .transpose()?,
        };
        if let Some(reason) = session.events.exploration_error.clone() {
            session.enter_exploration(reason)?;
        }
        Ok(session)
    }
    /// Install a prepared entry only after legacy setup/checkpoint restoration.
    /// The presentation owner starts gameplay updates after the field is ready.
    pub fn queue_authored_entry(&mut self, event: Option<Arc<crate::authored::PreparedEvent>>) {
        self.authored_entry = event;
    }
    pub fn step(&mut self, input: FieldInput) -> Result<()> {
        if self.allow_incomplete_scripts {
            if input.start
                && !self.menu_is_open()
                && self
                    .events
                    .world
                    .party
                    .as_ref()
                    .is_some_and(|p| p.travel.overworld.is_some())
            {
                self.events.cancel();
                self.events
                    .world
                    .request_world(0, 0, None)
                    .map_err(anyhow::Error::msg)?;
                return Ok(());
            }
            self.events
                .world
                .skip_battle_as_victory()
                .map_err(anyhow::Error::msg)?;
        }
        let result = self.step_inner(input);
        if !self.allow_incomplete_scripts {
            return result;
        }
        if let Err(error) = result {
            self.enter_exploration(format!("{error:#}"))?;
        }
        if !self.entered_control {
            self.entry_wait_ticks += 1;
            if self.entry_wait_ticks >= 1200
                && self.events.world.dialogue.is_empty()
                && self.events.world.choices.is_empty()
                && self.events.world.field_transition.is_none()
                && self.events.world.world_transition.is_none()
            {
                self.enter_exploration("Entrance event did not release walking controls".into())?;
            }
            self.entered_control = self.player_has_control();
        }
        Ok(())
    }
    pub fn enter_exploration(&mut self, reason: String) -> Result<()> {
        self.events.enter_exploration(reason);
        self.authored_entry = None;
        self.active_skit = None;
        self.menu = None;
        self.shop = None;
        self.crafting = None;
        self.menu_operation = None;
        self.dialogue.clear();
        self.choices = Default::default();
        self.talking.clear();
        self.action_hints = Default::default();
        self.save_points = Default::default();
        self.treasures = Default::default();
        for point in std::mem::take(&mut self.events.world.save_points) {
            self.events.world.actors.remove(&point.actor);
        }
        self.events.world.treasures.clear();
        self.conversation_facing = None;
        let id = self.events.world.controlled_actor;
        if !self.events.world.actors.contains_key(&id) {
            let mut player = Actor::new(id as u32, [0.; 3]);
            if let Some(model) = self.events.resources().model(id as u32) {
                player
                    .appearance
                    .hidden_nodes
                    .clone_from(&model.hidden_nodes);
            }
            self.events.world.insert_actor(id, player);
        }
        let floor = self.walkmesh.with_actors(self.events.world.actors.values());
        let actor = self.events.world.actors.get_mut(&id).unwrap();
        actor.position = floor
            .exploration_start(actor.position)
            .context("field has no exploration floor")?;
        actor.visible = true;
        actor.grounded = true;
        actor.appearance.model_hidden = false;
        actor.appearance.fixed_heading = None;
        actor.properties.insert(8, 255);
        for property in [7, 35, 36] {
            actor.properties.remove(&property);
        }
        for property in [30, 31, 32] {
            actor.properties.insert(property, 100);
        }
        actor.scripted_animation = false;
        actor.animation = None;
        actor.autonomy = Some(resonance_events::Autonomy::new(
            resonance_events::Behavior::Player,
            0.,
            actor.position,
        ));
        let entry = resonance_events::camera::EntryCamera::following(id);
        let mut camera = resonance_events::camera::CameraRig::default();
        camera.angles = entry.camera.angles;
        camera.distance = entry.camera.distance;
        *camera.current_mut() = entry.camera;
        camera.snap_follow_view(&self.events.world.actors);
        self.events.world.field_camera = Some(camera);
        self.entered_control = true;
        Ok(())
    }
    fn step_inner(&mut self, input: FieldInput) -> Result<()> {
        use resonance_events::input::Button;
        let pressed = [
            (Button::Accept, input.interact),
            (Button::Cancel, input.cancel),
            (Button::Skit, input.skit),
            (Button::Menu, input.menu),
            (Button::Start, input.start),
            (Button::Ring, input.alternate),
            (Button::PreviousPage, input.previous_page),
            (Button::NextPage, input.next_page),
        ]
        .into_iter()
        .filter_map(|(button, pressed)| pressed.then_some(button))
        .collect();
        self.events.world.input.sample(input.held_buttons, pressed);
        self.play_time.advance();
        self.effect_clock.advance();
        if self.field_control_available()
            && let Some(event) = self.authored_entry.take()
        {
            event.start(&mut self.events)?;
        }
        if self.player_has_control() && self.events.world.party.is_some() {
            self.events.restore_field_leader()?;
        }
        if self.active_skit.is_some() {
            return self.step_skit(input);
        }
        if let Some(request) = self.events.world.skit_request.take() {
            self.start_skit(
                request.id,
                request.skippable,
                request.preview,
                Some(request.operation),
            )?;
            return Ok(());
        }
        if let Some(request) = self.events.world.menu_request.take() {
            ensure!(!self.menu_is_open(), "nested field menu");
            match request.target {
                resonance_events::menu::Target::Shop(id) => {
                    self.shop = Some(shop::Shop::open(
                        id,
                        self.menu_resources
                            .clone()
                            .context("shop resources are missing")?,
                        self.events
                            .world
                            .party
                            .as_mut()
                            .context("shop party is missing")?,
                        request.operation,
                    )?);
                }
                resonance_events::menu::Target::Crafting(id) => {
                    self.crafting = Some(crafting::Crafting::open(
                        id,
                        self.menu_resources
                            .clone()
                            .context("crafting resources are missing")?,
                        request.operation,
                    )?);
                }
                resonance_events::menu::Target::Main => {
                    self.open_menu(crate::menu::Page::Main, self.menu_checkpoint()?, false);
                    self.menu_operation = Some(request.operation);
                    return Ok(());
                }
            }
        }
        if let Some(menu) = &mut self.menu {
            menu.set_play_time(self.play_time);
            let cue = menu.step(input);
            if let Some((party, gameplay_random)) = menu.take_party_changes() {
                self.events.world.party = Some(party);
                self.events.world.gameplay_random = gameplay_random;
            }
            if let Some(shop) = &mut self.shop
                && (menu.closed || menu.page == crate::menu::Page::Main)
            {
                self.menu = None;
                shop.return_from_equipment();
            } else if menu.closed {
                self.menu = None;
                self.events.restore_field_leader()?;
                if let Some(operation) = self.menu_operation.take() {
                    operation.complete(Some(0)).map_err(anyhow::Error::msg)?;
                } else {
                    self.events.world.input_enabled = true;
                }
            }
            self.menu_sound(cue)?;
            return Ok(());
        }
        if let Some(shop) = &mut self.shop {
            let cue = shop.step(
                input,
                self.events
                    .world
                    .party
                    .as_mut()
                    .context("shop party is missing")?,
            )?;
            if shop.closed {
                self.shop = None;
            } else if shop.take_equipment_request() {
                self.open_menu(crate::menu::Page::Equip, self.menu_checkpoint()?, false);
            }
            self.menu_sound(cue.map(|cue| cue as i16))?;
            return Ok(());
        }
        if let Some(crafting) = &mut self.crafting {
            let cue = crafting.step(
                input,
                self.events
                    .world
                    .party
                    .as_mut()
                    .context("crafting party is missing")?,
            )?;
            if crafting.closed {
                self.crafting = None;
            }
            self.menu_sound(cue.map(|cue| cue as i16))?;
            return Ok(());
        }
        let at_circle = self.events.world.save_points.iter().any(|p| p.active);
        if (input.menu || input.interact && at_circle)
            && let Ok(checkpoint) = self.checkpoint()
        {
            let page = if input.menu {
                crate::menu::Page::Main
            } else {
                crate::menu::Page::Slots(crate::menu::Mode::Save)
            };
            self.open_menu(page, checkpoint, at_circle);
            self.events.world.input_enabled = false;
            self.menu_sound(Some(
                resonance_content::field_audio::ServiceCue::MenuOpen as i16,
            ))?;
            return Ok(());
        }
        if input.skit && self.player_has_control() {
            let skit_id = self.skits.prompt().map(|prompt| prompt.id);
            if let Some(id) = skit_id {
                self.start_skit(id, true, false, None)?;
                self.skits.open();
                return Ok(());
            }
        }
        let talking = self.step_dialogue(input)?;
        self.save_points
            .interact(&mut self.events, input.interact && !talking)?;
        self.treasures
            .step(&mut self.events, input.interact && !talking)?;
        if !talking {
            self.ring.step(&mut self.events, input.alternate)?;
        }
        self.blocks.settle(&mut self.events.world, &self.walkmesh);
        self.blocks.step(
            &mut self.events,
            &self.walkmesh,
            FieldInput {
                interact: input.interact && !talking,
                ..input
            },
        );
        if self.events.player_has_control() && !talking {
            let world = &self.events.world;
            let contact = world
                .actors
                .get(&world.controlled_actor)
                .and_then(|player| {
                    world.actors.iter().find_map(|(&id, actor)| {
                        (actor.visible
                            && actor.enemy.as_ref().is_some_and(|enemy| {
                                enemy.contact_cooldown == 0 && enemy.stun.is_none()
                            })
                            && actor
                                .position
                                .iter()
                                .zip(player.position)
                                .map(|(a, b)| (a - b).powi(2))
                                .sum::<f32>()
                                < (2. * actor.radius + player.radius).powi(2))
                        .then_some(id)
                    })
                });
            if let Some(actor) = contact {
                self.events.contact_enemy(actor)?;
            }
        }
        let can_trigger = self.events.player_has_control() && !talking;
        let interaction_target = input.interact.then(|| self.interaction_target()).flatten();
        let walkmesh = self.walkmesh.with_actors(self.events.world.actors.values());
        let controlled_actor = self.events.world.controlled_actor;
        let obstacles: Vec<_> = self
            .events
            .world
            .actors
            .iter()
            .filter(|(_, a)| {
                a.visible && a.collidable && a.contact == resonance_events::ActorContact::Cylinder
            })
            .map(|(&id, a)| (id, a.position, a.radius))
            .collect();
        let conversation_facing = &mut self.conversation_facing;
        let mut action = None;
        let attempted_contact = std::cell::Cell::new(None);
        let mut resolved = BTreeMap::new();
        let falling = matches!(self.player_fall, navigation::PlayerFall::Falling { .. });
        let player_fall = &mut self.player_fall;
        self.events.step_with_motion(
            self.effect_clock.tick(),
            |events| {
                let mut player_destination = None;
                if can_trigger {
                    let id = events.world.controlled_actor;
                    if let Some(actor) = events.world.actors.get(&id) {
                        let start = actor.position;
                        let mut stick = input
                            .direction
                            .map(|v| if v.is_finite() { v.clamp(-1., 1.) } else { 0. });
                        let length = stick[0].hypot(stick[1]);
                        if length > 1. {
                            stick = stick.map(|v| v / length);
                        }
                        let forward = events.world.field_camera.as_ref().map_or([0., 1.], |c| {
                            [c.target[0] - c.position[0], c.target[1] - c.position[1]]
                        });
                        // Field controls rotate in whole degrees relative to the view.
                        let angle = (-forward[0].atan2(forward[1]).to_degrees())
                            .trunc()
                            .to_radians();
                        let forward = [-angle.sin(), angle.cos()];
                        let size = events.world.player_size;
                        let speed = if input.run { 8. } else { 4. } * size.movement_scale();
                        let delta = [
                            (forward[1] * stick[0] + forward[0] * stick[1]) * speed,
                            (-forward[0] * stick[0] + forward[1] * stick[1]) * speed,
                        ];
                        let target = if falling {
                            start
                        } else {
                            walkmesh.move_by(start, delta, size.floor_clearance(), |p| {
                                events.world.actors.iter().any(|(other, a)| {
                                    if *other == id {
                                        return false;
                                    }
                                    const MODEL_PROBE_HEIGHT: f32 = 45.;
                                    if a.contains_solid(
                                        [p[0], p[1], p[2] + MODEL_PROBE_HEIGHT],
                                        CollisionQuery::Player,
                                    ) {
                                        return true;
                                    }
                                    let blocked = a.visible
                                        && a.collidable
                                        && a.contact == resonance_events::ActorContact::Cylinder
                                        && actor.contact == resonance_events::ActorContact::Cylinder
                                        // fn_80024284 adds the two authored
                                        // cylinder radii, with a 150-unit
                                        // vertical overlap tolerance. Local
                                        // models participate just like global ones.
                                        && (p[2] - a.position[2]).abs() <= ACTOR_CONTACT_HEIGHT
                                        && (p[0] - a.position[0]).hypot(p[1] - a.position[1])
                                            < actor.radius + a.radius;
                                    if blocked
                                        && a.contact_event
                                        && attempted_contact.get().is_none()
                                    {
                                        attempted_contact.set(Some(*other));
                                    }
                                    blocked
                                })
                            })
                        };
                        // Derive facing before adding world coordinates: subtracting
                        // rounded positions can push a whole-degree angle across its boundary.
                        let heading = (delta != [0.; 2]).then(|| movement_heading(delta));
                        player_destination = Some((id, target, heading));
                        let actor = events.world.actors.get_mut(&id).unwrap();
                        actor.set_movement_speed(speed * length.min(1.));
                        // Facing and locomotion follow input, even when a wall blocks
                        // part of the step. Apply collision after advancing that intent;
                        // a short wall slide is not a completed scripted move.
                        actor.motion = if delta[0] != 0. || delta[1] != 0. {
                            Some(resonance_events::ActorMotion {
                                target: [start[0] + delta[0], start[1] + delta[1], start[2]],
                                speed: speed * length.min(1.),
                            })
                        } else {
                            None
                        };
                        if input.interact
                            && let Some(target) = interaction_target
                            && events.interact(target)?
                        {
                            // Ordinary field conversations turn the selected person
                            // toward Lloyd while leaving the player's facing alone.
                            // Independent NPC 304/305 oracle checkpoints verify this.
                            let other = events.world.actors.get_mut(&target).unwrap();
                            if let Some(autonomy) = &mut other.autonomy {
                                autonomy.begin_conversation();
                            }
                            let previous_heading = other.target_heading;
                            other.target_heading = (start[0] - other.position[0])
                                .atan2(other.position[1] - start[1])
                                .to_degrees()
                                .rem_euclid(360.)
                                .trunc();
                            *conversation_facing =
                                Some((target, previous_heading, other.target_heading));
                        }
                    }
                }
                Ok((
                    player_destination,
                    !events.world.input_enabled,
                    events.world.mapped_input_disabled,
                ))
            },
            |(player_destination, scripted_control, event_paused), update, id, actor, previous| {
                let (scripted_control, event_paused) = match update {
                    resonance_events::MotionUpdate::Frame => (*scripted_control, *event_paused),
                    resonance_events::MotionUpdate::AnimationBinding {
                        event_paused,
                        input_enabled,
                    } => (!input_enabled, event_paused),
                };
                if update == resonance_events::MotionUpdate::Frame
                    && let Some((player, target, heading)) = *player_destination
                    && id == player
                {
                    actor.position = target;
                    if let Some(heading) = heading {
                        actor.target_heading = heading;
                    }
                }
                if actor.grounded && actor.resource < SCENERY_RESOURCE_BASE && actor.resource != 24
                {
                    if id != controlled_actor
                        && !event_paused
                        && actor.collidable
                        && actor.contact == resonance_events::ActorContact::Cylinder
                    {
                        // fn_80024284 uses the same authored radii and height
                        // for NPC and player contact. A smaller NPC boundary
                        // can put the player inside an inescapable overlap.
                        // fn_8001A6FC bypasses contact during mapped-input pause.
                        for &(other, position, radius) in &obstacles {
                            let position = resolved.get(&other).copied().unwrap_or(position);
                            if other == id
                                || (actor.position[2] - position[2]).abs() > ACTOR_CONTACT_HEIGHT
                            {
                                continue;
                            }
                            let mut candidate = previous;
                            for axis in 0..2 {
                                candidate[axis] = actor.position[axis];
                                if (candidate[0] - position[0]).hypot(candidate[1] - position[1])
                                    < actor.radius + radius
                                {
                                    actor.position[axis] = previous[axis];
                                    candidate[axis] = previous[axis];
                                }
                            }
                        }
                    }
                    // fn_8001D5F4 resolves the player during scripted arrivals too.
                    // Only the mapped-input pause suspends an unsupported fall.
                    let position = if id == controlled_actor {
                        walkmesh.resolve_player(previous, actor.position, player_fall, event_paused)
                    } else if actor.enemy.is_some() {
                        walkmesh.resolve_enemy(previous, actor.position)
                    } else {
                        walkmesh.resolve_motion(previous, actor.position, false)
                    };
                    if let Some(autonomy) = &mut actor.autonomy {
                        autonomy.resolve_floor(position.is_some());
                    }
                    actor.position = position.unwrap_or_else(|| {
                        if id == controlled_actor && (scripted_control || event_paused) {
                            // Authored approaches can leave the floor at a doorway.
                            // Keep their horizontal motion and hold the last height.
                            [actor.position[0], actor.position[1], previous[2]]
                        } else {
                            // fn_8001A6FC cancels the movement command (B0=-1)
                            // when a grounded actor reaches an unsupported
                            // destination. Retaining it would leave scripts
                            // waiting forever for an unreachable endpoint.
                            actor.motion = None;
                            previous
                        }
                    });
                }
                resolved.insert(id, actor.position);
            },
            |events| {
                self.walkmesh.update_enemy_sight(&mut events.world);
                conditions::step(&mut events.world, self.effect_clock.tick())?;
                self.save_points
                    .step_effects(events, self.effect_clock.tick())?;
                if can_trigger && events.player_has_control() {
                    // Contacts queue their script after movement and before this
                    // update's VM dispatch, including confirmed door entries.
                    if let Some(player) = events.world.actors.get(&events.world.controlled_actor) {
                        let contact = events.world.actors.iter().find_map(|(&id, actor)| {
                            (id != events.world.controlled_actor
                                && actor.contact_event
                                && actor.contact == resonance_events::ActorContact::Cylinder
                                && player.contact == resonance_events::ActorContact::Cylinder
                                && (player.position[2] - actor.position[2]).abs()
                                    <= ACTOR_CONTACT_HEIGHT
                                && (player.position[0] - actor.position[0])
                                    .hypot(player.position[1] - actor.position[1])
                                    < player.radius + actor.radius)
                                .then_some(id)
                        });
                        // Native cylinder contact is observed before cancelling
                        // penetration, so a solid callback actor still fires.
                        if let Some(actor) = contact.or_else(|| attempted_contact.get()) {
                            events.contact_actor(actor)?;
                        }
                    }
                    action = Self::step_triggers(events, input.interact)?;
                }
                Ok(())
            },
        )?;
        let action = if can_trigger && self.events.player_has_control() {
            action.or(self.interaction_action()?)
        } else {
            None
        };
        let free_control = !talking && self.events.player_has_control();
        self.save_points.step(&mut self.events, free_control)?;
        let world = &self.events.world;
        self.action_hints.step(
            self.blocks
                .active()
                .then_some(FieldAction::Move)
                .or(action)
                .or_else(|| {
                    world
                        .save_points
                        .iter()
                        .any(|p| p.active)
                        .then_some(FieldAction::Save)
                }),
            (self.blocks.active() || free_control && world.input_enabled)
                && world.field_transition.is_none()
                && !world.blocked_by_movie()
                && world
                    .fade
                    .as_ref()
                    .is_none_or(|f| world.tick >= f.start_tick.saturating_add(f.duration)),
        );
        self.skits.step(&self.events, self.map_id, free_control)?;
        if self.events.world.input_enabled
            && let Some((id, heading, automatic_heading)) = self.conversation_facing.take()
            && let Some(actor) = self.events.world.actors.get_mut(&id)
            && actor.target_heading == automatic_heading
        {
            // A script may select its own return heading (Colette uses 0x14).
            // Restore the previous target only if the script left ours alone.
            actor.target_heading = heading;
        }
        let walkmesh = self.walkmesh.with_actors(self.events.world.actors.values());
        for (id, actor) in &mut self.events.world.actors {
            if actor.grounded
                && actor.resource < SCENERY_RESOURCE_BASE
                && actor.resource != 24
                // A script can move an actor after its movement update. New
                // actors retain their spawn height until their first update;
                // cloth and hair must see that initial pose before grounding.
                && resolved.contains_key(id)
                && resolved.get(id) != Some(&actor.position)
                // Script coordinates use coarse authored heights. A teleported
                // grounded actor must reach its actual floor before the next
                // small walking step (Nova places Lloyd 36 units above it).
                && let Some(z) = walkmesh.height(actor.position, 128.)
            {
                actor.position[2] = z;
            }
        }
        let targets: Vec<_> = self
            .events
            .world
            .actors
            .iter()
            .map(|(&id, actor)| (id, self.actor_light(actor)))
            .collect();
        for (id, target) in targets {
            let light = &mut self.events.world.actors.get_mut(&id).unwrap().light;
            match light {
                Some(light) => light.approach(&target),
                None => *light = Some(target),
            }
        }
        Ok(())
    }

    fn open_menu(&mut self, page: crate::menu::Page, checkpoint: FieldCheckpoint, at_circle: bool) {
        let mut menu = crate::menu::Menu::new(page, Some(checkpoint), at_circle);
        menu.begin_opening();
        menu.resources = self.menu_resources.clone();
        menu.set_play_time(self.play_time);
        self.menu = Some(menu);
    }

    fn menu_sound(&mut self, cue: Option<i16>) -> Result<()> {
        if let Some(id) = cue {
            ensure!(
                resonance_content::field_audio::ServiceCue::from_id(id).is_some(),
                "native menu emitted undeclared service cue {id}"
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
    fn step_triggers(events: &mut EventRuntime, confirm: bool) -> Result<Option<FieldAction>> {
        let world = &mut events.world;
        let Some(actor) = world.actors.get(&world.controlled_actor) else {
            return Ok(None);
        };
        let radius = actor.radius;
        let angle = actor.target_heading.to_radians();
        let ahead = [
            actor.position[0] + angle.sin() * radius,
            actor.position[1] - angle.cos() * radius,
            actor.position[2],
        ];
        // Touch events use body contact. Confirmed interactions also reach one
        // player radius forward, so a door can be used before walking into it.
        let touching: Vec<_> = world
            .triggers
            .iter_mut()
            .enumerate()
            .filter_map(|(index, trigger)| {
                let touches = navigation::touches_trigger(trigger, actor.position, radius)
                    || trigger.transition.is_some()
                        && navigation::touches_trigger(trigger, ahead, radius);
                if !touches || (trigger.transition.is_some() && confirm) {
                    trigger.activations = 0;
                }
                touches.then_some(index)
            })
            .collect();
        let action = touching
            .iter()
            .map(|&index| {
                let trigger = &world.triggers[index];
                trigger.transition.unwrap_or(trigger.touch_metadata)[0]
            })
            .find(|id| *id != 0)
            .map(FieldAction::from_id)
            .transpose()?
            .flatten();
        for index in touching {
            if events.world.triggers[index].transition.is_none() || confirm {
                events.contact_trigger(index)?;
            }
        }
        Ok(action)
    }
    fn step_dialogue(&mut self, input: FieldInput) -> Result<bool> {
        self.dialogue.retain(|slot, player| {
            self.events
                .world
                .dialogue
                .get(slot)
                .is_some_and(|d| d.operation.id() == player.operation.id())
        });
        for (&slot, request) in &self.events.world.dialogue {
            if let Some(player) = self.dialogue.get_mut(&slot) {
                player.sync_flags(request);
            }
            if request.opening_actor.is_none()
                && request.operation.is_pending()
                && !self.dialogue.contains_key(&slot)
            {
                self.dialogue.insert(
                    slot,
                    crate::dialogue::DialoguePlayer::new(
                        request,
                        self.events
                            .world
                            .party
                            .as_ref()
                            .map_or(3, |p| u16::from(p.settings.preferences.message_speed)),
                    )?
                    .with_voice_durations(self.voice_durations.clone())
                    .with_voice_feedback(self.voice_feedback.clone()),
                );
            }
        }
        let choice_slot = self
            .events
            .world
            .choices
            .iter()
            .find(|(_, choice)| choice.operation.is_pending())
            .map(|(&slot, _)| slot);
        let focus = choice_slot.or_else(|| {
            self.dialogue
                .iter()
                .find(|(_, d)| !d.closed && !d.persistent && d.operation.is_pending())
                .map(|(slot, _)| *slot)
        });
        for (slot, player) in &mut self.dialogue {
            for voice in player.step(
                (input.interact || input.cancel) && choice_slot.is_none() && focus == Some(*slot),
                input.accelerate_dialogue && focus == Some(*slot),
            )? {
                self.events.world.audio_commands.push(match voice {
                    crate::dialogue::VoiceAction::Play(id) => {
                        self.events.world.voice = Some(resonance_events::VoicePlayback {
                            resource: id,
                            end_tick: self.events.world.tick.saturating_add(
                                self.voice_durations.get(&id).copied().unwrap_or(0),
                            ),
                        });
                        resonance_events::AudioCommand::Voice(id)
                    }
                    crate::dialogue::VoiceAction::Stop => {
                        self.events.world.voice = None;
                        resonance_events::AudioCommand::StopVoice
                    }
                });
            }
        }
        let speakers: std::collections::BTreeSet<_> = self
            .dialogue
            .iter()
            .filter(|(_, p)| p.is_talking())
            .filter_map(|(slot, p)| {
                self.events
                    .world
                    .dialogue
                    .get(slot)
                    .filter(|d| d.operation.id() == p.operation.id())
                    .and_then(|d| d.speaker_actor)
            })
            .collect();
        self.talking.retain(|actor, _| speakers.contains(actor));
        for actor in speakers {
            self.talking.entry(actor).or_insert(self.events.tick());
        }
        if let Some(slot) = choice_slot {
            use resonance_content::field_audio::ServiceCue;
            use resonance_events::dialogue::ChoiceConfirmation;
            let player = self
                .dialogue
                .get_mut(&slot)
                .ok_or_else(|| anyhow::anyhow!("choice dialogue player is missing"))?;
            let choice = self.events.world.choices.get_mut(&slot).unwrap();
            let lines = 1 + player
                .current()
                .glyphs
                .iter()
                .filter(|g| g.character == '\n')
                .count();
            ensure!(
                usize::from(choice.last_line) < lines,
                "choice extends beyond dialogue lines"
            );
            let (reason, moved) = self.choices.step(
                choice,
                crate::choice::ChoiceInput {
                    direction: if input.direction[1] > 0.5 {
                        -1
                    } else if input.direction[1] < -0.5 {
                        1
                    } else {
                        0
                    },
                    confirm: input.interact
                        || choice.confirmation == ChoiceConfirmation::AcceptOrShoulder
                            && (input.previous_page || input.next_page),
                    cancel: input.cancel,
                },
                player.accepts_input() && player.fully_revealed(),
            );
            if moved {
                self.events
                    .world
                    .audio_commands
                    .push(resonance_events::AudioCommand::Sound {
                        id: ServiceCue::Navigate as i16,
                        pan: 64,
                        volume: 127,
                        slot: None,
                    });
            }
            if let Some(reason) = reason {
                choice.finish(reason).map_err(anyhow::Error::msg)?;
                player.close();
                self.events.world.voice = None;
                self.events
                    .world
                    .audio_commands
                    .push(resonance_events::AudioCommand::StopVoice);
                use resonance_events::dialogue::ChoiceExit;
                if reason != ChoiceExit::Timeout {
                    self.events
                        .world
                        .audio_commands
                        .push(resonance_events::AudioCommand::Sound {
                            id: if reason == ChoiceExit::Confirm {
                                ServiceCue::Confirm
                            } else {
                                ServiceCue::Cancel
                            } as i16,
                            pan: 64,
                            volume: 127,
                            slot: None,
                        });
                }
            }
        }
        Ok(focus.is_some())
    }
    pub fn interaction_target(&self) -> Option<i32> {
        let id = self.events.world.controlled_actor;
        let player = self.events.world.actors.get(&id)?;
        self.events
            .world
            .actors
            .iter()
            .filter_map(|(id, actor)| {
                if (!actor.visible && !actor.interaction_anchor)
                    || actor.properties.get(&50) == Some(&1)
                    || !self.events.has_interaction(*id)
                {
                    return None;
                }
                let dx = actor.position[0] - player.position[0];
                let dy = actor.position[1] - player.position[1];
                let distance = dx.hypot(dy);
                within_interaction_reach(player, actor).then_some((*id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
    pub fn ground_below(&self, point: [f32; 3]) -> Option<navigation::GroundSurface> {
        self.walkmesh
            .with_actors(self.events.world.actors.values())
            .surface_below(point)
    }
    pub fn ground_surface(&self, point: [f32; 3]) -> Option<navigation::GroundSurface> {
        self.walkmesh
            .with_actors(self.events.world.actors.values())
            .surface(point, 32.)
    }
    pub fn character_light(&self, id: i32) -> resonance_events::effect::CharacterLight {
        self.events
            .world
            .actors
            .get(&id)
            .map_or_else(Default::default, |actor| {
                actor
                    .light
                    .clone()
                    .unwrap_or_else(|| self.actor_light(actor))
            })
    }
    fn actor_light(
        &self,
        actor: &resonance_events::Actor,
    ) -> resonance_events::effect::CharacterLight {
        if actor.properties.get(&41) == Some(&1) {
            return self
                .events
                .world
                .character_lights
                .get(&-2)
                .cloned()
                .unwrap_or_default();
        }
        self.target_light(actor.position)
    }
    fn target_light(&self, point: [f32; 3]) -> resonance_events::effect::CharacterLight {
        // The ground attribute mesh selects the light using its low five bits.
        let selector = self
            .light_regions
            .as_ref()
            .and_then(|mesh| mesh.surface(point, f32::INFINITY))
            .map_or(0, |surface| (surface.attributes & 31) as i32);
        self.events
            .world
            .character_lights
            .get(&selector)
            .cloned()
            .unwrap_or_default()
    }
}

fn within_interaction_reach(player: &Actor, actor: &Actor) -> bool {
    const HEIGHT: f32 = 145.;
    let [dx, dy] = [
        actor.position[0] - player.position[0],
        actor.position[1] - player.position[1],
    ];
    let distance = dx.hypot(dy);
    let angle = player.heading.to_radians();
    let facing = dx * angle.sin() - dy * angle.cos();
    distance > 0.
        && distance < (1.4 * f64::from(player.radius + actor.radius)) as f32
        && (f64::from(-facing / distance).sin() as f32).to_degrees() < -22.5
        && (actor.position[2] - player.position[2]).abs() <= HEIGHT
}
fn movement_heading([x, y]: [f32; 2]) -> f32 {
    // Fuse the conversion: separate rounding can turn 0.9999976 degrees into
    // exactly 1, changing the whole-degree facing selected by actor movement.
    y.atan2(x).mul_add(1_f32.to_degrees(), 90.).rem_euclid(360.)
}

pub fn start(
    script: &[u8],
    messages: Vec<symphonia_script::message::Message>,
    assets: &FieldAssets,
) -> Result<EventRuntime> {
    start_with_entry(script, messages, assets, FieldEntry::default())
}

fn bind_clips(
    resources: &mut ResourceLibrary,
    owner: u32,
    clips: &[resonance_content::SceneClip],
) -> Result<()> {
    for clip in clips {
        let clips = match clip.animation_resource {
            Some(resource) => resources.animations.entry(resource).or_default(),
            None => {
                resources
                    .bindings
                    .insert(owner as i32, (ResourceKind::Model, owner));
                &mut resources.models.entry(owner).or_default().clips
            }
        };
        let previous = clips.insert(clip.resource_slot, AnimationClip::from(clip));
        ensure!(
            previous.is_none_or(|value| value.duration_ticks == clip.duration_ticks()),
            "animation {:?}/{} has inconsistent durations across models",
            clip.animation_resource,
            clip.resource_slot
        );
    }
    Ok(())
}

fn start_with_entry(
    script: &[u8],
    messages: Vec<symphonia_script::message::Message>,
    assets: &FieldAssets,
    mut entry: FieldEntry,
) -> Result<EventRuntime> {
    assets.validate()?;
    if let (Some(data), Some(menu)) = (&mut entry.data, &entry.menu_data) {
        if data.ex_skills.is_none() {
            Arc::make_mut(data).ex_skills = Some(Arc::new(menu.ex_skills.clone()));
        }
        if let Some(party) = &mut entry.persistent.party {
            party.bind_ex_skills(data);
        }
    }
    let mut resources = ResourceLibrary {
        station_script: entry
            .services
            .as_ref()
            .map(|s| s.station.module().program.clone()),
        memory_circle_text: resonance_events::MemoryCircleText {
            tutorial: assets.save_point_tutorial.clone(),
            unlock: assets.save_point_unlock.clone(),
            no_gem: assets.save_point_no_gem.clone(),
        },
        blink: Some(assets.blink.clone()),
        menu_data: entry.menu_data,
        text: entry.text,
        skits: entry.skits.clone(),
        doors: assets.doors.clone(),
        camera_tracks: assets.camera_tracks.clone(),
        particles: assets
            .particles
            .iter()
            .map(|(&kind, recipe)| {
                (
                    kind,
                    resonance_events::ParticleKind::Flutter(recipe.clone()),
                )
            })
            .collect(),
        messages,
        session_data: entry.data,
        fields: entry.available_fields,
        movies: entry.available_movies,
        actor_names: ResourceLibrary::character_names(),
        ..Default::default()
    };
    for &id in assets.camera_tracks.keys() {
        resources
            .bindings
            .insert(id as i32, (ResourceKind::Camera, id));
    }
    for geometry in &assets.unbound_geometry {
        resources.bindings.insert(
            geometry.resource as i32,
            (ResourceKind::UnboundGeometry, geometry.resource),
        );
    }
    for &resource in assets.overlays.keys() {
        resources
            .bindings
            .insert(resource, (ResourceKind::Overlay, resource as u32));
    }
    resources.locators.insert(24);
    for character in &assets.actors {
        let model = character
            .parts
            .first()
            .ok_or_else(|| anyhow::anyhow!("character has no model"))?;
        let resource = character.resource;
        resources
            .bindings
            .insert(resource as i32, (ResourceKind::Model, resource));
        resources.models.insert(
            resource,
            ModelResource {
                has_eyes: model.appearance.as_ref().is_some_and(|a| a.eyes.is_some()),
                toon_lighting: character
                    .parts
                    .iter()
                    .any(|part| part.outline_color.is_some()),
                collision: Arc::new(character.collision.clone()),
                names: model.bone_names.clone(),
                hidden_nodes: character.hidden_nodes.iter().copied().collect(),
                ..Default::default()
            },
        );
        bind_clips(&mut resources, resource, &model.clips)?;
    }
    for (model, poses) in entry.attachments {
        for &(source, resource, slot) in poses.clips.keys() {
            ensure!(
                resources
                    .clips(resource, source)
                    .is_some_and(|clips| clips.contains_key(&slot)),
                "prepared attachment clip is missing"
            );
        }
        resources
            .models
            .get_mut(&model)
            .context("prepared attachment model is missing")?
            .attachments = poses;
    }
    let (mut world, memory) = entry.persistent.into_world();
    world.current_field = Some(assets.map_id);
    if let Some((party, menu)) = world.party.as_mut().zip(resources.menu_data.as_ref()) {
        party.travel.enter_field(&menu.world_map, assets.map_id);
    }
    let leader = world.party.as_ref().map_or(1, |p| p.field_leader);
    world.controlled_actor = i32::from(leader);
    let entry_camera = entry.camera.or_else(|| {
        (entry.kind == EntryKind::Arrival)
            .then(|| resonance_events::camera::EntryCamera::following(world.controlled_actor))
    });
    let mut camera = resonance_events::camera::CameraRig::default();
    camera.current_mut().follow = true;
    camera.current_mut().anchor_to_actor = true;
    camera.current_mut().actor = world.controlled_actor;
    if let Some(entry) = entry_camera {
        camera.angles = entry.camera.angles;
        camera.distance = entry.camera.distance;
        camera.position_rate = entry.position_rate;
        camera.target_rate = entry.target_rate;
        *camera.current_mut() = entry.camera;
    }
    world.field_camera = Some(camera);
    let resource = u32::from(leader);
    let model = resources
        .models
        .get(&resource)
        .context("field leader is not cooked")?;
    let mut player = Actor::new(resource, entry.position);
    player.autonomy = Some(resonance_events::Autonomy::new(
        resonance_events::Behavior::Player,
        0.,
        [0.; 3],
    ));
    player.face(entry.heading);
    if let Some(slot) = entry.idle_animation {
        ensure!(
            model.clips.contains_key(&slot),
            "field entry idle animation is missing"
        );
        player.idle_animation = slot;
    }
    player
        .appearance
        .hidden_nodes
        .clone_from(&model.hidden_nodes);
    world.insert_actor(world.controlled_actor, player);
    world.insert_actor(
        resonance_events::camera::ANCHOR_ACTOR,
        resonance_events::camera::anchor(),
    );
    for part in &assets.parts {
        let resource = SCENERY_RESOURCE_BASE + u32::from(part.resource);
        // Reserved script actor IDs address the scenery layers directly.
        let actor = match part.resource {
            0 => 0xF423C,
            10 => 0xF423D,
            2 => 0xF423E,
            12 => 0xF422C,
            _ => -(resource as i32),
        };
        resources.models.insert(
            resource,
            ModelResource {
                names: part.bone_names.clone(),
                ..Default::default()
            },
        );
        bind_clips(&mut resources, resource, &part.clips)?;
        let mut instance = Actor::new(resource, [0.; 3]);
        instance.cull_outside_view = false;
        instance.grounded = false;
        instance.collidable = false;
        instance.casts_shadow = false;
        if part.autoplay {
            let clip = part
                .clips
                .first()
                .ok_or_else(|| anyhow::anyhow!("field autoplay clip is missing"))?;
            instance.scripted_animation = true;
            instance.animation = Some(resonance_events::Animation::new(
                resource,
                clip.resource_slot,
                clip.duration_ticks(),
                world.tick,
            ));
        }
        world.insert_actor(actor, instance);
    }
    if entry.kind == EntryKind::Arrival {
        // Resolve the entrance before scripts configure the live camera.
        // Saved scenes resolve their view after setup and player placement.
        world
            .field_camera
            .as_mut()
            .unwrap()
            .snap_follow_view(&world.actors);
    }
    EventRuntime::with_state_policy(
        Arc::new(Program::decode(script)?),
        Arc::new(resources),
        world,
        memory,
        entry.allow_incomplete_scripts,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn scenery_and_actors_share_external_clips_without_replacing_autoplay() {
        let mut resources = ResourceLibrary::default();
        let own = resonance_content::SceneClip {
            motion: "clips/test.motion".into(),
            resource_slot: 0,
            duration_seconds: 1.,
            animation_resource: None,
            secondary_pose_nodes: Vec::new(),
        };
        let mut shared = resonance_content::SceneClip {
            motion: "clips/test.motion".into(),
            resource_slot: 12,
            duration_seconds: 2.,
            animation_resource: Some(0x10004),
            secondary_pose_nodes: Vec::new(),
        };
        bind_clips(
            &mut resources,
            SCENERY_RESOURCE_BASE,
            &[own, shared.clone()],
        )
        .unwrap();
        bind_clips(&mut resources, 1, &[shared.clone()]).unwrap();
        assert_eq!(
            resources.models[&SCENERY_RESOURCE_BASE]
                .clips
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [0]
        );
        assert_eq!(
            resources.animations[&0x10004][&12].duration_ticks,
            shared.duration_ticks()
        );
        assert!(!resources.models.contains_key(&0x10004));
        let mut own = shared.clone();
        own.animation_resource = None;
        own.duration_seconds = 1.;
        shared.animation_resource = Some(8);
        bind_clips(&mut resources, 8, &[own, shared.clone()]).unwrap();
        assert_ne!(
            resources.models[&8].clips[&12].duration_ticks,
            resources.animations[&8][&12].duration_ticks
        );
        assert_eq!(resources.resolve(8, ResourceKind::Model).unwrap(), 8);
        shared.duration_seconds = 3.;
        assert!(bind_clips(&mut resources, 2, &[shared]).is_err());
    }

    use super::*;
    use symphonia_script::{
        NativeCall, Width,
        message::{Message, Token},
    };

    fn native(words: &mut Vec<u16>, op: NativeCall, arguments: &[i32]) {
        for &value in arguments {
            words.extend([
                0x0200,
                value as u16,
                (value as u32 >> 16) as u16,
                0x3000,
                0x4000,
            ]);
        }
        words.push(0x2000 | op as u16);
    }

    fn choice_session() -> FieldSession {
        choice_session_with_flags(0x100)
    }

    fn choice_session_with_flags(flags: i32) -> FieldSession {
        let mut code = vec![4, 0, 0, 0];
        native(
            &mut code,
            NativeCall::ConfigureDialogue,
            &[0, 0x20, -2, 1, 0, 0, 0, 1],
        );
        native(&mut code, NativeCall::YieldCommand, &[3, 0]);
        native(
            &mut code,
            NativeCall::ConfigureDialogue,
            &[1, 0, -2, 7, 0, 0, 0, 2],
        );
        native(&mut code, NativeCall::YieldCommand, &[3, 1]);
        native(&mut code, NativeCall::ShowChoice, &[1, 1, 2, 0, flags]);
        code.extend([0x3000, 0x1200, 0x100, 0x1200, 0x20, 0x3010, 0x3000]);
        native(&mut code, NativeCall::CloseDialogue, &[0]);
        native(&mut code, NativeCall::CloseDialogue, &[1]);
        native(&mut code, NativeCall::EnableMappedInput, &[]);
        code.push(0x20ff);
        let resources = ResourceLibrary {
            messages: ["", "Question?", "Yes\nNo"]
                .into_iter()
                .map(|text| Message {
                    tokens: vec![Token::Text { text: text.into() }],
                })
                .collect(),
            ..Default::default()
        };
        FieldSession {
            allow_incomplete_scripts: false,
            entered_control: false,
            entry_wait_ticks: 0,
            map_id: 0,
            authored_entry: None,
            menu_resources: None,
            play_time: Default::default(),
            effect_clock: Default::default(),
            menu: None,
            shop: None,
            crafting: None,
            menu_operation: None,
            conversation_facing: None,
            save_points: Default::default(),
            treasures: Default::default(),
            ring: Default::default(),
            blocks: Default::default(),
            action_hints: Default::default(),
            skits: Default::default(),
            active_skit: None,
            skit_programs: BTreeMap::new(),
            voice_durations: Default::default(),
            voice_feedback: None,
            talking: Default::default(),
            events: EventRuntime::new(
                Arc::new(
                    Program::decode(
                        &code
                            .into_iter()
                            .flat_map(u16::to_be_bytes)
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
                ),
                Arc::new(resources),
            )
            .unwrap(),
            dialogue: BTreeMap::new(),
            choices: Default::default(),
            walkmesh: navigation::WalkMesh::new(&[resonance_content::field::CollisionGroup {
                surface: 0,
                vertices: vec![[0., 0., 0.], [10., 0., 0.], [0., 10., 0.]],
                triangles: vec![[0, 1, 2]],
            }])
            .unwrap(),
            player_fall: Default::default(),
            light_regions: None,
        }
    }

    #[test]
    fn mapped_pause_blocks_walking_but_preserves_scripted_motion() {
        let mut session = choice_session();
        session.events = EventRuntime::new(
            Arc::new(Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap()),
            Arc::new(ResourceLibrary::default()),
        )
        .unwrap();
        session.walkmesh = navigation::WalkMesh::new(&[resonance_content::field::CollisionGroup {
            surface: 0,
            vertices: vec![[-500., -500., 0.], [500., -500., 0.], [0., 500., 0.]],
            triangles: vec![[0, 1, 2]],
        }])
        .unwrap();
        let player = session.events.world.controlled_actor;
        session
            .events
            .world
            .insert_actor(player, Actor::new(1, [0.; 3]));
        session.events.world.input_enabled = true;
        session.events.world.mapped_input_disabled = true;
        let input = FieldInput {
            direction: [1., 0.],
            ..Default::default()
        };
        session.step(input).unwrap();
        assert_eq!(session.events.world.actors[&player].position, [0.; 3]);
        session.events.world.actors.get_mut(&player).unwrap().motion =
            Some(resonance_events::ActorMotion {
                target: [12., 0., 0.],
                speed: 3.,
            });
        for x in [3., 6., 9., 12.] {
            session.step(input).unwrap();
            assert_eq!(session.events.world.actors[&player].position, [x, 0., 0.]);
        }
        session.events.world.mapped_input_disabled = false;
        session.step(input).unwrap();
        assert_eq!(session.events.world.actors[&player].position, [16., 0., 0.]);
    }

    #[test]
    fn moving_npcs_stop_at_authored_body_radii_without_trapping_the_player() {
        for (player_radius, npc_radius, height, collidable, movement, paused) in [
            (42., 42., 0., true, 0, false),
            (20., 70., 0., true, 0, false),
            (42., 42., 100., true, 0, false),
            (42., 42., 151., true, 0, false),
            (42., 42., 0., true, 1, false),
            (42., 42., 0., true, 2, false),
            (42., 42., 0., false, 2, false),
            (42., 42., 0., true, 2, true),
        ] {
            let mut session = choice_session();
            session.events = EventRuntime::new(
                Arc::new(Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap()),
                Arc::new(ResourceLibrary::default()),
            )
            .unwrap();
            session.walkmesh = navigation::WalkMesh::new(&[0., height].map(|z| {
                resonance_content::field::CollisionGroup {
                    surface: 0,
                    vertices: vec![[-500., -500., z], [500., -500., z], [0., 500., z]],
                    triangles: vec![[0, 1, 2]],
                }
            }))
            .unwrap();
            let player_id = session.events.world.controlled_actor;
            let mut player = Actor::new(1, [0.; 3]);
            player.radius = player_radius;
            session.events.world.insert_actor(player_id, player);
            let mut npc = Actor::new(14, [0., 140., height]);
            npc.radius = npc_radius;
            npc.collidable = collidable;
            let mut autonomy = resonance_events::Autonomy::new(
                resonance_events::Behavior::Wander,
                2.,
                npc.position,
            );
            autonomy.activity = resonance_events::Activity::Walk;
            autonomy.initialized = true;
            autonomy.remaining = 1000;
            if movement == 2 {
                npc.motion = Some(resonance_events::ActorMotion {
                    target: [0., -400., height],
                    speed: 2.,
                });
            } else {
                if movement == 1 {
                    autonomy.behavior = resonance_events::Behavior::FollowPath;
                    npc.path.count = 1;
                    npc.path.points[0] = [0., -400., height];
                }
                npc.autonomy = Some(autonomy);
            }
            session.events.world.insert_actor(304, npc);
            session.events.world.input_enabled = true;
            session.events.world.mapped_input_disabled = paused;
            for _ in 0..90 {
                session.step(FieldInput::default()).unwrap();
            }
            let npc = &session.events.world.actors[&304];
            if height <= ACTOR_CONTACT_HEIGHT && collidable && !paused {
                assert!(
                    npc.position[1] >= player_radius + npc_radius,
                    "NPC penetrated the player: {:?}",
                    npc.position
                );
                assert!(npc.position[1] < player_radius + npc_radius + 2.);
            } else {
                assert!(
                    npc.position[1] < 0.,
                    "disabled, paused, or vertically separate collision must not block the NPC"
                );
            }
            session
                .step(FieldInput {
                    direction: [1., 0.],
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(
                session.events.world.actors[&player_id].position[0],
                if paused { 0. } else { 4. }
            );
        }
    }

    #[test]
    fn falling_player_keeps_descending_with_input_and_can_walk_after_landing() {
        let mut session = choice_session();
        session.events = EventRuntime::new(
            Arc::new(Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap()),
            Arc::new(ResourceLibrary::default()),
        )
        .unwrap();
        session.walkmesh = navigation::WalkMesh::new(&[resonance_content::field::CollisionGroup {
            surface: 0,
            vertices: vec![[-500., -500., 0.], [500., -500., 0.], [0., 500., 0.]],
            triangles: vec![[0, 1, 2]],
        }])
        .unwrap();
        let player = session.events.world.controlled_actor;
        let mut actor = Actor::new(1, [0., 0., 100.]);
        actor.grounded = false;
        session.events.world.insert_actor(player, actor);
        session.step(FieldInput::default()).unwrap();
        assert_eq!(session.events.world.actors[&player].position[2], 100.);
        session.events.world.input_enabled = true;
        session
            .events
            .world
            .actors
            .get_mut(&player)
            .unwrap()
            .grounded = true;
        for height in [91., 73., 46., 10., 10.] {
            session
                .step(FieldInput {
                    direction: [1., 0.],
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(
                session.events.world.actors[&player].position,
                [0., 0., height]
            );
        }
        session
            .step(FieldInput {
                direction: [1., 0.],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(session.events.world.actors[&player].position, [4., 0., 0.]);
    }

    #[test]
    fn live_scenery_floors_connect_a_gap_and_follow_transforms() {
        use resonance_content::field::{CollisionGroup, ModelCollision};
        let rectangle = |x: [f32; 2], y: [f32; 2], z| CollisionGroup {
            surface: 7,
            vertices: vec![
                [x[0], y[0], z],
                [x[1], y[0], z],
                [x[0], y[1], z],
                [x[1], y[1], z],
            ],
            triangles: vec![[0, 1, 2], [1, 3, 2]],
        };
        let mut session = choice_session();
        session.events = EventRuntime::new(
            Arc::new(Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap()),
            Arc::new(ResourceLibrary::default()),
        )
        .unwrap();
        session.walkmesh = navigation::WalkMesh::new(&[
            rectangle([-100., 100.], [-200., 0.], 0.),
            rectangle([-100., 100.], [240., 500.], 20.),
        ])
        .unwrap();
        session.events.world.controlled_actor = 1;
        session.events.world.input_enabled = true;
        session
            .events
            .world
            .insert_actor(1, Actor::new(1, [0., -80., 0.]));
        let mut bridge = Actor::new(SCENERY_RESOURCE_BASE, [0., 0., 10.]);
        bridge.visible = false;
        bridge.grounded = false;
        bridge.collidable = false;
        bridge.face(90.);
        bridge.properties.insert(30, 200);
        bridge.properties.insert(48, 1);
        bridge.model_collision = Some(Arc::new(ModelCollision {
            floors: vec![rectangle([0., 120.], [-80., 80.], 10.)],
            solids: Vec::new(),
        }));
        session.events.world.insert_actor(500, bridge);
        for _ in 0..50 {
            session
                .step(FieldInput {
                    direction: [0., 1.],
                    run: true,
                    ..Default::default()
                })
                .unwrap();
        }
        let reached = session.events.world.actors[&1].position;
        assert!(
            reached[1] > 300. && (reached[2] - 20.).abs() < 0.001,
            "{reached:?}"
        );
        let floor = session.ground_below([0., 120., 40.]).unwrap();
        assert_eq!((floor.height, floor.attributes), (20., 7));

        session.events.world.actors.get_mut(&500).unwrap().position[0] = 400.;
        assert!(session.ground_surface([0., 120., 20.]).is_none());
        assert!(session.ground_surface([400., 120., 20.]).is_some());
        session.events.world.actors.remove(&500);
        assert!(session.ground_below([400., 120., 40.]).is_none());
        session.events.world.actors.get_mut(&1).unwrap().position = [0., -80., 0.];
        for _ in 0..50 {
            session
                .step(FieldInput {
                    direction: [0., 1.],
                    run: true,
                    ..Default::default()
                })
                .unwrap();
        }
        assert!(session.events.world.actors[&1].position[1] < 0.);
    }

    #[test]
    fn exploration_fallback_retires_save_point_and_caption_actors() {
        let mut session = choice_session();
        let world = &mut session.events.world;
        world.insert_actor(world.controlled_actor, Actor::new(1, [2., 2., 0.]));
        world.insert_actor(
            500,
            Actor::new(resonance_content::field::SAVE_POINT_RESOURCE, [0.; 3]),
        );
        world.save_points.push(resonance_events::SavePoint {
            actor: 500,
            position: [0.; 3],
            resource: resonance_content::field::SAVE_POINT_RESOURCE,
            born: 0,
            active: true,
            unlock_flag: None,
            glow_scale: 1.,
        });
        world.insert_actor(999_989, Actor::new(0, [0.; 3]));
        world.overlays.insert(
            999_989,
            resonance_events::Overlay {
                born: 0,
                size: [0; 2],
                rgba: [255; 4],
                duration: 60,
                kind: resonance_events::OverlayKind::LocationCaption { hold_ticks: 60 },
            },
        );
        session
            .enter_exploration("unsupported event".into())
            .unwrap();
        let world = &session.events.world;
        assert!(world.save_points.is_empty());
        assert!(world.overlays.is_empty());
        assert!(!world.actors.contains_key(&500));
        assert!(!world.actors.contains_key(&999_989));
        assert!(world.actors.contains_key(&world.controlled_actor));
    }

    #[test]
    fn exploration_fallback_recovers_initial_and_later_script_failures_and_walks() {
        for initial in [false, true] {
            let mut words = vec![4, 0, 0, 0];
            if !initial {
                native(&mut words, NativeCall::YieldCommand, &[0, 2]);
            }
            native(&mut words, NativeCall::Unknown76, &[0; 2]);
            native(&mut words, NativeCall::SetEventBit, &[123]);
            words.push(0x20ff);
            let program = Arc::new(
                Program::decode(
                    &words
                        .into_iter()
                        .flat_map(u16::to_be_bytes)
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
            );
            if initial {
                assert!(EventRuntime::new(program.clone(), Default::default()).is_err());
            }
            let mut session = choice_session();
            session.allow_incomplete_scripts = true;
            let mut world = resonance_events::GameWorld::default();
            world.controlled_actor = 1;
            world.insert_actor(1, Actor::new(1, [2., 2., 0.]));
            session.events = EventRuntime::with_state_policy(
                program,
                Default::default(),
                world,
                Default::default(),
                true,
            )
            .unwrap();
            session.walkmesh =
                navigation::WalkMesh::new(&[resonance_content::field::CollisionGroup {
                    surface: 0,
                    vertices: vec![[-1000., -1000., 0.], [1000., -1000., 0.], [0., 1000., 0.]],
                    triangles: vec![[0, 1, 2]],
                }])
                .unwrap();
            if let Some(reason) = session.events.exploration_error.clone() {
                session.enter_exploration(reason).unwrap();
            }
            for _ in 0..4 {
                session.step(FieldInput::default()).unwrap();
            }
            assert!(session.player_has_control());
            assert!(session.events.exploration_error.is_some());
            assert_eq!(session.events.world.brightness(), 1.);
            assert!(!session.events.world.event_flags.contains(&123));
            let before = session.events.world.actors[&1].position;
            session
                .step(FieldInput {
                    direction: [1., 0.],
                    ..Default::default()
                })
                .unwrap();
            assert_ne!(session.events.world.actors[&1].position, before);
        }
    }

    #[test]
    fn line_contacts_dispatch_before_presenting_the_same_update() {
        for confirmed in [false, true] {
            let mut code = vec![10, 0, 0, 1, 0, if confirmed { 2 } else { 1 }, 0, 42, 0, 1];
            code.push(0x20ff);
            native(&mut code, NativeCall::DisableMappedInput, &[]);
            native(&mut code, NativeCall::SetTransitionMode, &[1, 20]);
            native(&mut code, NativeCall::YieldCommand, &[0, 20]);
            native(&mut code, NativeCall::EnableMappedInput, &[]);
            code.push(0x20ff);
            let mut session = choice_session();
            session.events = EventRuntime::new(
                Arc::new(
                    Program::decode(
                        &code
                            .into_iter()
                            .flat_map(u16::to_be_bytes)
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
                ),
                Arc::new(ResourceLibrary::default()),
            )
            .unwrap();
            session.walkmesh =
                navigation::WalkMesh::new(&[resonance_content::field::CollisionGroup {
                    surface: 0,
                    vertices: vec![
                        [-1000., -1000., 0.],
                        [1000., -1000., 0.],
                        [-1000., 1000., 0.],
                    ],
                    triangles: vec![[0, 1, 2]],
                }])
                .unwrap();
            let world = &mut session.events.world;
            world.input_enabled = true;
            world.fade = Some(resonance_events::Fade {
                start_tick: 0,
                duration: 1,
                from: 0.,
                to: 0.,
                white: false,
            });
            world.controlled_actor = 1;
            world.insert_actor(1, Actor::new(1, [-496., -317., 0.]));
            world.triggers.push(resonance_events::Trigger {
                activations: 0,
                ring_barrier: false,
                key: 42,
                automatic_event: false,
                shape: resonance_events::TriggerShape::Line([
                    [-540., -264., 0.],
                    [-540., -380., 0.],
                ]),
                height: 200.,
                transition: confirmed.then_some([0; 3]),
                touch_metadata: [0; 3],
            });
            session
                .step(FieldInput {
                    direction: [-1., 0.],
                    ..Default::default()
                })
                .unwrap();
            if confirmed {
                assert!(session.events.world.input_enabled);
                assert_eq!(session.events.world.fade.as_ref().unwrap().alpha(1), 0.);
                session
                    .step(FieldInput {
                        interact: true,
                        ..Default::default()
                    })
                    .unwrap();
            }
            let world = &session.events.world;
            let tick = if confirmed { 2 } else { 1 };
            assert_eq!(world.tick, tick);
            assert_eq!(world.actors[&1].position, [-500., -317., 0.]);
            assert!(!world.input_enabled);
            let fade = world.fade.as_ref().unwrap();
            assert_eq!(fade.start_tick, tick);
            assert!((fade.alpha(tick) - 13.8).abs() < 0.0001, "{fade:?}");
            session
                .step(FieldInput {
                    direction: [-1., 0.],
                    ..Default::default()
                })
                .unwrap();
            let world = &session.events.world;
            assert_eq!(world.actors[&1].position, [-500., -317., 0.]);
            assert!((world.fade.as_ref().unwrap().alpha(tick + 1) - 26.6).abs() < 0.0001);
            for _ in 0..30 {
                session.step(FieldInput::default()).unwrap();
            }
            assert!(session.events.world.input_enabled);
            session.events.world.fade = None;
            session
                .step(FieldInput {
                    interact: true,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(session.events.world.input_enabled, !confirmed);
        }
    }

    #[test]
    fn solid_actor_contact_dispatches_when_collision_prevents_penetration() {
        let mut code = vec![10, 0, 0, 1, 0, 0, 0xffff, 0xfffe, 0, 1, 0x20ff];
        native(&mut code, NativeCall::SetEventBit, &[77]);
        code.push(0x20ff);
        for resource in [2, SCENERY_RESOURCE_BASE + 0x100] {
            let mut session = choice_session();
            session.events = EventRuntime::new(
                Arc::new(
                    Program::decode(
                        &code
                            .clone()
                            .into_iter()
                            .flat_map(u16::to_be_bytes)
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
                ),
                Arc::new(ResourceLibrary::default()),
            )
            .unwrap();
            session.walkmesh =
                navigation::WalkMesh::new(&[resonance_content::field::CollisionGroup {
                    surface: 0,
                    vertices: vec![[-500., -500., 0.], [500., -500., 0.], [0., 500., 0.]],
                    triangles: vec![[0, 1, 2]],
                }])
                .unwrap();
            let world = &mut session.events.world;
            world.input_enabled = true;
            world.controlled_actor = 1;
            let mut player = Actor::new(1, [0.; 3]);
            player.radius = 25.;
            world.insert_actor(1, player);
            let mut obstacle = Actor::new(resource, [52., 0., 0.]);
            obstacle.radius = 25.;
            obstacle.contact_event = true;
            world.insert_actor(42, obstacle);
            session
                .step(FieldInput {
                    direction: [1., 0.],
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(session.events.world.actors[&1].position, [0.; 3]);
            assert!(session.events.world.event_flags.contains(&77));
        }
    }

    #[test]
    fn small_player_moves_slower_and_can_approach_a_narrow_floor_edge() {
        use resonance_events::PlayerSize;
        let mut positions = Vec::new();
        for size in [PlayerSize::Normal, PlayerSize::Small] {
            let mut session = choice_session();
            session.events = EventRuntime::new(
                Arc::new(Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap()),
                Arc::new(ResourceLibrary::default()),
            )
            .unwrap();
            session.walkmesh =
                navigation::WalkMesh::new(&[resonance_content::field::CollisionGroup {
                    surface: 0,
                    vertices: vec![
                        [-100., -100., 0.],
                        [100., -100., 0.],
                        [100., 100., 0.],
                        [-100., 100., 0.],
                    ],
                    triangles: vec![[0, 1, 2], [0, 2, 3]],
                }])
                .unwrap();
            let world = &mut session.events.world;
            world.input_enabled = true;
            world.controlled_actor = 1;
            world.player_size = size;
            world.insert_actor(1, Actor::new(1, [0.; 3]));
            let input = FieldInput {
                direction: [1., 0.],
                ..Default::default()
            };
            session.step(input).unwrap();
            positions.push(session.events.world.actors[&1].position[0]);
            for _ in 0..100 {
                session.step(input).unwrap();
            }
            let x = session.events.world.actors[&1].position[0];
            match size {
                PlayerSize::Normal => assert!((60. ..=64.).contains(&x), "{x}"),
                PlayerSize::Small => assert!((86. ..=88.).contains(&x), "{x}"),
            }
        }
        assert!((positions[0] / positions[1] - 3.).abs() < 0.0001);
    }

    #[test]
    fn movement_heading_preserves_the_observed_genis_house_turn_boundary() {
        // Ten downward steps inside Genis's house finish facing 0 degrees in
        // the source capture, despite a small positive X displacement.
        let heading = movement_heading([0.069_809_62, -3.999_390_8]);
        assert_eq!(heading, 0.999_997_6);
        assert_eq!(heading.trunc(), 0.);
        for (delta, expected) in [
            ([0., -1.], 0.),
            ([1., 0.], 90.),
            ([0., 1.], 180.),
            ([-1., 0.], 270.),
        ] {
            assert_eq!(movement_heading(delta).trunc().rem_euclid(360.), expected);
        }
    }

    #[test]
    fn scripted_door_approach_can_leave_the_floor_while_keyboard_movement_stays_bounded() {
        let start = [-2855.4521, 1513.175, 66.4043];
        let approach = [-2935.7058, 1510.4801, 192.68102];
        for scripted in [false, true] {
            let mut session = choice_session();
            session.events = EventRuntime::new(
                Arc::new(Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap()),
                Arc::new(ResourceLibrary::default()),
            )
            .unwrap();
            session.walkmesh =
                navigation::WalkMesh::new(&[resonance_content::field::CollisionGroup {
                    surface: 0,
                    vertices: vec![
                        [-2925., 1400., start[2]],
                        [-2700., 1400., start[2]],
                        [-2925., 1700., start[2]],
                    ],
                    triangles: vec![[0, 1, 2]],
                }])
                .unwrap();
            session.events.world.input_enabled = !scripted;
            session.events.world.controlled_actor = 1;
            let mut actor = Actor::new(1, start);
            actor.motion = scripted.then_some(resonance_events::ActorMotion {
                target: approach,
                speed: 6.,
            });
            session.events.world.insert_actor(1, actor);
            for _ in 0..15 {
                session
                    .step(FieldInput {
                        direction: [-1., 0.],
                        ..Default::default()
                    })
                    .unwrap();
            }
            let actor = &session.events.world.actors[&1];
            assert_eq!(actor.position[2], start[2]);
            if scripted {
                assert_eq!(actor.position[..2], approach[..2]);
                assert!(
                    actor.motion.is_none(),
                    "approach must finish before the door timeout"
                );
            } else {
                assert!(actor.position[0] < start[0]);
                assert!(session.walkmesh.surface(actor.position, 0.).is_some());
            }
        }
    }

    fn reveal_choices(session: &mut FieldSession) {
        for _ in 0..120 {
            if session.events.world.choices.contains_key(&1)
                && session
                    .dialogue
                    .get(&1)
                    .is_some_and(|page| page.accepts_input())
            {
                return;
            }
            session.step(FieldInput::default()).unwrap();
        }
        panic!("script never finished revealing the question and choices");
    }

    #[test]
    fn fast_confirm_does_not_close_a_page_before_its_choice_activates() {
        let mut session = choice_session();
        for _ in 0..200 {
            session
                .step(FieldInput {
                    interact: true,
                    ..Default::default()
                })
                .unwrap();
            if session.events.world.input_enabled {
                break;
            }
        }
        assert_eq!(session.events.memory().read(0x100, Width::S32).unwrap(), 1);
        assert!(session.events.world.input_enabled);
        assert!(session.events.world.dialogue.is_empty());
        assert_eq!(session.events.active_instances(), 0);
    }

    #[test]
    fn persistent_question_does_not_steal_choice_input_or_release_movement() {
        let mut session = choice_session();
        // An early confirm cannot dismiss the persistent question.
        session
            .step(FieldInput {
                interact: true,
                ..Default::default()
            })
            .unwrap();
        reveal_choices(&mut session);
        assert!(!session.dialogue[&0].closed);
        assert_eq!(session.events.world.choices[&1].selected_line, 0);
        assert!(!session.events.world.input_enabled);
        session
            .step(FieldInput {
                cancel: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            session.events.world.choices[&1]
                .operation
                .progress()
                .outcome,
            None
        );
        session
            .step(FieldInput {
                direction: [0., -1.],
                interact: true,
                ..Default::default()
            })
            .unwrap();
        assert!(session.dialogue[&1].window_visible());
        assert!(!session.events.world.input_enabled);
        for _ in 0..4 {
            session.step(FieldInput::default()).unwrap();
            assert!(!session.dialogue[&1].window_visible());
            assert!(!session.events.world.input_enabled);
        }
        session.step(FieldInput::default()).unwrap();
        assert_eq!(session.events.memory().read(0x100, Width::S32).unwrap(), 2);
        assert_eq!(session.events.memory().read(0x24, Width::S32).unwrap(), 0);
        assert!(session.events.world.dialogue.is_empty());
        assert!(session.events.world.input_enabled);
        assert_eq!(session.events.active_instances(), 0);
    }

    #[test]
    fn shoulder_buttons_confirm_only_choices_that_allow_them() {
        for allowed in [false, true] {
            for previous_page in [false, true] {
                let mut session = choice_session_with_flags(if allowed { 0x300 } else { 0x100 });
                reveal_choices(&mut session);
                session
                    .step(FieldInput {
                        direction: [0., -1.],
                        previous_page,
                        next_page: !previous_page,
                        ..Default::default()
                    })
                    .unwrap();
                for _ in 0..6 {
                    session.step(FieldInput::default()).unwrap();
                }
                assert_eq!(
                    session.events.memory().read(0x100, Width::S32).unwrap(),
                    if allowed { 2 } else { 0 }
                );
                assert_eq!(session.events.world.input_enabled, allowed);
            }
        }
    }

    #[test]
    fn choice_wrap_repeat_and_timeout_wait_for_revealed_text() {
        use crate::choice::{ChoiceInput, ChoicePlayer};
        use resonance_events::dialogue::ChoiceExit;
        let mut session = choice_session();
        reveal_choices(&mut session);
        let mut choice = session.events.world.choices[&1].clone();
        choice.timeout_ticks = Some(30);
        let mut player = ChoicePlayer::default();
        let up = ChoiceInput {
            direction: -1,
            ..Default::default()
        };
        for _ in 0..100 {
            assert_eq!(player.step(&mut choice, up, false), (None, false));
        }
        assert_eq!(player.step(&mut choice, up, true), (None, true));
        assert_eq!(choice.selected_line, 1); // Wrap from first to last.
        for _ in 0..19 {
            assert_eq!(player.step(&mut choice, up, true), (None, false));
        }
        assert_eq!(player.step(&mut choice, up, true), (None, true));
        assert_eq!(choice.selected_line, 0);
        for _ in 0..8 {
            assert_eq!(
                player.step(&mut choice, ChoiceInput::default(), true),
                (None, false)
            );
        }
        assert_eq!(
            player.step(&mut choice, ChoiceInput::default(), true),
            (Some(ChoiceExit::Timeout), false)
        );
        // A new operation resets countdown and repeat state.
        let mut fresh = choice_session();
        reveal_choices(&mut fresh);
        let choice = fresh.events.world.choices.get_mut(&1).unwrap();
        assert_eq!(player.step(choice, up, true), (None, true));
        assert_eq!(choice.selected_line, 1);
    }
}
