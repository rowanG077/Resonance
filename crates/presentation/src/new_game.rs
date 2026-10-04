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
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

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
pub(super) struct Session {
    pub field: FieldSession,
    pub overworld: Option<super::overworld::Scene>,
    world_package: Option<Arc<super::overworld::Package>>,
    pub assets: FieldAssets,
    pub ready_for_field: bool,
    pub audio: Option<super::field_audio::Assets>,
    pub identity: resonance_persistence::Identity,
    story_movie: Option<MovieAsset>,
    pub(super) prepared_movie: Option<movie::Prepared>,
    pending_movie: Option<(u64, super::loading::Task<(MovieAsset, movie::Prepared)>)>,
    movie_started: bool,
    data: Arc<resonance_content::session::SessionData>,
    skits: Arc<resonance_content::skit::SkitCatalog>,
    fields: BTreeMap<u32, Arc<FieldPackage>>,
    available_fields: BTreeSet<u32>,
}

pub(super) enum Start<'a> {
    NewGame,
    Saved(&'a FieldCheckpoint),
    Dungeon(super::dungeons::Destination),
}

impl Session {
    pub(super) fn movie_owns_audio(&self) -> bool {
        self.overworld.is_none()
            && self.assets.map_id != 5
            && (!self.movie_started || !self.ready_for_field)
    }

    pub(super) fn load(root: &Path) -> Result<Self> {
        let mut cache = super::loading::Cache::default();
        let files = Arc::new(Files::load(
            root,
            &[&manifest_path(5), &manifest_path(340)],
            &mut cache.bytes,
            || false,
        )?);
        Self::load_prepared(root, files, None, &mut cache)
    }

    pub(super) fn identity(root: &Path) -> Result<resonance_persistence::Identity> {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(b"Resonance/field-checkpoint/2");
        hash.update(std::fs::read(root.join("game/session-data.json"))?);
        // Inventories identify all prepared dependencies without reading media payloads.
        for map in available_fields(root)? {
            let path = if map == 3000 {
                resonance_content::overworld::PACKAGE_PATH.to_owned()
            } else {
                manifest_path(map)
            };
            hash.update(map.to_be_bytes());
            hash.update(Sha256::digest(std::fs::read(root.join(path))?));
        }
        Ok(resonance_persistence::Identity {
            schema: 2,
            content: hash.finalize().into(),
        })
    }

    pub(super) fn load_prepared(
        root: &Path,
        files: Arc<Files>,
        saved: Option<FieldCheckpoint>,
        cache: &mut super::loading::Cache,
    ) -> Result<Self> {
        Self::load_start(
            root,
            files,
            saved.as_ref().map_or(Start::NewGame, Start::Saved),
            cache,
        )
    }

