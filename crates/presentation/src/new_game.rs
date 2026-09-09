//! Title-to-game ownership and cooked field transitions. Field presentation
//! updates the active scenario and yields to its movie operations.
use super::audio_output::Player as AudioPlayer;
use super::{Events, PendingAudio, PendingInput, RunOptions, audio, movie};
use anyhow::{Context, Result, ensure};
use bevy::prelude::*;
use resonance_content::{MovieAsset, field::FieldAssets};
use resonance_game::field::{FieldEntry, FieldSession};
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[derive(Resource)]
pub(super) struct Request;

#[derive(Resource)]
pub(super) struct Session {
    pub field: FieldSession,
    pub assets: FieldAssets,
    pub ready_for_field: bool,
    pub audio: Option<super::field_audio::Assets>,
    story_movie: MovieAsset,
    pub(super) prepared_movie: Option<movie::Prepared>,
    movie_started: bool,
    data: Arc<resonance_content::session::SessionData>,
    fields: BTreeMap<u32, FieldPackage>,
}

struct FieldPackage {
    assets: FieldAssets,
    script: Vec<u8>,
    messages: Vec<u8>,
}
impl FieldPackage {
    fn load(files: &resonance_content::prepared::Files, path: &str) -> Result<Self> {
        let assets: FieldAssets = files.json(path)?;
        assets.validate()?;
        Ok(Self {
            script: files.read(&assets.script.path)?.to_vec(),
            messages: files.read(&assets.messages)?.to_vec(),
            assets,
        })
    }
}

impl Session {
    pub(super) fn movie_owns_audio(&self) -> bool {
        // The new-session story presentation owns the bus from field entry,
        // including the script updates before it requests the decoder.
        self.assets.map_id != 5 && (!self.movie_started || !self.ready_for_field)
    }

    pub(super) fn load(root: &Path) -> Result<Self> {
        let files = resonance_content::prepared::Files::load(
            root,
            &[
                "fields/new-game-setup.preload.json",
                "fields/iselia-classroom.preload.json",
            ],
            &mut Default::default(),
            || false,
        )?;
        Self::load_prepared(root, &files)
    }

    pub(super) fn load_prepared(
        root: &Path,
        files: &resonance_content::prepared::Files,
    ) -> Result<Self> {
        let classroom = FieldPackage::load(files, "fields/iselia-classroom.json")
            .context("New Game needs cooked classroom assets; run cook-classroom")?;
        let assets = &classroom.assets;
        for (path, expected) in &assets.files {
            ensure!(
                files
                    .manifests
                    .values()
                    .any(|m| m.files.get(path).is_some_and(|f| f.sha256 == *expected)),
                "cooked field dependency changed: {path}; run cook-classroom"
            );
        }
        for part in assets
            .parts
            .iter()
            .chain(assets.actors.iter().flat_map(|actor| &actor.parts))
        {
            for path in std::iter::once(&part.mesh).chain(&part.textures) {
                ensure!(
                    files.bytes.contains_key(path),
                    "New Game asset is missing: {path}; run cook-classroom"
                );
            }
        }
        let ui: resonance_content::font::DialogueArt = files
            .json("ui/dialogue.json")
            .context("New Game needs cooked dialogue art; run cook-classroom")?;
        ui.validate()?;
        ensure!(
            assets.files.contains_key(&ui.font)
                && assets.files.contains_key(&ui.cursor.path)
                && ui
                    .textures
                    .iter()
                    .all(|texture| assets.files.contains_key(&texture.path)),
            "dialogue artwork is missing from field dependency inventory; run cook-classroom"
        );
        let font: resonance_content::font::BitmapFont = files.json(&ui.font)?;
        font.validate()?;
        ensure!(
            assets.files.contains_key(&font.texture),
            "dialogue font is missing from field dependency inventory; run cook-classroom"
        );
        ensure!(
            files.bytes.contains_key(&font.texture)
                && ui
                    .textures
                    .iter()
                    .all(|t| files.bytes.contains_key(&t.path)),
            "New Game dialogue textures are missing; run cook-classroom"
        );
        ensure!(
            assets.files.contains_key("fields/new-game-setup.json")
                && assets.files.contains_key("game/session-data.json"),
            "New Game setup assets are missing; run cook-classroom"
        );
        let setup = FieldPackage::load(files, "fields/new-game-setup.json")?;
        ensure!(
            setup.assets.map_id == 5 && classroom.assets.map_id == 340,
            "incorrect New Game map bindings"
        );
        ensure!(
            setup
                .assets
                .files
                .iter()
                .all(|(path, hash)| assets.files.get(path) == Some(hash)),
            "New Game setup dependency inventory differs from the verified package"
        );
        let data: resonance_content::session::SessionData = files.json("game/session-data.json")?;
        data.validate()?;
        let data = Arc::new(data);
        let mut field = FieldSession::enter(
            &setup.script,
            serde_json::from_slice(&setup.messages)?,
            &setup.assets,
            FieldEntry {
                persistent: resonance_events::PersistentState {
                    party: Some(resonance_events::party::Party::new(
                        &data,
                        Default::default(),
                    )?),
                    ..Default::default()
                },
                data: Some(data.clone()),
                available_fields: [340].into(),
                // Fresh-game camera offsets are relative to the player, as on later field entries.
                position: [-719., -371., 0.],
                heading: 0.,
                // Fresh-game idle pose; no wait is inherited from an earlier scene.
                idle_animation: Some(116),
            },
        )?;
        let movie: MovieAsset = files
            .json("story-intro.json")
            .context("New Game needs its cooked story movie; run cook-story-intro")?;
        movie.validate()?;
        ensure!(
            root.join(&movie.path).is_file(),
            "New Game story movie is missing"
        );
        let audio = super::field_audio::Assets::load_with(root, Some(files))?;
        field.voice_durations = audio.voice_durations();
        Ok(Self {
            field,
            assets: setup.assets,
            ready_for_field: true,
            audio: Some(audio),
            story_movie: movie,
            prepared_movie: None,
            movie_started: false,
            data,
            fields: [(classroom.assets.map_id, classroom)].into(),
        })
    }

