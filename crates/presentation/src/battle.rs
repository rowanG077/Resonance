//! The battle scene borrows presentation while retaining the complete field owner.
mod capture;
mod input;
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::{
        RenderTarget, ScalingMode,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
};
use resonance_battle::{Battle, BattleFrame};
use resonance_content::{menu_data::MenuData, prepared::Files, session::SessionData};
use resonance_events::{battle::Request, party::Party};
use resonance_game::battle::{encounter, lifecycle, results};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
};

fn command_cue(event: resonance_game::battle::command::Event) -> Option<u16> {
    match event {
        resonance_game::battle::command::Event::Opened { .. }
        | resonance_game::battle::command::Event::Confirmed { .. } => Some(2),
        resonance_game::battle::command::Event::Navigated { .. } => Some(1),
        resonance_game::battle::command::Event::Disabled { .. } => Some(4),
        resonance_game::battle::command::Event::Cancelled => Some(3),
        resonance_game::battle::command::Event::VoiceStreamsPaused(_) => None,
    }
}

/// Field-owned inputs captured together before the preparation worker starts.
pub(super) struct Entry {
    pub files: Arc<Files>,
    pub party: Party,
    pub setup: resonance_events::battle::Setup,
    pub options: encounter::PrepareOptions,
    pub data: Arc<SessionData>,
    pub libc_seed: u32,
}

pub(super) struct Package {
    pub assets: Arc<encounter::Assets>,
    pub prepared: encounter::Prepared,
    pub audio: Arc<super::battle_audio::Assets>,
    pub party: Party,
    pub menus: Arc<MenuData>,
    pub data: Arc<SessionData>,
    pub catalogue: Arc<resonance_content::arte::Catalogue>,
    pub libc_seed: u32,
    pub entry_seed: u32,
}

#[derive(Resource)]
pub(super) struct Owner {
    request: Request,
    phase: Phase,
    input_tick: u32,
    // Only the pre-bank pause is outstanding; Playback takes it at Begin.
    music_paused: bool,
    entry: Option<super::battle_entry::View>,
}
impl Owner {
    pub(super) fn failed(&self) -> bool {
        matches!(self.phase, Phase::Failed)
    }

    pub(super) fn presenting(&self) -> bool {
        matches!(&self.phase, Phase::Scene(scene) if scene.presenting && scene.failure.is_none()
            && scene.dispatch.admitted(scene.gpu_ready))
    }
    pub(super) fn owns_presentation(&self) -> bool {
        // The retained field cameras own a valid frozen image until the entry
        // camera takes over. Host preparation must not insert a black flash.
        self.entry.is_some()
    }
}
enum Phase {
    Loading(super::loading::BattlePending),
    Scene(Box<Scene>),
    Failed,
}

fn diagnostics(world: &World) -> resonance_content::diagnostics::Diagnostics {
    super::diagnostics::policy(world)
}

/// A missing mandatory battle input cannot produce a meaningful outcome. Keep
/// the application usable without completing its caller or saving guessed state.
fn finish_failed(world: &mut World, owner: Owner) {
    if diagnostics(world).paranoid() {
        let request = owner.request.clone();
        retire(world, owner, false);
        world.insert_resource(Owner {
            request,
            phase: Phase::Failed,
            input_tick: 0,
            music_paused: false,
            entry: None,
        });
        world.write_message(AppExit::error());
    } else {
        retire(world, owner, false);
        if let Err(error) = super::game_over::return_to_title(world) {
            let _ = diagnostics(world).report("return from failed battle", error);
        }
    }
}

struct Scene {
    view: super::battle_view::View,
    hud: super::field_ui::BattleHud,
    game_over: Option<super::field_ui::GameOverArt>,
    audio: Arc<super::battle_audio::Assets>,
    settings: resonance_content::menu_data::CustomizeSettings,
    core: Battle,
    lifecycle: lifecycle::Lifecycle,
    candidate: Option<results::Candidate>,
    characters: Vec<u8>,
    actors: Vec<encounter::ActorActions>,
    action_names: BTreeMap<u16, String>,
    music: u16,
    frame: Option<BattleFrame>,
    camera: Option<Entity>,
    target: RenderTarget,
    active: bool,
    presenting: bool,
    gpu_ready: bool,
    dispatch: super::battle_entry::Dispatch,
    transition: resonance_game::battle::entry_transition::EntryTransition,
    switched: bool,
    failure: Option<String>,
    completed: Option<results::Completed>,
    requested_music: Option<u16>,
    generation: u64,
    entry_seed: u32,
    preparing_since: std::time::Instant,
    diagnostics: resonance_content::diagnostics::Diagnostics,
}

/// Used by every field simulation and rendering pass, including standalone
/// captures which do not install the live scene owner.
pub(super) fn field_running(
    session: Option<Res<super::new_game::Session>>,
    resident: Option<Res<super::loading::Resident>>,
) -> bool {
    !session.is_some_and(|session| session.field.events.battle_pending())
        && !resident.is_some_and(|resident| resident.battle.load(Ordering::Acquire))
}

