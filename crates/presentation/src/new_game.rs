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

#[derive(Resource)]
pub(super) struct Session {
    pub field: FieldSession,
    pub assets: FieldAssets,
    pub ready_for_field: bool,
    pub audio: Option<super::field_audio::Assets>,
    pub identity: resonance_persistence::Identity,
    story_movie: Option<MovieAsset>,
    pub(super) prepared_movie: Option<movie::Prepared>,
    movie_started: bool,
    pub(super) data: Arc<resonance_content::session::SessionData>,
    skits: Arc<resonance_content::skit::SkitCatalog>,
    fields: BTreeMap<u32, Arc<FieldPackage>>,
    available_fields: BTreeSet<u32>,
}

impl Session {
    pub(super) fn movie_owns_audio(&self) -> bool {
        self.assets.map_id != 5 && (!self.movie_started || !self.ready_for_field)
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
        hash.update(b"Resonance/GQSEAF/rev0/field-checkpoint/1");
        hash.update(std::fs::read(root.join("game/session-data.json"))?);
        // Schema 1's compatibility inputs stay fixed when new fields become
        // playable. Each added package is independently checked on preparation;
        // changing shared data or these original scripts still changes the identity.
        for map in [5, 330, 332, 340] {
            let path = resonance_content::field::metadata_path(map);
            let field: FieldAssets = serde_json::from_slice(&std::fs::read(root.join(path))?)?;
            field.validate()?;
            ensure!(
                field.map_id == map,
                "save content has the wrong field binding"
            );
            let script = std::fs::read(root.join(&field.script.path))?;
            ensure!(
                format!("{:x}", Sha256::digest(&script)) == field.script.sha256,
                "field {map} script identity differs"
            );
            hash.update(map.to_be_bytes());
            hash.update(Sha256::digest(script));
        }
        let mut content: [u8; 32] = hash.finalize().into();
        // Correcting four equipment-owner permissions leaves the saved layout
        // unchanged. Preserve its identity only for this exact content revision;
        // other data or script changes still produce an incompatible identity.
        if content
            == [
                0xfd, 0x25, 0x6c, 0x03, 0x98, 0xd5, 0xc8, 0x07, 0x7d, 0x31, 0x94, 0xf2, 0xdd, 0x79,
                0x12, 0x00, 0x35, 0xe2, 0x98, 0x5b, 0xb0, 0xdf, 0x9c, 0xd1, 0x3f, 0xe8, 0xe7, 0x8b,
                0x64, 0xce, 0xeb, 0xfd,
            ]
        {
            content = [
                0x71, 0xe4, 0x84, 0xac, 0xda, 0x23, 0xd9, 0x5c, 0x3a, 0xc0, 0xb6, 0x66, 0xa8, 0x47,
                0x73, 0x19, 0x98, 0xaf, 0x87, 0x73, 0x5b, 0xdf, 0x06, 0x12, 0x65, 0x16, 0x97, 0xc4,
                0x1e, 0x93, 0x41, 0xac,
            ];
        }
        Ok(resonance_persistence::Identity { schema: 1, content })
    }

