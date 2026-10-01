//! Scene ownership, field preparation and checkpoint entry.
mod fields;
use super::audio_output::Player as AudioPlayer;
use super::{Events, PendingAudio, PendingInput, RunOptions, audio, movie};
use anyhow::{Context, Result, ensure};
use bevy::prelude::*;
pub(super) use fields::{FieldPackage, available_fields};
pub(super) use resonance_content::field::preload_path as manifest_path;
use resonance_content::{MovieAsset, field::FieldAssets, prepared::Files};
use resonance_game::field::{FieldCheckpoint, FieldEntry, FieldSession};
use std::{collections::BTreeSet, path::Path, sync::Arc};

#[derive(Resource)]
pub(super) struct Request(pub Option<Vec<u8>>);

/// Keep the source and its pending operation alive after preparation fails.
#[derive(Resource)]
pub(super) struct TransitionFailure;

fn transition_failed(world: &mut World, error: anyhow::Error) {
    error!("Area transition failed: {error:#}");
    world.remove_resource::<super::loading::FieldPending>();
    world.remove_resource::<super::loading::WorldPending>();
    world.insert_resource(TransitionFailure);
    world
        .resource::<super::loading::Resident>()
        .active
        .store(true, std::sync::atomic::Ordering::Release);
    for mut window in world.query::<&mut Window>().iter_mut(world) {
        window.title = format!("Resonance — {error:#} — Enter: Retry / F9: Load quicksave");
    }
}

#[derive(Resource)]
pub(super) struct InitialPreferences(pub resonance_content::menu_data::CustomizeSettings);

/// Slot browsing needs three verified definitions, independently of any field.
pub(super) fn save_context(
    root: &Path,
    diagnostics: resonance_content::diagnostics::Diagnostics,
) -> Result<(
    resonance_persistence::Identity,
    Arc<resonance_content::session::SessionData>,
)> {
    use resonance_content::{
        field_preload::{SHARED_PATH, Shared},
        save_identity,
    };
    let shared: Shared<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(root.join(SHARED_PATH))?)?;
    shared.validate_structure()?;
    let inventory = [
        save_identity::PATH,
        "game/session-data.json",
        "game/menu-data.json",
    ]
    .into_iter()
    .map(|path| {
        let value = shared
            .files
            .get(path)
            .with_context(|| format!("missing save dependency {path}"))?;
        Ok((path.to_owned(), serde_json::from_value(value.clone())?))
    })
    .collect::<Result<_>>()?;
    let files = Files::new(diagnostics).with_dependencies(
        root,
        inventory,
        &mut Default::default(),
        || false,
    )?;
    let identity = resonance_persistence::Identity::load(&files)?;
    let data = resonance_content::session::SessionData::load(&files)?;
    Ok((identity, Arc::new(data)))
}

#[derive(Resource)]
pub(super) struct Session {
    pub scene: Scene,
    pub allow_incomplete_scripts: bool,
    world_package: Option<Arc<super::overworld::Package>>,
    /// Actual publication state, retained for load validation before normal updates.
    pub restored_checkpoint: Option<FieldCheckpoint>,
    pub ready_for_field: bool,
    pub audio: Option<Arc<super::field_audio::Assets>>,
    pub identity: resonance_persistence::Identity,
    story_movie: Option<MovieAsset>,
    pub(super) prepared_movie: Option<movie::Prepared>,
    pending_movie: Option<(u64, super::loading::Task<(MovieAsset, movie::Prepared)>)>,
    movie_started: bool,
    available_fields: BTreeSet<u32>,
}

pub(super) enum Scene {
    Field(Box<FieldScene>),
    World(Box<super::overworld::Scene>),
}

pub(super) struct FieldScene {
    pub session: FieldSession,
    pub package: Arc<FieldPackage>,
}

pub(super) enum Start<'a> {
    NewGame,
    Saved(&'a FieldCheckpoint),
    Dungeon(super::dungeons::Destination),
}