/// The request-producing update still publishes its final field pose and UI.
/// Simulation keeps using field_running; this gate admits only publication.
pub(super) fn field_presenting(
    owner: Option<Res<Owner>>,
    resident: Option<Res<super::loading::Resident>>,
) -> bool {
    owner.as_ref().is_some_and(|owner| {
        owner
            .entry
            .as_ref()
            .is_some_and(super::battle_entry::View::needs_field_publication)
    }) || !resident.is_some_and(|resident| resident.battle.load(Ordering::Acquire))
}

fn publish_field(mut commands: Commands, owner: Option<ResMut<Owner>>) {
    if let Some(mut owner) = owner
        && let Some(entry) = &mut owner.entry
    {
        entry.publish(&mut commands);
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<input::Controls>()
        .add_systems(PreUpdate, input::gather.after(bevy::input::InputSystems))
        .add_systems(Update, manage.after(super::field_view::FieldPreparation))
        .add_systems(Update, prepare.after(manage))
        .add_systems(PostUpdate, publish_field.after(super::field_audit::check))
        .add_systems(
            FixedUpdate,
            advance
                .after(super::field_view::advance_live)
                .after(super::timing::advance_clock),
        )
        .add_systems(
            PostUpdate,
            render
                .after(super::sparse_animation::affine::propagate)
                .before(bevy::asset::AssetEventSystems)
                .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds)
                .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        );
    super::battle_view::install(app);
    super::battle_entry::install(app);
}

/// Move the exact request only after the worker is owned. Neither preparation
/// failure nor cancellation retires the retained event VM or its party state.
fn manage(world: &mut World) {
    if !world.contains_resource::<Owner>() {
        let Some(request) = world
            .get_resource::<super::new_game::Session>()
            .and_then(|session| session.field.events.world.battle_request.clone())
        else {
            return;
        };
        if !request.is_pending() {
            return;
        }
        let random_seed = entry_seed(world, &request);
        let result = (|| -> Result<Entry> {
            let session = world.resource::<super::new_game::Session>();
            let options = encounter::PrepareOptions {
                random_seed: random_seed?,
                map: u16::try_from(session.assets.map_id)?,
                world_music: session
                    .field
                    .events
                    .memory()
                    .read(0x50, symphonia_script::Width::S32)?,
                story: session.field.story_progress()?,
                // The current session/save format supports a fresh game only;
                // it has no New Game Plus Increase Tension grade-shop flag.
                overlimit_boost: false,
            };
            Ok(Entry {
                files: session.files(),
                party: session
                    .field
                    .events
                    .world
                    .party
                    .clone()
                    .context("battle has no party")?,
                setup: request.setup,
                options,
                data: session.data.clone(),
                libc_seed: session.field.events.world.random_state,
            })
        })();
        let mut music_paused = false;
        let result = result.and_then(|entry| {
            world
                .get_resource_mut::<super::field_audio::Control>()
                .context("battle needs the retained audio mixer")?
                .prepare_battle_entry()?;
            music_paused = true;
            super::loading::BattlePending::battle(
                world.resource::<super::RunOptions>().assets.clone(),
                entry,
                world.resource::<super::loading::Resident>(),
            )
        });
        let mut entry = None;
        let phase = match result.and_then(|task| {
            entry = Some(super::battle_entry::View::capture(world)?);
            Ok(task)
        }) {
            Ok(task) => Phase::Loading(task),
            Err(error) => {
                let _ = diagnostics(world).report("battle preparation", error);
                Phase::Failed
            }
        };
        let resident = world.resource::<super::loading::Resident>();
        resident.battle.store(true, Ordering::Release);
        resident
            .active
            .store(matches!(phase, Phase::Failed), Ordering::Release);
        world
            .resource_mut::<super::field_view::Controls>()
            .clear_actions();
        world.resource_mut::<input::Controls>().reset();
        world
            .resource_mut::<super::new_game::Session>()
            .field
            .events
            .world
            .battle_request
            .take();
        world.insert_resource(Owner {
            request,
            phase,
            input_tick: 0,
            music_paused,
            entry,
        });
    }
    let Some(mut owner) = world.remove_resource::<Owner>() else {
        return;
    };
    if matches!(owner.phase, Phase::Failed) {
        finish_failed(world, owner);
        return;
    }
    if !owner.request.is_pending() || !world.contains_resource::<super::new_game::Session>() {
        retire(world, owner, false);
        return;
    }
    if let Phase::Scene(scene) = &mut owner.phase {
        if scene.failure.is_none() {
            scene.failure = scene.view.check().err().map(|error| format!("{error:#}"));
        }
        if let Some(error) = scene.failure.take() {
            let _ = scene.diagnostics.report("battle", anyhow::anyhow!(error));
            finish_failed(world, owner);
            return;
        }
    }

    if let Phase::Loading(task) = &owner.phase
        && let Some(audio) = task.poll_audio()
    {
        let result = begin_audio(world, audio);
        if world.contains_resource::<super::battle_audio::Playback>() {
            owner.music_paused = false;
        }
        if let Err(error) = result {
            let _ = diagnostics(world).report("battle audio preparation", error);
            finish_failed(world, owner);
            return;
        }
    }

    let completed = match &mut owner.phase {
        Phase::Scene(scene) => scene.completed.take(),
        _ => None,
    };
    if let Some(completed) = completed {
        if matches!(&owner.phase, Phase::Scene(scene) if scene.core.is_diagnostic()) {
            let _ = diagnostics(world).report(
                "battle result",
                anyhow::anyhow!("diagnostic battle finished; persistent result discarded"),
            );
            finish_failed(world, owner);
            return;
        }
        if completed.result == resonance_battle::BattleResult::Defeat
            && owner.request.setup.defeat == resonance_events::battle::DefeatPolicy::GameOver
        {
            let Phase::Scene(scene) = &mut owner.phase else {
                unreachable!()
            };
            let Some(art) = scene.game_over.take() else {
                scene.failure = Some("fatal defeat has no prepared game-over artwork".into());
                world.insert_resource(owner);
                return;
            };
            if !art.ready(world.resource::<Assets<Image>>()) {
                scene.game_over = Some(art);
                scene.failure = Some("game-over artwork lost its prepared resources".into());
                world.insert_resource(owner);
                return;
            }
            let request = owner.request.clone();
            // Defeat music and the retained pending caller pass to Game Over;
            // no successful field callback or battle-audio End is issued.
            dispose(world, owner, None);
            if let Err(error) = super::game_over::enter(world, art) {
                error!("Game-over entry failed: {error:#}");
                world.insert_resource(Owner {
                    request,
                    phase: Phase::Failed,
                    input_tick: 0,
                    music_paused: false,
                    entry: None,
                });
            }
            return;
        }
        if let Err(error) = commit(world, &owner.request, completed) {
            if let Phase::Scene(scene) = &mut owner.phase {
                scene.failure = Some(format!("{error:#}"));
            }
            world.insert_resource(owner);
        } else {
            retire(world, owner, true);
        }
        return;
    }
    let result = match &owner.phase {
        Phase::Loading(task) => match task.poll() {
            Ok(None) => None,
            Ok(Some(result)) => Some(result),
            Err(error) => Some(Err(error)),
        },
        _ => None,
    };
    if let Some(result) = result {
        match result.and_then(|package| scene(world, package, owner.request.id())) {
            Ok(scene) => owner.phase = Phase::Scene(Box::new(scene)),
            Err(error) => {
                let _ = diagnostics(world).report("battle preparation", error);
                owner.phase = Phase::Failed;
                world
                    .resource::<super::loading::Resident>()
                    .active
                    .store(false, Ordering::Release);
            }
        }
    }
    world.insert_resource(owner);
}