    pub(super) fn load_start(
        root: &Path,
        files: Arc<Files>,
        start: Start<'_>,
        cache: &mut super::loading::Cache,
    ) -> Result<Self> {
        let mut data: resonance_content::session::SessionData =
            files.json("game/session-data.json")?;
        let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
        menu.validate()?;
        data.ex_skills = Some(Arc::new(menu.ex_skills));
        data.validate()?;
        let data = Arc::new(data);
        let skits: resonance_content::skit::SkitCatalog = files.json("game/skits.json")?;
        skits.validate()?;
        let skits = Arc::new(skits);
        let identity = Self::identity(root)?;
        let map = match &start {
            Start::NewGame => 5,
            Start::Saved(checkpoint) => checkpoint.map_id,
            Start::Dungeon(destination) => destination.map,
        };
        let saved = match &start {
            Start::Saved(checkpoint) => Some(*checkpoint),
            _ => None,
        };
        let new_game = matches!(start, Start::NewGame);
        let initial = Arc::new(FieldPackage::load(root, files.clone(), map, cache)?);
        let available_fields = available_fields(root)?;
        let mut entry = if let Start::Dungeon(destination) = &start {
            destination.entry(data.clone(), available_fields.clone())?
        } else if let Some(checkpoint) = saved {
            checkpoint
                .clone()
                .entry(&initial.assets, data.clone(), available_fields.clone())?
        } else {
            FieldEntry {
                services: None,
                attachments: Default::default(),
                allow_incomplete_scripts: false,
                kind: Default::default(),
                menu_data: None,
                play_time: Default::default(),
                persistent: resonance_events::PersistentState {
                    party: Some(resonance_events::party::Party::new(
                        &data,
                        Default::default(),
                    )?),
                    ..Default::default()
                },
                data: Some(data.clone()),
                skits: None,
                text: Default::default(),
                available_fields: available_fields.clone(),
                available_movies: Default::default(),
                position: [-719., -371., 0.],
                heading: 0.,
                idle_animation: Some(116),
                camera: None,
            }
        };
        entry.skits = Some(skits.clone());
        let mut field = initial.enter(entry)?;
        if let Some(checkpoint) = saved {
            initialize_checkpoint(&mut field, checkpoint)?;
        }
        initial.queue_entry(
            &mut field,
            if saved.is_some() {
                resonance_game::field::EntryKind::Restore
            } else {
                resonance_game::field::EntryKind::Arrival
            },
        );
        let story_movie = if new_game {
            let movie: MovieAsset = files.json("movies/1.json")?;
            movie.validate()?;
            ensure!(
                root.join(&movie.path).is_file(),
                "New Game story movie is missing"
            );
            Some(movie)
        } else {
            None
        };
        let mut fields = BTreeMap::new();
        if map == 5 {
            fields.insert(340, Arc::new(FieldPackage::load(root, files, 340, cache)?));
        }
        fields.insert(map, initial.clone());
        Ok(Self {
            field,
            overworld: None,
            world_package: None,
            assets: initial.assets.clone(),
            ready_for_field: true,
            audio: Some((*initial.audio).clone()),
            identity,
            story_movie,
            prepared_movie: None,
            pending_movie: None,
            movie_started: !new_game,
            data,
            skits,
            fields,
            available_fields,
        })
    }

    pub(super) fn files(&self) -> Arc<Files> {
        self.overworld.as_ref().map_or_else(
            || self.fields[&self.assets.map_id].files.clone(),
            |scene| scene.package.files.clone(),
        )
    }

    pub(super) fn load_world_prepared(
        root: &Path,
        files: Arc<Files>,
        saved: super::saves::WorldCheckpoint,
        cache: &mut super::loading::Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let available_fields = available_fields(root)?;
        let world = Arc::new(resonance_game::overworld::Prepared::load(
            root,
            &mut cache.bytes,
            available_fields.clone(),
            cancelled,
        )?);
        let data = world
            .resources
            .session_data
            .clone()
            .context("world session data missing")?;
        let skits = world
            .resources
            .skits
            .clone()
            .context("world skits missing")?;
        let persistent = saved.overworld.progress.clone().into_state(&data)?;
        let game = resonance_game::overworld::Session::restore(
            world.assets(saved.overworld.state.world, &persistent)?,
            saved.overworld,
        )?;
        let audio = cache.audio.load(root, "worlds/audio.json", &world.files)?;
        let scene = super::overworld::Scene::new(game, world.clone())?;
        let package = Arc::new(FieldPackage::load(root, files, saved.anchor_field, cache)?);
        // The scene owner retains a suspended field, just as it does on a live
        // exit. It never executes this field's arrival script on a world load.
        let mut field = package.enter(FieldEntry {
            persistent,
            data: Some(data.clone()),
            skits: Some(skits.clone()),
            available_fields: available_fields.clone(),
            ..Default::default()
        })?;
        field.events.cancel();
        Ok(Self {
            field,
            overworld: Some(scene),
            world_package: Some(Arc::new(super::overworld::Package {
                world,
                audio: audio.clone(),
            })),
            assets: package.assets.clone(),
            ready_for_field: false,
            audio: Some((*audio).clone()),
            identity: Self::identity(root)?,
            story_movie: None,
            prepared_movie: None,
            pending_movie: None,
            movie_started: true,
            data,
            skits,
            fields: [(saved.anchor_field, package)].into(),
            available_fields,
        })
    }