impl Session {
    pub(super) fn movie_owns_audio(&self) -> bool {
        self.is_field() && self.map_id() != 5 && (!self.movie_started || !self.ready_for_field)
    }

    pub(super) fn load(root: &Path) -> Result<Self> {
        let mut cache = super::loading::Cache::default();
        let files = Arc::new(Files::load(
            root,
            &[&manifest_path(5)],
            &mut cache.bytes,
            || false,
        )?);
        Self::load_prepared(root, files, None, None, &mut cache)
    }

    pub(super) fn load_prepared(
        root: &Path,
        files: Arc<Files>,
        saved: Option<FieldCheckpoint>,
        initial_preferences: Option<resonance_content::menu_data::CustomizeSettings>,
        cache: &mut super::loading::Cache,
    ) -> Result<Self> {
        Self::load_start(
            root,
            files,
            saved.as_ref().map_or(Start::NewGame, Start::Saved),
            initial_preferences,
            cache,
        )
    }

    pub(super) fn load_start(
        root: &Path,
        files: Arc<Files>,
        start: Start<'_>,
        initial_preferences: Option<resonance_content::menu_data::CustomizeSettings>,
        cache: &mut super::loading::Cache,
    ) -> Result<Self> {
        ensure!(
            matches!(start, Start::NewGame) || initial_preferences.is_none(),
            "initial preferences require a fresh New Game"
        );
        if let Some(preferences) = &initial_preferences {
            preferences.validate()?;
        }
        let identity = resonance_persistence::Identity::load(&files)?;
        let map = match &start {
            Start::NewGame => 5,
            Start::Saved(checkpoint) => checkpoint.map_id,
            Start::Dungeon(destination) => destination.map,
        };
        let new_game = matches!(start, Start::NewGame);
        let initial = Arc::new(FieldPackage::load(files.clone(), map, cache)?);
        let data = &initial.data;
        let available_fields = available_fields(root)?;
        let field = if let Start::Saved(checkpoint) = &start {
            initial.restore(checkpoint, available_fields.clone())?
        } else {
            let entry = if let Start::Dungeon(destination) = &start {
                destination.entry(data.clone(), available_fields.clone())?
            } else {
                FieldEntry {
                    persistent: resonance_events::PersistentState {
                        party: Some(resonance_events::party::Party::new(
                            data,
                            resonance_events::party::Settings {
                                preferences: initial_preferences.unwrap_or_default(),
                                ..Default::default()
                            },
                        )?),
                        ..Default::default()
                    },
                    available_fields: available_fields.clone(),
                    position: [-719., -371., 0.],
                    idle_animation: Some(116),
                    ..Default::default()
                }
            };
            let mut field = initial.enter(entry)?;
            initial.queue_entry(&mut field, resonance_game::field::EntryKind::Arrival);
            field
        };
        let restored_checkpoint = matches!(start, Start::Saved(_))
            .then(|| field.checkpoint())
            .transpose()?;
        let story_movie = if new_game {
            files.diagnostics().attempt(
                "New Game movie",
                (|| {
                    let movie: MovieAsset = files.json("movies/1.json")?;
                    movie.validate()?;
                    ensure!(
                        !files.is_rejected(&movie.path) && root.join(&movie.path).is_file(),
                        "New Game story movie is missing or failed verification"
                    );
                    Ok(movie)
                })(),
            )?
        } else {
            None
        };
        Ok(Self {
            allow_incomplete_scripts: field.allow_incomplete_scripts,
            scene: Scene::Field(Box::new(FieldScene {
                session: field,
                package: initial.clone(),
            })),
            world_package: None,
            restored_checkpoint,
            ready_for_field: true,
            audio: Some(initial.audio.clone()),
            identity,
            story_movie,
            prepared_movie: None,
            pending_movie: None,
            movie_started: !new_game,
            available_fields,
        })
    }

    pub(super) fn is_field(&self) -> bool {
        matches!(self.scene, Scene::Field(_))
    }