fn begin_audio(world: &mut World, audio: super::loading::BattleAudio) -> Result<()> {
    ensure!(
        !world.contains_resource::<super::battle_audio::Playback>(),
        "battle audio already owned"
    );
    let mut control = world
        .get_resource_mut::<super::field_audio::Control>()
        .context("battle needs the retained audio mixer")?;
    let playback =
        super::battle_audio::Playback::begin(&mut control, audio.assets, audio.settings)?;
    // Keep ownership even if the subsequent music request fails: dispose must
    // return the queued Begin to the retained field on every failure path.
    world.insert_resource(playback);
    world
        .resource::<super::battle_audio::Playback>()
        .music(Some(i16::try_from(audio.track)?), 100)
}

fn entry_seed(world: &mut World, request: &Request) -> Result<u32> {
    if let Some(mut seeds) = world.get_resource_mut::<super::saves::BattleSeeds>() {
        return seeds.for_request(request.id(), request.setup.encounter);
    }
    Ok(resonance_game::battle::entry::Random::from_elapsed_millis(
        world
            .resource::<super::CaptureStart>()
            .0
            .elapsed()
            .as_millis() as u64,
    )
    .state())
}

/// 44E1C masks names by declared formation resource, independent of Monster
/// Book knowledge. The original enemy header supplies the byte-pair count.
fn enemy_hud(
    assets: &encounter::Inputs,
    prepared: &encounter::Prepared,
) -> Result<Vec<super::field_ui::BattleEnemyHud>> {
    let mut groups = Vec::new();
    for (group, enemy) in assets.enemies.resources.iter().enumerate() {
        let source: resonance_content::battle_model::Enemy = assets
            .files
            .json(&resonance_content::battle_model::enemy_path(enemy.id))?;
        let name = if assets.enemies.hidden_names & (1 << group) != 0 {
            assets
                .ui
                .intro
                .hidden_name_symbol
                .repeat(usize::from(source.hidden_name_units))
        } else {
            source.name
        };
        // The existing enemy effect bank already publishes native member 0x1d4
        // in slot 14. Reuse its first texture/page instead of cooking it again.
        let icon = assets
            .enemy_effects
            .get(&enemy.id)
            .and_then(|bank| bank.art.as_ref())
            .and_then(|art| art.textures.get(&14))
            .and_then(|textures| textures.first())
            .and_then(|texture| texture.images.first())
            .with_context(|| format!("enemy {} has no prepared HUD icon", enemy.id))?
            .clone();
        groups.push((name, icon));
    }
    let actors = prepared.core.actor_ids().skip(prepared.characters.len());
    ensure!(
        actors.count() == assets.enemies.spawns.len(),
        "enemy HUD roster differs from prepared battle"
    );
    prepared
        .core
        .actor_ids()
        .skip(prepared.characters.len())
        .zip(&assets.enemies.spawns)
        .map(|(actor, spawn)| {
            let (name, icon) = groups
                .get(spawn.resource)
                .context("enemy HUD group is not prepared")?;
            Ok(super::field_ui::BattleEnemyHud {
                actor: actor.index(),
                group: u8::try_from(spawn.resource)?,
                name: name.clone(),
                icon: icon.clone(),
            })
        })
        .collect()
}