    pub(super) fn replace_loaded(&mut self, mut candidate: Self) {
        self.events_mut().cancel();
        for (id, package) in std::mem::take(&mut self.fields) {
            candidate.fields.entry(id).or_insert(package);
        }
        *self = candidate;
    }

    /// Validate and initialize a candidate before touching the current scene.
    pub(super) fn restore(&mut self, checkpoint: FieldCheckpoint) -> Result<()> {
        let package = self
            .fields
            .get(&checkpoint.map_id)
            .context("saved field is not prepared")?
            .clone();
        let mut entry = checkpoint.clone().entry(
            &package.assets,
            self.data.clone(),
            self.available_fields.clone(),
        )?;
        entry.skits = Some(self.skits.clone());
        let mut field = package.enter(entry)?;
        initialize_checkpoint(&mut field, &checkpoint)?;
        package.queue_entry(&mut field, resonance_game::field::EntryKind::Restore);
        self.activate(field, &package, false);
        self.prepared_movie = None;
        Ok(())
    }

    pub(super) fn refresh_scripts(
        &mut self,
        map: u32,
        script_root: Option<std::path::PathBuf>,
        resident: &super::loading::Resident,
    ) -> Result<()> {
        let package = self
            .fields
            .get(&map)
            .context("saved field is not prepared")?;
        let refreshed = resident.refresh_scripts(script_root, package)?;
        self.fields.insert(map, Arc::new(refreshed));
        Ok(())
    }

    fn activate(&mut self, field: FieldSession, package: &FieldPackage, starting_story: bool) {
        self.pending_movie = None;
        self.events_mut().cancel();
        self.overworld = None;
        self.field = field;
        self.assets = package.assets.clone();
        self.ready_for_field = true;
        self.movie_started = !starting_story;
        self.audio = Some((*package.audio).clone());
    }

    pub(super) fn events(&self) -> &resonance_events::EventRuntime {
        self.overworld
            .as_ref()
            .map_or(&self.field.events, |scene| &scene.session.events)
    }

    pub(super) fn events_mut(&mut self) -> &mut resonance_events::EventRuntime {
        self.overworld
            .as_mut()
            .map_or(&mut self.field.events, |scene| &mut scene.session.events)
    }

    fn change_world(&mut self, package: Arc<super::overworld::Package>) -> Result<()> {
        let persistent = self.events().persistent_state()?;
        let session = if let Some(scene) = &self.overworld
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
            let play_time = self
                .overworld
                .as_ref()
                .map_or(self.field.play_time, |s| s.session.play_time);
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
        self.overworld = Some(scene);
        self.audio = Some((*package.audio).clone());
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
        let starting_story =
            self.overworld.is_none() && self.assets.map_id == 5 && request.map == 340;
        let mut entry = if let Some(scene) = &self.overworld {
            scene.session.field_entry()?
        } else {
            FieldEntry {
                play_time: self.field.play_time,
                persistent: self.field.events.persistent_state()?,
                data: Some(self.data.clone()),
                skits: Some(self.skits.clone()),
                available_fields: self.available_fields.clone(),
                position: request.position,
                heading: request.heading,
                camera: request.camera.clone(),
                ..Default::default()
            }
        };
        entry.allow_incomplete_scripts = self.field.allow_incomplete_scripts;
        let mut field = package.enter(entry)?;
        if let Some(reason) = &field.events.exploration_error {
            warn!(
                "Field {} entered as an exploration preview: {reason}",
                package.assets.map_id
            );
        }
        if self.overworld.is_none() {
            field.continue_ambient(&self.field);
        }
        package.queue_entry(&mut field, resonance_game::field::EntryKind::Arrival);
        self.activate(field, &package, starting_story);
        self.fields.insert(package.assets.map_id, package);
        Ok(())
    }