    pub(super) fn menu(&self) -> Option<&resonance_game::menu::Menu> {
        match &self.scene {
            Scene::Field(field) => field.session.menu.as_ref(),
            Scene::World(scene) => scene.session.menu.as_ref(),
        }
    }

    pub(super) fn menu_mut(&mut self) -> Option<&mut resonance_game::menu::Menu> {
        match &mut self.scene {
            Scene::Field(field) => field.session.menu.as_mut(),
            Scene::World(scene) => scene.session.menu.as_mut(),
        }
    }

    pub(super) fn field(&self) -> &FieldSession {
        let Scene::Field(field) = &self.scene else {
            panic!("field access requires an active field");
        };
        &field.session
    }

    pub(super) fn field_mut(&mut self) -> &mut FieldSession {
        let Scene::Field(field) = &mut self.scene else {
            panic!("field access requires an active field");
        };
        &mut field.session
    }

    pub(super) fn field_package(&self) -> &Arc<FieldPackage> {
        let Scene::Field(field) = &self.scene else {
            panic!("field resources require an active field");
        };
        &field.package
    }

    pub(super) fn field_assets(&self) -> &FieldAssets {
        &self.field_package().assets
    }

    pub(super) fn overworld(&self) -> Option<&super::overworld::Scene> {
        match &self.scene {
            Scene::World(scene) => Some(scene),
            Scene::Field(_) => None,
        }
    }

    pub(super) fn overworld_mut(&mut self) -> Option<&mut super::overworld::Scene> {
        match &mut self.scene {
            Scene::World(scene) => Some(scene),
            Scene::Field(_) => None,
        }
    }

    pub(super) fn map_id(&self) -> u32 {
        match &self.scene {
            Scene::Field(field) => field.package.assets.map_id,
            Scene::World(_) => 3000,
        }
    }

    pub(super) fn play_time(&self) -> resonance_game::clock::PlayTime {
        match &self.scene {
            Scene::Field(field) => field.session.play_time,
            Scene::World(scene) => scene.session.play_time,
        }
    }

    pub(super) fn advance_play_time(&mut self) {
        match &mut self.scene {
            Scene::Field(field) => field.session.play_time.advance(),
            Scene::World(scene) => scene.session.play_time.advance(),
        }
    }

    pub(super) fn files(&self) -> Arc<Files> {
        match &self.scene {
            Scene::Field(field) => field.package.files.clone(),
            Scene::World(scene) => scene.package.files.clone(),
        }
    }

    pub(super) fn load_world_prepared(
        root: &Path,
        files: Arc<Files>,
        saved: resonance_game::overworld::Checkpoint,
        cache: &mut super::loading::Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let available_fields = available_fields(root)?;
        let identity = resonance_persistence::Identity::load(&files)?;
        let world = Arc::new(resonance_game::overworld::Prepared::load(
            root,
            Arc::unwrap_or_clone(files),
            &mut cache.bytes,
            available_fields.clone(),
            cancelled,
        )?);
        let data = world
            .resources
            .session_data
            .clone()
            .context("world session data missing")?;
        let persistent = saved.progress.clone().into_state(&data)?;
        let game = resonance_game::overworld::Session::restore(
            world.assets(saved.state.world, &persistent)?,
            saved,
        )?;
        let audio = cache.audio.load("worlds/audio.json", &world.files)?;
        let scene = super::overworld::Scene::new(game, world.clone())?;
        Ok(Self {
            scene: Scene::World(Box::new(scene)),
            allow_incomplete_scripts: false,
            world_package: Some(Arc::new(super::overworld::Package {
                world,
                audio: audio.clone(),
            })),
            ready_for_field: false,
            audio: Some(audio.clone()),
            identity,
            restored_checkpoint: None,
            story_movie: None,
            prepared_movie: None,
            pending_movie: None,
            movie_started: true,
            available_fields,
        })
    }

    pub(super) fn replace_loaded(&mut self, candidate: Self) {
        self.events_mut().cancel();
        *self = candidate;
    }