fn action_names(
    prepared: &encounter::Prepared,
    catalogue: &resonance_content::arte::Catalogue,
) -> Result<BTreeMap<u16, String>> {
    prepared
        .actors
        .iter()
        .flat_map(|actor| &actor.techniques)
        .map(|(&technique, &action)| {
            let name = catalogue
                .definition(usize::from(technique))?
                .name
                .as_ref()
                .with_context(|| format!("technique {technique} has no prepared name"))?;
            Ok((action, name.clone()))
        })
        .collect()
}

fn scene(world: &mut World, package: Package, generation: u64) -> Result<Scene> {
    let diagnostics = diagnostics(world);
    let Package {
        assets,
        prepared,
        audio,
        party,
        menus,
        data,
        catalogue,
        libc_seed,
        entry_seed,
    } = package;
    let enemies = enemy_hud(&assets, &prepared)?;
    let action_names = action_names(&prepared, &catalogue)?;
    *world
        .resource::<super::loading::Resident>()
        .files
        .write()
        .unwrap() = Some(Arc::new(assets.files.clone()));
    let server = world.resource::<AssetServer>().clone();
    let toon = world.resource::<super::field_view::Art>().toon_ramp.clone();
    let target = world
        .query_filtered::<&RenderTarget, With<super::FieldCamera>>()
        .single(world)?
        .clone();
    let (mut hud, game_over) = world.resource_scope(
        |world, mut materials: Mut<Assets<super::field_ui::Surface>>| {
            let mut images = world.resource_mut::<Assets<Image>>();
            let hud = super::field_ui::BattleHud::load(
                |path| Ok(assets.files.read(path)?.to_vec()),
                &enemies,
                &server,
                &mut materials,
                &mut images,
            )?;
            let game_over = assets
                .game_over
                .as_ref()
                .map(|art| super::field_ui::GameOverArt::load(art, &server, &mut materials))
                .transpose()?;
            Ok::<_, anyhow::Error>((hud, game_over))
        },
    )?;
    let view = super::battle_view::View::load_with_diagnostics(
        assets.stage.clone(),
        prepared
            .models
            .into_iter()
            .map(|model| super::battle_view::ModelAsset {
                resource: model.resource,
                skeleton: model.skeleton,
                parts: model.parts,
                capacity: model.capacity,
                lit: model.lit,
                effect: model.effect,
                texture_channels: model.texture_channels,
                weapon_flags: model.weapon_flags,
                suppressed_nodes: model.suppressed_nodes,
            })
            .collect(),
        toon,
        prepared
            .effects
            .into_iter()
            .map(|effect| super::battle_view::EffectBank {
                resource: effect.resource,
                source: effect.source,
                textures: effect.textures,
                members: effect.members,
                palettes: effect.palettes,
            })
            .collect(),
        prepared
            .trails
            .into_iter()
            .map(|trail| super::battle_view::TrailAsset {
                resource: trail.resource,
                material: trail.material,
                texture: trail.texture,
                capacity: trail.capacity,
            })
            .collect(),
        &assets.ui,
        &server,
        diagnostics.clone(),
    )?;
    let settings = party.settings.preferences.clone();
    hud.settings(settings.clone());
    let candidate =
        results::Candidate::new(prepared.results, party, libc_seed, data, menus, catalogue)?;
    let mut core = Battle::new(prepared.core);
    core.set_diagnostics(diagnostics.clone());
    Ok(Scene {
        view,
        hud,
        game_over,
        audio,
        settings,
        core,
        lifecycle: prepared.lifecycle.start()?,
        candidate: Some(candidate),
        characters: prepared.characters,
        actors: prepared.actors,
        action_names,
        music: prepared.music,
        frame: None,
        camera: None,
        target,
        active: false,
        presenting: false,
        gpu_ready: false,
        dispatch: default(),
        transition: prepared.entry_transition,
        switched: false,
        failure: None,
        completed: None,
        requested_music: None,
        generation,
        entry_seed,
        preparing_since: std::time::Instant::now(),
        diagnostics,
    })
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Shared renderer assets and the retained camera/audio owners.
fn prepare(
    mut commands: Commands,
    owner: Option<ResMut<Owner>>,
    server: Res<AssetServer>,
    mut assets: super::battle_view::AssetsForView,
    entities: super::battle_view::Entities,
    warmup: Res<super::battle_view::Warmup>,
    resident: Res<super::loading::Resident>,
    mut cameras: Query<
        &mut Camera,
        Or<(With<super::FieldCamera>, With<super::FieldOverlayCamera>)>,
    >,
    mut outputs: ResMut<Assets<super::materials::TitleOutput>>,
    mut entry_materials: ResMut<Assets<super::field_ui::Surface>>,
) {
    let Some(mut owner) = owner else {
        return;
    };
    let Owner { phase, entry, .. } = &mut *owner;
    let Phase::Scene(scene) = phase else {
        return;
    };
    if scene.active || scene.failure.is_some() {
        return;
    }
    let result = (|| -> Result<()> {
        ensure!(
            scene.preparing_since.elapsed().as_secs() <= 120,
            "battle GPU preparation timed out"
        );
        let entry = entry.as_mut().context("battle lost its field capture")?;
        let entry_ready = entry.prepare(
            &scene.transition,
            &mut commands,
            &mut assets.images,
            &mut assets.meshes,
            &mut entry_materials,
            scene.target.clone(),
        )?;
        if entry.owns_target() {
            for mut camera in &mut cameras {
                camera.is_active = false;
            }
            super::materials::TitleOutput::update(&mut outputs, |brightness| {
                brightness.x = 1.;
                brightness.y = 0.;
            });
        }
        if entry_ready && !scene.presenting {
            scene.presenting = true;
        }
        scene.hud.prepare(&mut commands, &mut assets.meshes);
        if let Some(art) = &mut scene.game_over {
            art.prepare(&mut commands, &mut assets.meshes);
        }
        let draws: Vec<_> = scene
            .hud
            .entities()
            .chain(scene.game_over.iter().flat_map(|art| art.entities()))
            .collect();
        let images: Vec<_> = scene
            .hud
            .images()
            .iter()
            .chain(scene.game_over.iter().flat_map(|art| art.images()))
            .collect();
        for image in &images {
            if !assets.images.contains(*image)
                && let Some(bevy::asset::LoadState::Failed(error)) =
                    server.get_load_state(image.id())
            {
                scene.diagnostics.report(
                    "battle HUD image",
                    anyhow::anyhow!("battle HUD image failed to load: {error}"),
                )?;
                assets
                    .images
                    .insert(image.id(), super::battle_view::placeholder_image())?;
            }
        }
        let ready = images.iter().all(|image| assets.images.contains(*image));
        if !scene.view.prepare(
            &mut commands,
            &server,
            &mut assets,
            &entities,
            &warmup,
            ready.then_some(draws.as_slice()),
        )? {
            if scene.camera.is_none()
                && let Some(target) = scene.view.warm_target()
            {
                let camera = commands
                    .spawn((
                        Camera2d,
                        Msaa::Off,
                        Tonemapping::None,
                        Camera {
                            order: -14,
                            clear_color: ClearColorConfig::None,
                            ..default()
                        },
                        RenderTarget::Image(target.into()),
                        RenderLayers::layer(super::battle_view::WARM_LAYER),
                        super::camera::overlay_alignment(),
                        overlay_projection(),
                    ))
                    .id();
                scene.camera = Some(camera);
            }
            for entity in draws {
                commands.entity(entity).insert((
                    RenderLayers::layer(super::battle_view::WARM_LAYER),
                    Visibility::Visible,
                    NoFrustumCulling,
                ));
            }
            return Ok(());
        }
        if !scene.switched {
            if !entry_ready {
                return Ok(());
            }
            scene.view.activate(&mut commands, scene.target.clone())?;
            super::battle_view::activate_camera(
                &mut commands,
                scene.camera.context("battle HUD camera was not warmed")?,
                scene.target.clone(),
                overlay_projection(),
                -3,
            );
            for entity in scene
                .hud
                .entities()
                .chain(scene.game_over.iter().flat_map(|art| art.entities()))
            {
                commands.entity(entity).insert((
                    RenderLayers::layer(super::battle_view::LAYER),
                    Visibility::Hidden,
                ));
            }
            for mut camera in &mut cameras {
                camera.is_active = false;
            }
            scene.frame = Some(scene.core.snapshot());
            scene.switched = true;
            return Ok(());
        }
        if !scene.view.active_ready()? {
            return Ok(());
        }
        scene
            .view
            .apply_feedback(&mut commands, scene.hud.feedback())?;
        scene.gpu_ready = true;
        resident.active.store(true, Ordering::Release);
        Ok(())
    })();
    if let Err(error) = result {
        error!("Battle scene preparation failed: {error:#}");
        // Keep the candidate and field owner intact for diagnostics; no caller
        // completion or persistent writeback can occur on this path.
        scene.failure = Some(format!("{error:#}"));
        resident.active.store(false, Ordering::Release);
    }
}

pub(super) fn overlay_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::Fixed {
            width: 640.,
            height: 480.,
        },
        ..OrthographicProjection::default_2d()
    })
}