    pub(super) fn prepare_movie(
        &mut self,
        root: &Path,
        cancelled: impl Fn() -> bool,
    ) -> Result<()> {
        self.prepared_movie = Some(movie::Prepared::load(
            root,
            self.story_movie
                .as_ref()
                .context("session has no startup movie")?,
            cancelled,
        )?);
        Ok(())
    }
}

pub(super) fn initialize_checkpoint(
    field: &mut FieldSession,
    checkpoint: &FieldCheckpoint,
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
    let world = &mut field.events.world;
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
    field.play_time = resonance_game::clock::PlayTime::resume(checkpoint.played_ticks());
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
        match super::loading::Pending::start(
            world.resource::<RunOptions>().assets.clone(),
            world.resource::<RunOptions>().script_root.clone(),
            request.0,
            world.resource::<super::loading::Resident>(),
        ) {
            Ok(pending) => world.insert_resource(pending),
            Err(error) => {
                error!("Could not prepare New Game: {error:#}");
                world.write_message(AppExit::error());
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
            error!("Could not start New Game: {error:#}");
            world.write_message(AppExit::error());
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

/// A fully validated candidate can replace the title only after preparation succeeds.
pub(super) fn activate(world: &mut World, session: Session) {
    super::saves::title::retire(world);
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
    // Retire title artwork, while retaining the shared camera/output/movie
    // surfaces for the field renderer to take over.
    let models: Vec<_> = world
        .query_filtered::<Entity, With<WorldAssetRoot>>()
        .iter(world)
        .collect();
    for entity in models {
        world.despawn(entity);
    }
    let title_art: Vec<_> = world
        .query_filtered::<Entity, Or<(With<super::TitleQuad>, With<super::glow::GlowMesh>)>>()
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
    info!(
        "Started field {}",
        world.resource::<Session>().assets.map_id
    );
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
            session.field.allow_incomplete_scripts = true;
        }
        if let Err(error) = session.events_mut().world.skip_battle_as_victory() {
            error!("Could not finish test battle: {error}");
        }
    }
}

/// The VM requests a field; the scene owner replaces it after validating its
/// cooked package. Outstanding callbacks are cancelled before actors retire.
pub(super) fn transition(world: &mut World) {
    if world.contains_resource::<super::saves::WorldLoad>() {
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
            .overworld
            .as_ref()
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
        let next = if let Some(pending) = world.get_resource::<super::loading::FieldPending>() {
            let Some(result) = pending.poll()? else {
                return Ok(());
            };
            world.remove_resource::<super::loading::FieldPending>();
            Arc::new(result?)
        } else if script_root.is_none()
            && let Some(package) = world.resource::<Session>().fields.get(&request.map)
        {
            package.clone()
        } else {
            let pending = super::loading::FieldPending::field(
                world.resource::<RunOptions>().assets.clone(),
                script_root.clone(),
                request.map,
                world
                    .resource::<Session>()
                    .fields
                    .get(&request.map)
                    .cloned(),
                world.resource::<super::loading::Resident>(),
            )?;
            world.insert_resource(pending);
            return Ok(());
        };
        world.resource_mut::<Session>().change_field(next)?;
        super::field_audio::leave_field(world)?;
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
        world.resource_mut::<Session>().change_world(package)?;
        super::field_audio::leave_field(world)?;
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
    if session.overworld.is_some() {
        return;
    }
    if movie.active {
        return;
    }
    let result = (|| -> Result<()> {
        if let Some(request) = session
            .field
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
                .field
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
        error!("Field movie failed: {error:#}");
        session.field.events.cancel();
        exit.write(AppExit::error());
    }
}

pub(super) fn movie_handoff(mut session: Option<ResMut<Session>>, movie: Res<movie::Playback>) {
    if let Some(session) = &mut session
        && session.overworld.is_none()
        && session.movie_started
        && !movie.active
        && !session.ready_for_field
        && session
            .field
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
        info!(
            "Movie returned to field {} presentation",
            session.assets.map_id
        );
    }
}