    pub(super) fn prepare_movie(
        &mut self,
        root: &Path,
        cancelled: impl Fn() -> bool,
    ) -> Result<()> {
        self.prepared_movie = Some(movie::Prepared::load(root, &self.story_movie, cancelled)?);
        Ok(())
    }
}

/// Exclusive access makes replacement atomic: validate first, then cancel
/// the old scene. A missing asset leaves the title usable for another attempt.
pub(super) fn enter(world: &mut World) {
    if world.contains_resource::<Session>() {
        return;
    }
    if world.remove_resource::<Request>().is_some()
        && !world.contains_resource::<super::loading::Pending>()
    {
        match super::loading::Pending::new(world.resource::<RunOptions>().assets.clone()) {
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
    let prepared = match result {
        Ok(prepared) => prepared,
        Err(error) => {
            error!("Could not start New Game: {error:#}");
            world.write_message(AppExit::error());
            return;
        }
    };
    info!(
        "Field bytes verified in {:.3}s: {} bytes read, {} bytes reused",
        elapsed.as_secs_f64(),
        prepared.files.disk_bytes,
        prepared.files.reused_bytes
    );
    *world
        .resource::<super::loading::Resident>()
        .files
        .write()
        .unwrap() = Some(prepared.files);
    world
        .resource::<super::loading::Resident>()
        .active
        .store(false, std::sync::atomic::Ordering::Release);
    let session = prepared.session;
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
    info!("New Game: running the original setup scenario");
}

/// The VM requests a field; the scene owner replaces it after validating its
/// cooked package. Outstanding callbacks are cancelled before actors retire.
pub(super) fn transition(
    mut session: Option<ResMut<Session>>,
    resident: Res<super::loading::Resident>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(session) = &mut session else {
        return;
    };
    let Some(request) = session.field.events.world.field_transition.clone() else {
        return;
    };
    resident
        .active
        .store(false, std::sync::atomic::Ordering::Release);
    let result = (|| -> Result<()> {
        let next = session
            .fields
            .remove(&request.map)
            .context("requested field is not prepared")?;
        ensure!(
            next.assets.map_id == request.map,
            "requested field binding differs"
        );
        let persistent = session.field.events.take_persistent()?;
        let mut field = FieldSession::enter(
            &next.script,
            serde_json::from_slice(&next.messages)?,
            &next.assets,
            FieldEntry {
                persistent,
                data: Some(session.data.clone()),
                available_fields: Default::default(),
                position: request.position,
                heading: request.heading,
                idle_animation: None,
            },
        )?;
        field.voice_durations = session.field.voice_durations.clone();
        field.voice_feedback = session.field.voice_feedback.clone();
        session.field = field;
        session.assets = next.assets;
        session.ready_for_field = true;
        session.movie_started = false;
        info!("Entered field {} through SymphoniaScript", request.map);
        Ok(())
    })();
    if let Err(error) = result {
        error!("Field transition failed: {error:#}");
        session.field.events.cancel();
        exit.write(AppExit::error());
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
                session
                    .prepared_movie
                    .take()
                    .context("script requested an unprepared movie")?,
                session.story_movie.clone(),
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
                .is_some_and(|party| !party.settings.stereo);
            session.ready_for_field = false;
        }
        Ok(())
    })();
    if let Err(error) = result {
        error!("New Game entry failed: {error:#}");
        session.field.events.cancel();
        exit.write(AppExit::error());
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