fn restore_reader(world: &mut World) {
    let files = world
        .get_resource::<super::new_game::Session>()
        .map(|session| session.files());
    let resident = world.resource::<super::loading::Resident>();
    *resident.files.write().unwrap() = files;
    resident.active.store(true, Ordering::Release);
}

fn retire(world: &mut World, owner: Owner, resume: bool) {
    dispose(world, owner, Some(resume));
    restore_reader(world);
    world
        .resource::<super::loading::Resident>()
        .battle
        .store(false, Ordering::Release);
}

fn dispose(world: &mut World, owner: Owner, audio_return: Option<bool>) {
    if let Some(resume) = audio_return {
        let playback = world.remove_resource::<super::battle_audio::Playback>();
        if let Some(mut control) = world.get_resource_mut::<super::field_audio::Control>() {
            let result = if let Some(playback) = playback {
                playback.finish(&mut control, resume)
            } else if owner.music_paused {
                control.cancel_battle_entry(resume)
            } else {
                Ok(())
            };
            if let Err(error) = result {
                error!("Battle audio return failed: {error:#}");
            }
        }
    }
    if let Some(entry) = owner.entry {
        entry.dispose(world);
    }
    if let Phase::Scene(scene) = owner.phase {
        let Scene {
            view,
            hud,
            camera,
            game_over,
            ..
        } = *scene;
        view.despawn(&mut world.commands());
        hud.despawn(world);
        if let Some(art) = game_over {
            art.despawn(world);
        }
        if let Some(camera) = camera {
            world.despawn(camera);
        }
    }
    for mut camera in world.query_filtered::<&mut Camera, Or<(With<super::FieldCamera>, With<super::FieldOverlayCamera>)>>().iter_mut(world) {
        camera.is_active = true;
    }
    let white_fade = world
        .get_resource::<super::new_game::Session>()
        .and_then(|session| {
            session
                .field
                .events
                .world
                .fade
                .as_ref()
                .filter(|fade| fade.white)
                .map(|fade| fade.alpha(session.field.events.tick()) as u8 as f32 / 255.)
        })
        .unwrap_or(0.);
    super::materials::TitleOutput::update(
        &mut world.resource_mut::<Assets<super::materials::TitleOutput>>(),
        |brightness| {
            brightness.x = 1.;
            brightness.y = white_fade;
        },
    );
    world.resource_mut::<input::Controls>().clear();
    world
        .resource_mut::<super::field_view::Controls>()
        .clear_actions();
}