    pub(super) fn activate(
        &mut self,
        field: FieldSession,
        package: Arc<FieldPackage>,
        starting_story: bool,
    ) {
        self.pending_movie = None;
        self.events_mut().cancel();
        self.restored_checkpoint = None;
        self.allow_incomplete_scripts = field.allow_incomplete_scripts;
        self.ready_for_field = true;
        self.movie_started = !starting_story;
        self.audio = Some(package.audio.clone());
        self.scene = Scene::Field(Box::new(FieldScene {
            session: field,
            package,
        }));
    }

    pub(super) fn data(&self) -> &Arc<resonance_content::session::SessionData> {
        self.events()
            .resources()
            .session_data
            .as_ref()
            .expect("active scene has prepared session definitions")
    }

    pub(super) fn events(&self) -> &resonance_events::EventRuntime {
        match &self.scene {
            Scene::Field(field) => &field.session.events,
            Scene::World(scene) => &scene.session.events,
        }
    }

    pub(super) fn events_mut(&mut self) -> &mut resonance_events::EventRuntime {
        match &mut self.scene {
            Scene::Field(field) => &mut field.session.events,
            Scene::World(scene) => &mut scene.session.events,
        }
    }

    fn change_world(&mut self, package: Arc<super::overworld::Package>) -> Result<()> {
        let persistent = self.events().persistent_state()?;
        let session = if let Some(scene) = self.overworld()
            && let Some(destination) = scene.session.world_destination()
        {
            scene
                .session
                .change_world(package.world.assets(destination, &persistent)?)?
        } else {
            let request = self
                .events()
                .world
                .world_transition
                .as_ref()
                .context("world transition is missing")?;
            let destination = super::overworld::destination(request.location, &persistent)?;
            let play_time = self.play_time();
            let assets = package.world.assets(destination, &persistent)?;
            if (513..=526).contains(&request.location) {
                let definition = package
                    .world
                    .definition
                    .visuals
                    .cinematics
                    .get(&request.location)
                    .context("world cinematic is not prepared")?
                    .clone();
                resonance_game::overworld::Session::play_cinematic(
                    assets,
                    request,
                    Arc::new(definition),
                    persistent,
                    play_time,
                )?
            } else {
                resonance_game::overworld::Session::return_from_field(
                    assets, request, persistent, play_time,
                )?
            }
        };
        let scene = super::overworld::Scene::new(session, package.world.clone())?;
        self.events_mut().cancel();
        self.scene = Scene::World(Box::new(scene));
        self.audio = Some(package.audio.clone());
        self.world_package = Some(package);
        self.ready_for_field = false;
        Ok(())
    }

    fn change_field(&mut self, package: Arc<FieldPackage>) -> Result<()> {
        let request = self
            .events()
            .world
            .field_transition
            .as_ref()
            .context("field transition is missing")?;
        ensure!(
            package.assets.map_id == request.map,
            "prepared field differs from the requested destination"
        );
        let starting_story = self.is_field() && self.map_id() == 5 && request.map == 340;
        let field = if let Some(scene) = self.overworld() {
            let mut entry = scene.session.field_entry()?;
            entry.allow_incomplete_scripts = self.allow_incomplete_scripts;
            let mut field = package.enter(entry)?;
            package.queue_entry(&mut field, resonance_game::field::EntryKind::Arrival);
            field
        } else {
            package.transition(self.field())?
        };
        if let Some(reason) = &field.events.exploration_error {
            warn!(
                "Field {} entered as an exploration preview: {reason}",
                package.assets.map_id
            );
        }
        self.activate(field, package, starting_story);
        Ok(())
    }