    pub(super) fn load_prepared(
        root: &Path,
        files: Arc<Files>,
        saved: Option<FieldCheckpoint>,
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
        let map = saved.as_ref().map_or(5, |c| c.map_id);
        let initial = Arc::new(FieldPackage::load(root, files.clone(), map, cache)?);
        let available_fields = available_fields(root)?;
        let mut entry = if let Some(checkpoint) = &saved {
            checkpoint
                .clone()
                .entry(&initial.assets, data.clone(), available_fields.clone())?
        } else {
            FieldEntry {
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
                position: [-719., -371., 0.],
                heading: 0.,
                idle_animation: Some(116),
                camera: None,
            }
        };
        entry.skits = Some(skits.clone());
        let mut field = initial.enter(entry)?;
        if let Some(checkpoint) = &saved {
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
        let story_movie = if saved.is_none() {
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
        let mut fields = BTreeMap::new();
        if map == 5 {
            fields.insert(340, Arc::new(FieldPackage::load(root, files, 340, cache)?));
        }
        fields.insert(map, initial.clone());
        Ok(Self {
            field,
            assets: initial.assets.clone(),
            ready_for_field: true,
            audio: Some((*initial.audio).clone()),
            identity,
            story_movie,
            prepared_movie: None,
            movie_started: saved.is_some(),
            data,
            skits,
            fields,
            available_fields,
        })
    }

    pub(super) fn files(&self) -> Arc<Files> {
        self.fields[&self.assets.map_id].files.clone()
    }

    pub(super) fn replace_loaded(&mut self, mut candidate: Self) {
        self.field.events.cancel();
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
        self.field.events.cancel();
        self.field = field;
        self.assets = package.assets.clone();
        self.ready_for_field = true;
        self.movie_started = !starting_story;
        self.audio = Some((*package.audio).clone());
    }

    fn change_field(&mut self, package: Arc<FieldPackage>) -> Result<()> {
        let request = self
            .field
            .events
            .world
            .field_transition
            .as_ref()
            .context("field transition is missing")?;
        ensure!(
            package.assets.map_id == request.map,
            "prepared field differs from the requested destination"
        );
        let starting_story = self.assets.map_id == 5 && request.map == 340;
        let mut field = package.enter(FieldEntry {
            play_time: self.field.play_time,
            persistent: self.field.events.persistent_state()?,
            data: Some(self.data.clone()),
            skits: Some(self.skits.clone()),
            available_fields: self.available_fields.clone(),
            position: request.position,
            heading: request.heading,
            camera: request.camera.clone(),
            ..Default::default()
        })?;
        field.continue_ambient(&self.field);
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
mod tests;

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

/// The VM requests a field; the scene owner replaces it after validating its
/// cooked package. Outstanding callbacks are cancelled before actors retire.
pub(super) fn transition(world: &mut World) {
    let Some(session) = world.get_resource::<Session>() else {
        return;
    };
    let Some(request) = session.field.events.world.field_transition.clone() else {
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
        super::field_audio::leave_field(world)?;
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
        let diagnostics = world
            .resource::<super::loading::Resident>()
            .diagnostics
            .clone();
        world.remove_resource::<super::loading::FieldPending>();
        world.resource_mut::<Session>().field.events.cancel();
        if diagnostics.report("field transition", error).is_err() {
            world.write_message(AppExit::error());
        } else if let Err(error) = super::game_over::return_to_title(world) {
            let _ = diagnostics.report("field transition recovery", error);
            world.remove_resource::<Session>();
        }
    }
}

/// Observe movie requests after the field's fixed update. Field loading and
/// input stay in one update path, including the scene before the story movie.
pub(super) fn advance(
    mut session: Option<ResMut<Session>>,
    mut movie: ResMut<movie::Playback>,
    mut images: ResMut<Assets<Image>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(session) = &mut session else {
        return;
    };
    if session.movie_started {
        return;
    }
    let result = (|| -> Result<()> {
        if let Some(request) = session.field.events.world.movie.clone() {
            ensure!(request.resource == 1, "New Game movie binding is missing");
            movie.start_script_movie(
                request.resource,
                session
                    .prepared_movie
                    .take()
                    .context("script requested an unprepared movie")?,
                session
                    .story_movie
                    .clone()
                    .context("script movie is unavailable in this session")?,
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
        }
        Ok(())
    })();
    if let Err(error) = result {
        let diagnostics = session.files().diagnostics().clone();
        if diagnostics
            .report("New Game movie; skipping video", error)
            .is_err()
        {
            session.field.events.cancel();
            exit.write(AppExit::error());
        } else {
            if let Some(request) = &session.field.events.world.movie
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
            "New Game script session is ready for field {} presentation",
            session.assets.map_id
        );
    }
}