#[allow(clippy::too_many_arguments)] // One fixed visit joins retained session, device input and mixer acknowledgements.
fn advance(
    owner: Option<ResMut<Owner>>,
    mut controls: ResMut<input::Controls>,
    playback: Option<ResMut<super::battle_audio::Playback>>,
    mut session: Option<ResMut<super::new_game::Session>>,
    pause: Res<super::PresentationPause>,
) {
    let Some(mut owner) = owner else {
        controls.clear();
        return;
    };
    if pause.0 {
        return;
    }
    if matches!(owner.phase, Phase::Failed) {
        controls.clear();
        return;
    }
    if !owner.presenting() {
        controls.clear();
        return;
    }
    let tick = owner.input_tick;
    owner.input_tick = owner.input_tick.wrapping_add(1);
    let Phase::Scene(scene) = &mut owner.phase else {
        controls.loading_update(tick);
        return;
    };
    if scene.failure.is_some() || scene.completed.is_some() {
        controls.loading_update(tick);
        return;
    }
    let result = (|| -> Result<()> {
        let mut playback = playback.context("active battle lost its audio owner")?;
        if !scene.active {
            controls.loading_update(tick);
            let initialized = scene.dispatch == super::battle_entry::Dispatch::Actors;
            scene.dispatch.advance(&mut scene.transition);
            if initialized {
                // 40C8 constructs P0. 3EA4, the next
                // admitted visit, performs the first battle world update/draw.
                scene.active = true;
            }
            if let Some(sound) = scene.transition.advance(false) {
                ensure!(sound.resource == 0, "screen break requires a cue program");
                playback.menu_cue(sound.index)?;
            }
            session
                .as_mut()
                .context("battle lost its retained field")?
                .field
                .play_time
                .advance();
            return Ok(());
        }
        // 3EA4 advances the entry fade before its admitted camera/world visit.
        scene.dispatch.advance(&mut scene.transition);
        let actor = scene.actors.first().context("battle has no leader")?.actor;
        let target_step = controls.target_step(tick);
        let command = controls.command_input(&scene.settings, target_step);
        let confirm = controls.confirm();
        let controller = controls.consume(actor, &scene.settings, target_step);
        let battle_input = resonance_battle::BattleInput {
            controllers: vec![controller],
            voices_finished: playback.completed()?,
            ..Default::default()
        };
        let mut presentation = Presentation {
            hud: &mut scene.hud,
            audio: &mut playback,
            assets: &scene.audio,
            requested_music: &mut scene.requested_music,
            characters: &scene.characters,
            action_names: &scene.action_names,
            generation: scene.generation,
            diagnostics: &scene.diagnostics,
        };
        let frame = {
            let mut services = scene
                .candidate
                .as_mut()
                .context("battle result candidate was already consumed")?
                .services(&mut presentation);
            scene.lifecycle.step(
                &mut scene.core,
                lifecycle::Input {
                    battle: battle_input,
                    command,
                    confirm,
                },
                &mut services,
            )?
        };
        if scene.lifecycle.take_command_repeat_reset() {
            controls.reset_command_repeat();
        }
        for event in scene.lifecycle.take_command_events() {
            if let Some(cue) = command_cue(event) {
                playback.menu_cue(cue)?;
            }
            if let resonance_game::battle::command::Event::VoiceStreamsPaused(paused) = event {
                playback.pause_voice_streams(paused)?;
            }
            if let resonance_game::battle::command::Event::Confirmed { selected } = event {
                // Stage 4 owns row submenus. Keep the source row event and let
                // the session policy decide whether the missing route is fatal.
                scene.diagnostics.attempt::<()>(
                    "battle command submenu",
                    Err(anyhow::anyhow!(
                        "command row {selected} is not prepared in Stage 3"
                    )),
                )?;
            }
        }
        let camera = frame.camera.context("battle frame has no camera")?;
        playback.dispatch(&frame.cues, |point| {
            Some(resonance_battle::project_screen_x(camera, point))
        })?;
        // 6184 advances/draws BDA8/BBA0 after the current owner callback.
        if let Some(sound) = scene.transition.advance(false) {
            ensure!(sound.resource == 0, "screen break requires a cue program");
            playback.menu_cue(sound.index)?;
        }
        if let Some(outcome) = &frame.outcome {
            scene.completed = Some(
                scene
                    .candidate
                    .take()
                    .unwrap()
                    .finish(&scene.core, outcome)?,
            );
        }
        scene.frame = Some(frame);
        // The field's effects, VM and movement stay held. Save play time counts
        // this one presented battle visit on the same 60000/1001 cadence.
        session
            .as_mut()
            .context("battle lost its retained field")?
            .field
            .play_time
            .advance();
        Ok(())
    })();
    if let Err(error) = result {
        scene.failure = Some(format!("{error:#}"));
    }
}