    pub(super) fn prepare_movie(
        &mut self,
        root: &Path,
        cancelled: impl Fn() -> bool,
    ) -> Result<()> {
        ensure!(!cancelled(), "movie preparation cancelled");
        let diagnostics = self.files().diagnostics().clone();
        let Some(movie) = self.story_movie.as_ref() else {
            diagnostics.report(
                "New Game movie",
                anyhow::anyhow!("session has no startup movie; skipping video"),
            )?;
            return Ok(());
        };
        let stopped = std::cell::Cell::new(false);
        let prepared = movie::Prepared::load(root, movie, || {
            let stop = cancelled();
            stopped.set(stopped.get() || stop);
            stop
        });
        ensure!(!stopped.get(), "movie preparation cancelled");
        self.prepared_movie = diagnostics.attempt("New Game movie preparation", prepared)?;
        Ok(())
    }
}

pub(super) fn initialize_checkpoint(
    field: &mut FieldSession,
    checkpoint: &FieldCheckpoint,
    data: &resonance_content::session::SessionData,
) -> Result<()> {
    // Field setup may yield; never confirm dialogue to make a checkpoint loadable.
    let mut settled = false;
    for _ in 0..120 {
        let had_control = field.checkpoint().is_ok();
        ensure!(
            !field.events.world.blocked_by_movie()
                && field
                    .events
                    .world
                    .dialogue
                    .values()
                    .all(|d| !d.operation.is_pending())
                && field
                    .events
                    .world
                    .choices
                    .values()
                    .all(|c| !c.operation.is_pending())
                && field.events.world.field_transition.is_none(),
            "saved progression restarts a foreground event"
        );
        // Input release follows actor updates. A complete ordinary pose update
        // may also queue a touch handler; let it retire before accepting the load.
        field.step(Default::default())?;
        if had_control && field.checkpoint().is_ok() {
            settled = true;
            break;
        }
    }
    field
        .checkpoint()
        .context("saved field did not return player control")?;
    ensure!(
        settled,
        "saved field did not keep control through a complete update"
    );
    // Setup builds transient actors and services against a private copy of the
    // saved progress. Its writes and random draws are construction-only. Keep
    // the event clock advancing so waits and effect timestamps stay coherent,
    // then publish the captured progress before ordinary gameplay resumes.
    // The first sixteen script words are expression/choice scratch values.
    for (index, value) in checkpoint
        .progress
        .script_globals
        .iter()
        .enumerate()
        .skip(16)
    {
        field.events.set_global(index as u16, *value)?;
    }
    let mut party = checkpoint.progress.party.clone();
    party.bind_rules(data);
    let world = &mut field.events.world;
    ensure!(
        world.controlled_actor == i32::from(party.field_leader),
        "saved field setup changed its controlled actor"
    );
    world.party = Some(party);
    world
        .event_flags
        .clone_from(&checkpoint.progress.event_flags);
    world
        .event_records
        .clone_from(&checkpoint.progress.event_records);
    world.random_state = checkpoint.progress.random_state;
    world
        .gameplay_random
        .clone_from(&checkpoint.progress.gameplay_random);
    let player = world
        .actors
        .get_mut(&world.controlled_actor)
        .context("saved field has no controlled actor")?;
    // Setup advances ground correction. A save made while walking on a slope
    // must resume at its captured pose, before the next ordinary physics step.
    player.position = checkpoint.position;
    player.face(checkpoint.heading);
    player.motion = None;
    if let Some(animation) = &mut player.animation {
        animation.blend_ticks = 0;
    }
    world
        .field_camera
        .as_mut()
        .context("saved field has no camera")?
        .snap_follow_view(&world.actors);
    // Preparing the scene must not consume the saved gameplay clocks.
    let travel = &mut world
        .party
        .as_mut()
        .context("saved field has no party")?
        .travel;
    let saved = &checkpoint.progress.party.travel;
    travel.field_ticks = saved.field_ticks;
    travel.field_countdown = saved.field_countdown;
    travel.ring_timer = saved.ring_timer;
    field.play_time = resonance_game::clock::PlayTime::resume(checkpoint.played_ticks);
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;

/// Exclusive access makes replacement atomic: validate first, then cancel
/// the old scene. A missing asset leaves the title usable for another attempt.
pub(super) fn enter(world: &mut World) {
    if world.contains_resource::<Session>() {
        return;
    }
    if let Some(request) = world.remove_resource::<Request>()
        && !world.contains_resource::<super::loading::Pending>()
    {
        let preferences = request
            .0
            .is_none()
            .then(|| {
                world
                    .get_resource::<InitialPreferences>()
                    .map(|preferences| preferences.0.clone())
            })
            .flatten();
        match super::loading::Pending::start(
            world.resource::<RunOptions>().assets.clone(),
            world.resource::<RunOptions>().script_root.clone(),
            request.0,
            preferences,
            world.resource::<super::loading::Resident>(),
        ) {
            Ok(pending) => world.insert_resource(pending),
            Err(error) => {
                entry_failed(world, "New Game preparation", error);
                return;
            }
        }
    }
    let Some(pending) = world.get_resource::<super::loading::Pending>() else {
        return;
    };
    let result = match pending.poll() {
        Ok(None) => return,
        Ok(Some(result)) => result,
        Err(error) => Err(error),
    };
    let elapsed = pending.started.elapsed();
    world.remove_resource::<super::loading::Pending>();
    let session = match result {
        Ok(session) => session,
        Err(error) => {
            entry_failed(world, "New Game entry", error);
            return;
        }
    };
    let files = session.files();
    info!(
        "Field bytes verified in {:.3}s: {} bytes read, {} bytes reused",
        elapsed.as_secs_f64(),
        files.disk_bytes,
        files.reused_bytes
    );
    activate(world, session);
}

fn entry_failed(world: &mut World, scope: &str, error: anyhow::Error) {
    let diagnostics = world
        .resource::<super::loading::Resident>()
        .diagnostics
        .clone();
    // The title has not been retired yet. Clearing the one-shot request/worker
    // leaves it available for another action instead of retrying every update.
    world.remove_resource::<Request>();
    world.remove_resource::<super::loading::Pending>();
    if diagnostics.report(scope, error).is_err() {
        world.write_message(AppExit::error());
    }
}

/// A fully validated candidate can replace the title only after preparation succeeds.
pub(super) fn activate(world: &mut World, session: Session) {
    world.remove_resource::<super::TitleActive>();
    super::saves::title::retire(world);
    super::session_screen::retire(world);
    let files = session.files();
    *world
        .resource::<super::loading::Resident>()
        .files
        .write()
        .unwrap() = Some(files);
    world
        .resource::<super::loading::Resident>()
        .active
        .store(false, std::sync::atomic::Ordering::Release);
    if let Some(mut events) = world.remove_resource::<Events>() {
        events.0.cancel();
    }
    world.resource_mut::<PendingAudio>().0 = None;
    world.resource_mut::<audio::MenuSounds>().control = None;
    let music: Vec<_> = world
        .query_filtered::<Entity, With<AudioPlayer<audio::GameAudio>>>()
        .iter(world)
        .collect();
    for entity in music {
        world.despawn(entity);
    }
    // Retain the title for returning sessions and share its output/camera.
    let models: Vec<_> = world
        .query_filtered::<Entity, (With<WorldAssetRoot>, Without<super::scene::PartRoot>)>()
        .iter(world)
        .collect();
    for entity in models {
        world.despawn(entity);
    }
    let title_art: Vec<_> = world
        .query_filtered::<Entity, Or<(
            With<super::TitleQuad>,
            With<super::glow::GlowMesh>,
            With<super::scene::PartRoot>,
        )>>()
        .iter(world)
        .collect();
    for entity in title_art {
        world.entity_mut(entity).insert(Visibility::Hidden);
    }
    let held = world.resource::<PendingInput>().held;
    world.insert_resource(PendingInput {
        held,
        ..Default::default()
    });
    world.insert_resource(session);
    info!("Started field {}", world.resource::<Session>().map_id());
}

/// Replay testing resolves the pending encounter and resumes its original caller.
pub(super) fn skip_test_battles(
    options: Res<RunOptions>,
    dungeons: Option<Res<super::dungeons::Menu>>,
    session: Option<ResMut<Session>>,
) {
    if (options.skip_battles || dungeons.is_some_and(|menu| menu.testing))
        && let Some(mut session) = session
    {
        if options.allow_incomplete_scripts {
            session.allow_incomplete_scripts = true;
            if let Scene::Field(field) = &mut session.scene {
                field.session.allow_incomplete_scripts = true;
            }
        }
        if let Err(error) = session.events_mut().world.skip_battle_as_victory() {
            error!("Could not finish test battle: {error}");
        }
    }
}

/// The VM requests a field; the scene owner replaces it after validating its
/// cooked package. Outstanding callbacks are cancelled before actors retire.
pub(super) fn transition(world: &mut World) {
    if world.contains_resource::<super::saves::Quickload>() {
        return;
    }
    if world.contains_resource::<TransitionFailure>() {
        let retry = world
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Enter)
            || world
                .query::<&Gamepad>()
                .iter(world)
                .any(|pad| pad.just_pressed(GamepadButton::South));
        if !retry {
            return;
        }
        world.remove_resource::<TransitionFailure>();
    }
    let Some(session) = world.get_resource::<Session>() else {
        return;
    };
    if session.events().world.world_transition.is_some()
        || session
            .overworld()
            .is_some_and(|s| s.session.world_destination().is_some())
    {
        transition_world(world);
        return;
    }
    let Some(request) = session.events().world.field_transition.clone() else {
        return;
    };
    let script_root = world
        .get_resource::<RunOptions>()
        .and_then(|options| options.script_root.clone());
    world
        .resource::<super::loading::Resident>()
        .active
        .store(false, std::sync::atomic::Ordering::Release);
    let result = (|| -> Result<()> {
        if !super::field_audio::leave_field(world)? {
            return Ok(());
        }
        let next = if let Some(pending) = world.get_resource::<super::loading::FieldPending>() {
            let Some(result) = pending.poll()? else {
                return Ok(());
            };
            world.remove_resource::<super::loading::FieldPending>();
            Arc::new(result?)
        } else {
            let session = world.resource::<Session>();
            let previous = match &session.scene {
                Scene::Field(field) if field.package.assets.map_id == request.map => {
                    Some(field.package.clone())
                }
                _ => None,
            };
            let pending = super::loading::FieldPending::field(
                world.resource::<RunOptions>().assets.clone(),
                script_root.clone(),
                request.map,
                previous,
                world.resource::<super::loading::Resident>(),
            )?;
            world.insert_resource(pending);
            return Ok(());
        };
        super::field_audio::leave_field(world)?;
        world.resource_mut::<Session>().change_field(next)?;
        let files = world.resource::<Session>().files();
        *world
            .resource::<super::loading::Resident>()
            .files
            .write()
            .unwrap() = Some(files);
        info!("Entered field {} through SymphoniaScript", request.map);
        Ok(())
    })();
    if let Err(error) = result {
        transition_failed(world, error);
    }
}

fn transition_world(world: &mut World) {
    world
        .resource::<super::loading::Resident>()
        .active
        .store(false, std::sync::atomic::Ordering::Release);
    let result = (|| -> Result<()> {
        let package = if let Some(package) = &world.resource::<Session>().world_package {
            package.clone()
        } else if let Some(pending) = world.get_resource::<super::loading::WorldPending>() {
            let Some(result) = pending.poll()? else {
                return Ok(());
            };
            world.remove_resource::<super::loading::WorldPending>();
            Arc::new(result?)
        } else {
            let pending = super::loading::WorldPending::overworld(
                world.resource::<RunOptions>().assets.clone(),
                world.resource::<Session>().available_fields.clone(),
                world.resource::<super::loading::Resident>(),
            )?;
            world.insert_resource(pending);
            return Ok(());
        };
        super::field_audio::leave_field(world)?;
        world.resource_mut::<Session>().change_world(package)?;
        *world
            .resource::<super::loading::Resident>()
            .files
            .write()
            .unwrap() = Some(world.resource::<Session>().files());
        info!("Entered overworld through SymphoniaScript");
        Ok(())
    })();
    if let Err(error) = result {
        transition_failed(world, error);
    }
}

/// Observe movie requests after the field's fixed update. Field loading and
/// input stay in one update path, including the scene before the story movie.
pub(super) fn advance(
    mut session: Option<ResMut<Session>>,
    options: Res<RunOptions>,
    mut movie: ResMut<movie::Playback>,
    mut images: ResMut<Assets<Image>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(session) = &mut session else {
        return;
    };
    if session.overworld().is_some() || session.events().world.screen_request.is_some() {
        return;
    }
    if movie.active {
        return;
    }
    let result = (|| -> Result<()> {
        if let Some(request) = session
            .field()
            .events
            .world
            .movie
            .clone()
            .filter(|request| request.operation.is_pending())
        {
            let prepared = if request.resource == 1 && session.prepared_movie.is_some() {
                Some((
                    session
                        .story_movie
                        .clone()
                        .context("startup movie is missing")?,
                    session.prepared_movie.take().unwrap(),
                ))
            } else {
                if session
                    .pending_movie
                    .as_ref()
                    .is_some_and(|(id, _)| *id != request.operation.id())
                {
                    session.pending_movie = None;
                }
                if let Some((_, pending)) = &session.pending_movie {
                    let Some(prepared) = pending.poll()? else {
                        return Ok(());
                    };
                    session.pending_movie = None;
                    Some(prepared?)
                } else {
                    let asset: MovieAsset = session
                        .files()
                        .json(&format!("movies/{}.json", request.resource))?;
                    asset.validate()?;
                    let root = options.assets.clone();
                    let pending = super::loading::Task::spawn(move |stop| {
                        let prepared = movie::Prepared::load(&root, &asset, || {
                            stop.load(std::sync::atomic::Ordering::Relaxed)
                        })?;
                        Ok((asset, prepared))
                    })?;
                    session.pending_movie = Some((request.operation.id(), pending));
                    None
                }
            };
            let Some((asset, prepared)) = prepared else {
                return Ok(());
            };
            movie.start_script_movie(
                request.resource,
                prepared,
                asset,
                request.operation.clone(),
                &mut images,
            )?;
            session.movie_started = true;
            movie.mono = session
                .field()
                .events
                .world
                .party
                .as_ref()
                .is_some_and(|party| !party.settings.preferences.stereo);
            session.ready_for_field = false;
        } else {
            session.pending_movie = None;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let diagnostics = session.files().diagnostics().clone();
        if diagnostics
            .report("New Game movie; skipping video", error)
            .is_err()
        {
            session.field_mut().events.cancel();
            exit.write(AppExit::error());
        } else {
            if let Some(request) = &session.field().events.world.movie
                && request.operation.is_pending()
                && let Err(error) = request.operation.complete(None)
            {
                let _ = diagnostics.report("skipped movie completion", anyhow::Error::msg(error));
            }
            session.movie_started = true;
            session.ready_for_field = true;
        }
    }
}

pub(super) fn movie_handoff(mut session: Option<ResMut<Session>>, movie: Res<movie::Playback>) {
    if let Some(session) = &mut session
        && session.is_field()
        && session.movie_started
        && !movie.active
        && !session.ready_for_field
        && session
            .field()
            .events
            .world
            .movie
            .as_ref()
            .is_some_and(|request| {
                matches!(
                    request.operation.progress().outcome,
                    Some(resonance_events::Outcome::Completed(_))
                )
            })
    {
        session.ready_for_field = true;
        info!("Movie returned to field {} presentation", session.map_id());
    }
}

/// Renderer readiness follows the active scene for audio, saves and handoffs.
pub(super) fn scene_ready(world: &mut World) -> bool {
    match world
        .get_resource::<Session>()
        .map(|session| session.is_field())
    {
        Some(true) => super::field_view::ready(world),
        Some(false) => super::overworld::ready(world),
        None => false,
    }
}