struct Presentation<'a> {
    hud: &'a mut super::field_ui::BattleHud,
    audio: &'a mut super::battle_audio::Playback,
    assets: &'a super::battle_audio::Assets,
    requested_music: &'a mut Option<u16>,
    characters: &'a [u8],
    action_names: &'a BTreeMap<u16, String>,
    generation: u64,
    diagnostics: &'a resonance_content::diagnostics::Diagnostics,
}
impl results::Presentation for Presentation<'_> {
    fn after_world(&mut self, frame: &BattleFrame) -> Result<()> {
        self.diagnostics
            .attempt("battle HUD update", self.hud.advance(frame))?;
        self.diagnostics
            .attempt("battle combo display", self.hud.combat_cues(frame))?;
        for cue in &frame.cues {
            if let resonance_battle::Cue::Notice {
                actor,
                action,
                duration,
                kind,
            } = cue
            {
                let result = (|| {
                    let name = self
                        .action_names
                        .get(action)
                        .with_context(|| format!("action {action} has no prepared notice text"))?;
                    self.hud.notice_request(
                        actor.index(),
                        name,
                        *duration,
                        *kind,
                        frame,
                        self.characters,
                    )
                })();
                self.diagnostics
                    .attempt("battle technique notice", result)?;
            }
        }
        Ok(())
    }

    fn observations(&self) -> lifecycle::Observations {
        lifecycle::Observations {
            music_ready: self.requested_music.is_some_and(|track| {
                self.assets.music_ready(track) || !self.diagnostics.paranoid()
            }),
            // Candidate::new verified every selected performance and voice;
            // this adapter exists only after their submitted GPU draws finish.
            performance_ready: true,
            results_ready: self.hud.results_ready(),
            ..Default::default()
        }
    }
    fn request(
        &mut self,
        kind: lifecycle::RequestKind,
        selection: Option<&results::Selection>,
        results: Option<&results::Results>,
        battle: &mut Battle,
    ) -> Result<Vec<resonance_battle::Cue>> {
        use lifecycle::RequestKind::*;
        match kind {
            SelectVictory { .. }
            | ConstructRewards
            | RecoverTp
            | PerformVictory
            | AcceptVictory
            | RecordEscape
            | ResultCamera { .. } => {} // Applied by the game candidate before this callback.
            RequestMusic { track } => {
                if !self.assets.music_ready(track) {
                    self.diagnostics.report(
                        "battle result music",
                        anyhow::anyhow!(
                            "result music {track} was not prepared; continuing without it"
                        ),
                    )?;
                }
                *self.requested_music = Some(track);
            }
            PlayMusic { track, fade_ms } => {
                self.audio.music(Some(i16::try_from(track)?), fade_ms)?
            }
            StopMusic { fade_ms } => self.audio.music(None, fade_ms)?,
            UpdateResults { age, confirm } => {
                self.diagnostics.attempt(
                    "battle result display",
                    self.hud.update_results(
                        age,
                        results.context("result display has no reward state")?,
                        confirm,
                    ),
                )?;
                for cue in self.hud.take_result_sounds() {
                    self.audio.menu_cue(cue)?;
                }
            }
            VictoryBanner | LevelNotices | ExNotices | DefeatNotice | EscapeNotice => {
                self.diagnostics.attempt(
                    "battle overlay",
                    self.hud.overlay_request(
                        kind,
                        selection,
                        results,
                        battle,
                        self.characters,
                        self.generation,
                    ),
                )?;
            }
        }
        Ok(Vec::new())
    }
}

#[allow(clippy::too_many_arguments)] // Apply held poses after propagation without another animation clock.
fn render(
    mut commands: Commands,
    owner: Option<ResMut<Owner>>,
    mut globals: Query<&mut GlobalTransform>,
    mut transforms: Query<&mut Transform>,
    mut surfaces: ResMut<Assets<super::materials::TitleSurface>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(mut owner) = owner else {
        return;
    };
    let Owner { phase, entry, .. } = &mut *owner;
    let Phase::Scene(scene) = phase else {
        return;
    };
    if let Some(entry) = entry
        && let Err(error) = entry.render(
            &scene.transition,
            scene.active && scene.frame.as_ref().is_some_and(|frame| frame.update != 0),
            &mut commands,
            &mut meshes,
        )
    {
        scene.failure = Some(format!("{error:#}"));
    }
    if !scene.switched || scene.failure.is_some() {
        return;
    }
    let result = (|| -> Result<()> {
        let frame = scene
            .frame
            .as_ref()
            .context("active battle has no held frame")?;
        if scene.active {
            scene.diagnostics.attempt(
                "battle feedback",
                scene
                    .view
                    .apply_feedback(&mut commands, scene.hud.feedback()),
            )?;
        }
        scene.diagnostics.attempt(
            "battle rendering",
            scene.view.apply(
                frame,
                &mut commands,
                &mut globals,
                &mut transforms,
                &mut surfaces,
                &mut meshes,
            ),
        )?;
        scene.diagnostics.attempt(
            "battle HUD",
            scene
                .hud
                .render(frame, &scene.characters, &mut commands, &mut meshes),
        )?;
        // 63F8 is a callback after ordinary HUD (6EEE4). The UI owner
        // consumes the immutable game frame; a close visit passes None.
        scene.diagnostics.attempt(
            "battle command strip",
            scene.hud.render_commands(
                scene.lifecycle.command_frame().as_ref(),
                frame.hud_update,
                &mut commands,
                &mut meshes,
            ),
        )?;
        scene.diagnostics.attempt(
            "battle transition",
            scene.hud.render_transition(
                scene.core.transition().or_else(|| scene.transition.fade()),
                &mut commands,
                &mut meshes,
            ),
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        scene.failure = Some(format!("{error:#}"));
    }
}

/// Publish the candidate and complete its exact native operation in one exclusive
/// world visit. A cancelled/duplicate token cannot leave a partial party update.
fn commit(world: &mut World, request: &Request, completed: results::Completed) -> Result<()> {
    let outcome = completed.result;
    let mut session = world
        .get_resource_mut::<super::new_game::Session>()
        .context("battle return lost its retained field")?;
    completed.commit(&mut session.field.events.world, request)?;
    info!(
        "Battle request {} completed once with {:?}",
        request.id(),
        outcome
    );
    Ok(())
}

#[cfg(test)]
#[path = "battle/fatal_tests.rs"]
mod fatal_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Resource, Default)]
    struct Visits(u32);

    #[test]
    fn all_field_passes_can_share_a_gate_without_a_live_session() {
        let mut app = App::new();
        app.init_resource::<Visits>()
            .init_resource::<super::super::loading::Resident>()
            .add_systems(
                Update,
                (|mut visits: ResMut<Visits>| visits.0 += 1).run_if(field_running),
            );
        app.update();
        assert_eq!(app.world().resource::<Visits>().0, 1);
        app.world()
            .resource::<super::super::loading::Resident>()
            .battle
            .store(true, Ordering::Release);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<Visits>().0, 1);
        app.world()
            .resource::<super::super::loading::Resident>()
            .battle
            .store(false, Ordering::Release);
        app.update();
        assert_eq!(app.world().resource::<Visits>().0, 2);
    }

    #[test]
    fn command_events_use_the_source_9e38_menu_rows() {
        use resonance_game::battle::command::Event;

        assert_eq!(command_cue(Event::Navigated { selected: 1 }), Some(1));
        assert_eq!(command_cue(Event::Confirmed { selected: 0 }), Some(2));
        assert_eq!(command_cue(Event::Cancelled), Some(3));
        assert_eq!(command_cue(Event::Disabled { selected: 5 }), Some(4));
    }
}
