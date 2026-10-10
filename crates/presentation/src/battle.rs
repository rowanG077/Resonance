//! The battle scene borrows presentation while retaining the complete field owner.
pub(crate) mod appearance;
mod capture;
mod feedback;
mod input;
mod rumble;
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::{
        RenderTarget, ScalingMode,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
};
use resonance_battle::{ActorId, Battle, BattleFrame, Side};
use resonance_content::{menu_data::MenuData, prepared::Files, session::SessionData};
use resonance_events::{battle::Request, party::Party};
use resonance_game::battle::{encounter, lifecycle, results};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
};

// Positioned effects and voices use the same final frame camera as the world.
fn audio_camera(frame: &BattleFrame) -> Result<resonance_battle::CameraPose> {
    frame.camera.context("battle audio frame has no camera")
}

/// Field-owned inputs captured together before the preparation worker starts.
pub(super) struct Entry {
    pub files: Arc<Files>,
    pub party: Party,
    pub setup: resonance_events::battle::Setup,
    pub options: encounter::PrepareOptions,
    pub data: Arc<SessionData>,
    pub menus: Arc<MenuData>,
    pub gameplay_random: resonance_events::GameplayRandom,
}

impl Entry {
    /// Seed the private candidate. Only a completed battle publishes its draw.
    fn capture(
        session: &super::new_game::Session,
        setup: resonance_events::battle::Setup,
    ) -> Result<Self> {
        let events = session.events();
        let party = events.world.party.clone().context("battle has no party")?;
        let mut gameplay_random = events.world.gameplay_random;
        let options = encounter::PrepareOptions {
            random_seed: gameplay_random.next_u64(),
            map: if session.overworld.is_some() {
                3000
            } else {
                u16::try_from(session.assets.map_id)?
            },
            world_music: events.memory().read(0x50, symphonia_script::Width::S32)?,
            story: events.memory().read(0x40, symphonia_script::Width::S32)?,
            story3: events.world.event_flags.contains(&3),
            colette_state: events.memory().read(0x4c, symphonia_script::Width::S32)?,
            devils_arms_unlocked: events.world.event_flags.contains(&0x3fa),
            victory_story_flags: [27, 28].map(|id| events.world.event_flags.contains(&id)),
            overlimit_boost: party
                .new_game_plus
                .benefits
                .contains(&resonance_content::grade::Benefit::IncreasedTension),
        };
        Ok(Self {
            files: session.files(),
            party,
            setup,
            options,
            data: session.data.clone(),
            menus: events
                .resources()
                .menu_data
                .clone()
                .context("battle has no menu data")?,
            gameplay_random,
        })
    }
}

pub(super) struct Package {
    pub assets: Arc<encounter::Assets>,
    pub prepared: encounter::Prepared,
    pub audio: Arc<super::battle_audio::Assets>,
    pub party: Party,
    pub menus: Arc<MenuData>,
    pub data: Arc<SessionData>,
    pub catalogue: Arc<resonance_content::arte::Catalogue>,
    pub gameplay_random: resonance_events::GameplayRandom,
    pub entry_seed: u64,
}

#[derive(Resource)]
pub(super) struct Owner {
    request: Request,
    phase: Phase,
    input_tick: u32,
    publish_field: bool,
}
impl Owner {
    pub(super) fn failed(&self) -> bool {
        matches!(self.phase, Phase::Failed)
    }

    pub(super) fn presenting(&self) -> bool {
        matches!(&self.phase, Phase::Scene(scene) if scene.state == SceneState::Active
            && scene.menu_ready && scene.failure.is_none())
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
            publish_field: false,
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
    menu_backdrop: super::menu_backdrop::BattleBackdrop,
    view: super::battle_view::View,
    hud: super::field_ui::BattleHud,
    game_over: Option<super::field_ui::GameOverArt>,
    audio: Arc<super::battle_audio::Assets>,
    settings: resonance_content::menu_data::CustomizeSettings,
    core: Battle,
    models: resonance_battle::Models,
    feedback: feedback::Feedback,
    effects: resonance_battle::Effects,
    particles: Vec<resonance_battle::ParticleFrame>,
    lifecycle: lifecycle::Lifecycle,
    candidate: Option<results::Candidate>,
    characters: Vec<u8>,
    action_names: BTreeMap<resonance_battle::ActionKey, String>,
    music: u16,
    frame: Option<BattleFrame>,
    camera: Option<Entity>,
    target: RenderTarget,
    state: SceneState,
    menu_ready: bool,
    entry_remaining: u8,
    entry_color: [u8; 3],
    failure: Option<String>,
    completed: Option<results::Completed>,
    entry_seed: u64,
    preparing_since: std::time::Instant,
    diagnostics: resonance_content::diagnostics::Diagnostics,
}

const ENTRY_FADE_TICKS: u8 = 15;

impl Scene {
    fn entry_alpha(&self) -> u8 {
        (u16::from(self.entry_remaining) * 255 / u16::from(ENTRY_FADE_TICKS)) as u8
    }

    fn feedback_paused(&self) -> bool {
        self.frame.as_ref().is_none_or(|frame| frame.clock.paused())
    }
}

/// Each state waits for a real render submission, then simulation owns the scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SceneState {
    Warming,
    Activating,
    Active,
}

/// Shared UI cursor slots, independent of Party and save data.
#[derive(Resource, Default)]
pub(super) struct MenuMemory {
    pub(super) inventory: resonance_game::battle::command::InventoryMemory,
    /// Reset whenever the main field menu opens.
    pub(super) character: usize,
    /// Shared Unison selection, clamped when the page opens.
    pub(super) unison_character: usize,
}

/// Used by every field simulation and rendering pass, including standalone
/// captures which do not install the live scene owner.
pub(super) fn field_running(
    session: Option<Res<super::new_game::Session>>,
    resident: Option<Res<super::loading::Resident>>,
) -> bool {
    !session.is_some_and(|session| session.events().battle_pending())
        && !resident.is_some_and(|resident| resident.battle.load(Ordering::Acquire))
}

/// The request-producing update still publishes its final field pose and UI.
/// Simulation keeps using field_running; this gate admits only publication.
pub(super) fn field_presenting(
    owner: Option<Res<Owner>>,
    resident: Option<Res<super::loading::Resident>>,
) -> bool {
    owner.as_ref().is_some_and(|owner| owner.publish_field)
        || !resident.is_some_and(|resident| resident.battle.load(Ordering::Acquire))
}

fn publish_field(owner: Option<ResMut<Owner>>) {
    if let Some(mut owner) = owner {
        owner.publish_field = false;
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PublishBattle;

fn configure_publication(app: &mut App) {
    app.configure_sets(
        PostUpdate,
        PublishBattle
            .after(super::sparse_animation::affine::propagate)
            .before(bevy::asset::AssetEventSystems)
            // Layer::show queues Visibility changes. CheckVisibility consumes
            // InheritedVisibility, so these must reach propagation in this
            // same draw, including the one retained by the menu capture.
            .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate)
            .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds)
            .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta)
            .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
    );
}

pub(super) fn install(app: &mut App) {
    configure_publication(app);
    app.init_resource::<input::Controls>()
        .init_resource::<MenuMemory>()
        .init_resource::<rumble::State>()
        .add_systems(
            PreUpdate,
            input::gather
                .after(bevy::input::InputSystems)
                .run_if(not(resource_exists::<super::saves::ScenarioInput>)),
        )
        .add_systems(
            FixedPreUpdate,
            input::gather
                .after(bevy::input::InputSystems)
                .run_if(resource_exists::<super::saves::ScenarioInput>),
        )
        .add_systems(Update, manage.after(super::field_view::FieldPreparation))
        .add_systems(Update, prepare.after(manage))
        .add_systems(PostUpdate, publish_field.after(super::field_audit::check))
        .add_systems(
            FixedUpdate,
            advance
                .after(super::field_view::advance_live)
                .after(super::timing::advance_clock),
        )
        .add_systems(PostUpdate, render.in_set(PublishBattle))
        .add_systems(PostUpdate, rumble::present.after(PublishBattle));
    super::battle_view::install(app);
}

/// Move the exact request only after the worker is owned. Neither preparation
/// failure nor cancellation retires the retained event VM or its party state.
fn manage(world: &mut World) {
    retry_audio(world);
    if !world.contains_resource::<Owner>() {
        if world.contains_resource::<super::battle_audio::Playback>() {
            return;
        }
        let Some(request) = world
            .get_resource::<super::new_game::Session>()
            .and_then(|session| session.events().world.battle_request.clone())
        else {
            return;
        };
        if !request.is_pending() {
            return;
        }
        let _audio_owner = debug_span!(
            "battle_audio_owner",
            request = request.id(),
            phase = "entry"
        )
        .entered();
        let result = Entry::capture(world.resource::<super::new_game::Session>(), request.setup);
        let result = result.and_then(|entry| {
            super::loading::BattlePending::battle(
                world.resource::<super::RunOptions>().assets.clone(),
                entry,
                world.resource::<super::loading::Resident>(),
            )
        });
        let phase = match result {
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
        if let Some(mut controls) = world.get_resource_mut::<super::overworld::Controls>() {
            *controls = Default::default();
        }
        world
            .resource_mut::<super::new_game::Session>()
            .events_mut()
            .world
            .battle_request
            .take();
        world.insert_resource(Owner {
            request,
            phase,
            input_tick: 0,
            publish_field: true,
        });
    }
    let Some(mut owner) = world.remove_resource::<Owner>() else {
        return;
    };
    let _audio_owner = debug_span!(
        "battle_audio_owner",
        request = owner.request.id(),
        phase = "manage"
    )
    .entered();
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
                    publish_field: false,
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
        match result.and_then(|package| scene(world, package)) {
            Ok(mut scene) => {
                scene.entry_color = [if owner.request.transition_white == Some(true) {
                    255
                } else {
                    0
                }; 3];
                if let Err(error) = begin_audio(world, &scene) {
                    scene.failure = Some(format!("{error:#}"));
                }
                owner.phase = Phase::Scene(Box::new(scene));
            }
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

fn begin_audio(world: &mut World, scene: &Scene) -> Result<()> {
    ensure!(
        !world.contains_resource::<super::battle_audio::Playback>(),
        "battle audio already owned"
    );
    let mut control = world
        .get_resource_mut::<super::field_audio::Control>()
        .context("battle needs the retained audio mixer")?;
    let settings = &scene.settings;
    let playback = super::battle_audio::Playback::begin(
        &mut control,
        scene.audio.clone(),
        super::battle_audio::Settings {
            music: settings.volumes.music,
            effects: settings.volumes.effects,
            battle_effects: settings.volumes.battle_effects,
            voice: if settings.battle_voiceover {
                settings.volumes.battle_voice
            } else {
                0
            },
            stereo: settings.stereo,
        },
        Some(i16::try_from(scene.music)?),
    )?;
    world.insert_resource(playback);
    Ok(())
}

fn retry_audio(world: &mut World) {
    let Some(mut playback) = world.remove_resource::<super::battle_audio::Playback>() else {
        return;
    };
    let result = world
        .get_resource_mut::<super::field_audio::Control>()
        .map_or(Ok(true), |mut control| playback.retry(&mut control));
    match result {
        Ok(true) => {}
        Ok(false) => {
            world.insert_resource(playback);
        }
        Err(error) => {
            world.insert_resource(playback);
            error!("Battle audio handoff failed: {error:#}");
            world.write_message(AppExit::error());
        }
    }
}

/// Hidden formations use a neutral label, independent of Monster Book knowledge.
fn enemy_hud(
    assets: &encounter::Inputs,
    prepared: &encounter::Prepared,
) -> Result<Vec<super::field_ui::BattleEnemyHud>> {
    let mut groups = Vec::new();
    for resource in &assets.enemies.resources {
        let enemy = &resource.monster;
        let name = if resource.hidden_name {
            "Unknown".into()
        } else {
            enemy.name.clone()
        };
        // Enemy portraits share the effect bank's icon atlas.
        let icon = assets
            .enemy_effects
            .get(&enemy.id)
            .and_then(|bank| bank.art.as_ref())
            .and_then(|art| art.textures.get(&14))
            .and_then(|textures| textures.first())
            .and_then(|texture| texture.images.first())
            .map(|icon| {
                let checked = (|| {
                    icon.validate()?;
                    ensure!(
                        icon.width >= 32 && icon.height >= 32,
                        "enemy HUD icon is too small"
                    );
                    assets.files.read(&icon.path)?;
                    Ok(icon.clone())
                })();
                assets
                    .files
                    .diagnostics()
                    .attempt("battle enemy icon", checked)
            })
            .transpose()?
            .flatten();
        groups.push((name, icon));
    }
    let party_count = prepared.characters.len();
    ensure!(
        prepared.core.actors().len() == party_count + assets.enemies.spawns.len(),
        "enemy HUD roster differs from prepared battle"
    );
    assets
        .enemies
        .spawns
        .iter()
        .enumerate()
        .map(|(index, spawn)| {
            let (name, icon) = groups
                .get(spawn.resource)
                .context("enemy HUD group is not prepared")?;
            Ok(super::field_ui::BattleEnemyHud {
                actor: party_count + index,
                group: u8::try_from(spawn.resource)?,
                name: name.clone(),
                icon: icon.clone(),
            })
        })
        .collect()
}

fn action_names(
    battle: &Battle,
    actors: &[(ActorId, u8)],
    menus: &MenuData,
) -> BTreeMap<resonance_battle::ActionKey, String> {
    actors
        .iter()
        .flat_map(|(actor, _)| battle.prepared_techniques(*actor))
        .filter_map(|prepared| {
            // Missing optional text is reported if this action emits a notice.
            menus
                .technique_text(prepared.catalogue)
                .ok()
                .map(|text| (prepared.action, text.name.clone()))
        })
        .collect()
}

fn scene(world: &mut World, package: Package) -> Result<Scene> {
    let path = world
        .resource::<super::new_game::Session>()
        .assets
        .toon_ramp
        .clone();
    let toon = world
        .resource::<AssetServer>()
        .load_builder()
        .with_settings(|settings: &mut bevy::image::ImageLoaderSettings| {
            settings.is_srgb = false;
            settings.sampler = bevy::image::ImageSampler::linear();
        })
        .load(path);
    let target = world
        .query_filtered::<&RenderTarget, With<super::FieldCamera>>()
        .single(world)?
        .clone();
    construct_scene(world, package, toon, target)
}

fn construct_scene(
    world: &mut World,
    package: Package,
    toon: Handle<Image>,
    target: RenderTarget,
) -> Result<Scene> {
    let diagnostics = diagnostics(world);
    let Package {
        assets,
        prepared,
        audio,
        party,
        menus,
        data,
        catalogue,
        gameplay_random,
        entry_seed,
    } = package;
    let enemies = enemy_hud(&assets, &prepared)?;
    let action_names = action_names(&prepared.core, &prepared.results.actors, &menus);
    *world
        .resource::<super::loading::Resident>()
        .files
        .write()
        .unwrap() = Some(Arc::new(assets.files.clone()));
    let server = world.resource::<AssetServer>().clone();
    let (mut hud, game_over) = world.resource_scope(
        |world, mut materials: Mut<Assets<super::field_ui::Surface>>| {
            let mut images = world.resource_mut::<Assets<Image>>();
            let hud = super::field_ui::BattleHud::load(
                assets.ui.clone(),
                menus.clone(),
                data.clone(),
                |path| Ok(assets.files.read(path)?.to_vec()),
                &enemies,
                &server,
                &mut materials,
                &mut images,
                &diagnostics,
            )?;
            let game_over = assets
                .game_over
                .as_ref()
                .map(|art| super::field_ui::GameOverArt::load(art, &mut materials, &mut images))
                .transpose()?;
            Ok::<_, anyhow::Error>((hud, game_over))
        },
    )?;
    let view = super::battle_view::View::load_with_diagnostics(
        assets.stage.clone(),
        prepared.models,
        toon,
        prepared.effects,
        prepared.trails,
        &server,
        diagnostics.clone(),
    )?;
    let settings = party.settings.preferences.clone();
    hud.settings(settings.clone());
    let candidate = results::Candidate::new(
        prepared.results,
        prepared.core.actors(),
        party,
        gameplay_random,
        data,
        menus,
        catalogue,
    )?;
    let core = prepared.core;
    let mut lifecycle = prepared.lifecycle;
    let memory = world.resource::<MenuMemory>();
    lifecycle.restore_menu_memory(resonance_game::battle::command::Memory {
        inventory: memory.inventory,
        unison_character: memory.unison_character,
        tech_character: memory.character,
    });
    Ok(Scene {
        menu_backdrop: super::menu_backdrop::BattleBackdrop::new(world)?,
        view,
        hud,
        game_over,
        audio,
        settings,
        core,
        models: prepared.model_player,
        feedback: feedback::Feedback::new(prepared.feedback, !entry_seed),
        effects: resonance_battle::Effects::new(
            prepared.effect_banks,
            Some(prepared.poison_effect),
            !entry_seed,
            diagnostics.clone(),
        )?,
        particles: Vec::new(),
        lifecycle,
        candidate: Some(candidate),
        characters: prepared.characters,
        action_names,
        music: prepared.music,
        frame: None,
        camera: None,
        target,
        state: SceneState::Warming,
        menu_ready: true,
        entry_remaining: ENTRY_FADE_TICKS,
        entry_color: [0; 3],
        failure: None,
        completed: None,
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
) {
    let Some(mut owner) = owner else {
        return;
    };
    let Owner { phase, .. } = &mut *owner;
    let Phase::Scene(scene) = phase else {
        return;
    };
    if scene.state == SceneState::Active || scene.failure.is_some() {
        return;
    }
    let result = (|| -> Result<()> {
        ensure!(
            scene.preparing_since.elapsed().as_secs() <= 120,
            "battle GPU preparation timed out"
        );
        scene.hud.prepare(&mut commands, &mut assets.meshes);
        if let Some(art) = &mut scene.game_over {
            art.prepare(&mut commands, &mut assets.meshes);
        }
        let draws: Vec<_> = scene
            .hud
            .entities()
            .chain(scene.game_over.iter().flat_map(|art| art.entities()))
            .chain(std::iter::once(scene.menu_backdrop.entity()))
            .collect();
        if !scene.view.prepare(
            &mut commands,
            &server,
            &mut assets,
            &entities,
            &warmup,
            Some(draws.as_slice()),
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
        if scene.state == SceneState::Warming {
            super::materials::TitleOutput::update(&mut outputs, |brightness| {
                brightness.x = 1.;
                brightness.y = 0.;
            });
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
                .chain(std::iter::once(scene.menu_backdrop.entity()))
            {
                commands.entity(entity).insert((
                    RenderLayers::layer(super::battle_view::LAYER),
                    Visibility::Hidden,
                ));
            }
            for mut camera in &mut cameras {
                camera.is_active = false;
            }
            // Keep startup feedback queued for the first visible update.
            let mut frame = scene.core.snapshot();
            scene
                .models
                .advance(&mut frame, resonance_battle::BattleClock::Held, false)?;
            scene.frame = Some(frame);
            scene.state = SceneState::Activating;
            return Ok(());
        }
        if !scene.view.finish_activation()? {
            return Ok(());
        }
        scene.state = SceneState::Active;
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

fn dispose(world: &mut World, mut owner: Owner, audio_return: Option<bool>) {
    if let Phase::Scene(scene) = &mut owner.phase {
        let memory = scene.lifecycle.take_menu_memory();
        let mut retained = world.resource_mut::<MenuMemory>();
        retained.inventory = memory.inventory;
        retained.character = memory.tech_character;
        retained.unison_character = memory.unison_character;
    }
    let _audio_owner = debug_span!(
        "battle_audio_owner",
        request = owner.request.id(),
        phase = "dispose"
    )
    .entered();
    if let Some(resume) = audio_return
        && let Some(mut playback) = world.remove_resource::<super::battle_audio::Playback>()
    {
        let result = world
            .get_resource_mut::<super::field_audio::Control>()
            .map_or(Ok(true), |mut control| {
                playback.finish(&mut control, resume)
            });
        if !matches!(result, Ok(true)) {
            world.insert_resource(playback);
        }
        if let Err(error) = result {
            error!("Battle audio return failed: {error:#}");
            world.write_message(AppExit::error());
        }
    }
    if let Phase::Scene(scene) = owner.phase {
        let Scene {
            view,
            hud,
            camera,
            game_over,
            menu_backdrop,
            ..
        } = *scene;
        view.despawn(&mut world.commands());
        hud.despawn(world);
        world.despawn(menu_backdrop.entity());
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
                .events()
                .world
                .fade
                .as_ref()
                .filter(|fade| fade.white)
                .map(|fade| fade.alpha(session.events().tick()) as u8 as f32 / 255.)
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
    if let Some(mut controls) = world.get_resource_mut::<super::overworld::Controls>() {
        *controls = Default::default();
    }
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
    scenario: Option<ResMut<super::saves::ScenarioInput>>,
) {
    let Some(mut owner) = owner else {
        controls.clear();
        return;
    };
    if matches!(owner.phase, Phase::Failed) {
        controls.clear();
        return;
    }
    if !owner.presenting() {
        controls.discard();
        return;
    }
    if matches!(&owner.phase, Phase::Scene(scene) if scene.menu_backdrop.awaiting_draw()) {
        // Publish the outgoing strip before advancing the shared menu. Multiple fixed
        // updates must not erase the frame waiting to be captured.
        return;
    }
    let request = owner.request.id();
    let tick = owner.input_tick;
    owner.input_tick = owner.input_tick.wrapping_add(1);
    let Phase::Scene(scene) = &mut owner.phase else {
        controls.discard();
        return;
    };
    if scene.failure.is_some() || scene.completed.is_some() {
        controls.discard();
        return;
    }
    let _audio_owner = debug_span!(
        "battle_audio_owner",
        request,
        input_tick = tick,
        update_before = scene.frame.as_ref().map(|frame| frame.update),
        g_before = scene.core.ledger().elapsed_ticks,
        c_before = scene.core.ledger().combat_ticks
    )
    .entered();
    let result = (|| -> Result<()> {
        let mut playback = playback.context("active battle lost its audio owner")?;
        let input = controls.sample(&scene.settings);
        if let Some(mut scenario) = scenario {
            scenario.acknowledge_input();
        }
        let session = session
            .as_mut()
            .context("battle lost its retained session")?;
        match &mut session.overworld {
            Some(world) => world.session.play_time.advance(),
            None => session.field.play_time.advance(),
        }
        if scene.entry_remaining > 0 {
            scene.entry_remaining -= 1;
            return Ok(());
        }
        let command = input.command_input(
            scene.lifecycle.command_controller(),
            scene.core.actors(),
            scene.lifecycle.command_input_kind(),
        );
        let battle_input = resonance_battle::BattleInput {
            controllers: input.actor_inputs(scene.core.actor_ids().map(|actor| {
                (
                    actor,
                    &scene.core.actors()[actor.index()],
                    scene.core.activity(actor),
                )
            })),
            ..Default::default()
        };
        let mut presentation = Presentation {
            hud: &mut scene.hud,
            audio: &mut playback,
            action_names: &scene.action_names,
            feedback: &scene.feedback.resources,
            diagnostics: &scene.diagnostics,
        };
        let mut frame = {
            let candidate = scene
                .candidate
                .as_mut()
                .context("battle result candidate was already consumed")?;
            scene.lifecycle.step(
                &mut scene.core,
                lifecycle::Input {
                    battle: battle_input,
                    command,
                    confirm: input.confirm(),
                    cook: input.cook(),
                },
                candidate,
                &mut presentation,
            )?
        };
        let clock = frame.clock;
        let paused = clock.paused();
        let settle = clock == resonance_battle::BattleClock::Running
            && scene.core.phase() == resonance_battle::BattlePhase::Combat;
        scene.feedback.resolve(&mut frame);
        scene.models.advance(&mut frame, clock, settle)?;
        let effects = scene.effects.advance(&frame, paused)?;
        frame.cues.extend(effects);
        scene.particles = scene.effects.frames();
        let _audio_frame = debug_span!(
            "battle_audio_frame",
            update = frame.update,
            g = scene.core.ledger().elapsed_ticks,
            c = scene.core.ledger().combat_ticks
        )
        .entered();
        for event in scene.lifecycle.take_command_events() {
            use resonance_game::battle::command::Event;
            match event {
                Event::Cue(cue) => playback.menu_cue(cue)?,
                Event::CaptureBackdrop => scene.menu_backdrop.request_capture(),
                Event::VoiceStreamsPaused(paused) => playback.pause_voice_streams(paused)?,
            }
        }
        let camera = audio_camera(&frame)?;
        playback.dispatch(&frame.cues, |point| {
            Some(resonance_battle::project_screen_x(camera, point))
        })?;
        if let Some(outcome) = &frame.outcome {
            scene.completed = Some(
                scene
                    .candidate
                    .take()
                    .unwrap()
                    .finish(&scene.core, outcome)?,
            );
        }
        // Feedback changes only the published view, after simulation and audio positioning.
        scene.view.advance_trails(&frame, paused)?;
        scene.feedback.advance(&mut frame, paused);
        scene.frame = Some(frame);
        Ok(())
    })();
    if let Err(error) = result {
        scene.failure = Some(format!("{error:#}"));
    }
}

struct Presentation<'a> {
    hud: &'a mut super::field_ui::BattleHud,
    audio: &'a mut super::battle_audio::Playback,
    action_names: &'a BTreeMap<resonance_battle::ActionKey, String>,
    feedback: &'a resonance_game::battle::feedback::Feedback,
    diagnostics: &'a resonance_content::diagnostics::Diagnostics,
}
fn update_hud(
    hud: &mut super::field_ui::BattleHud,
    frame: &BattleFrame,
    action_names: &BTreeMap<resonance_battle::ActionKey, String>,
    feedback: &resonance_game::battle::feedback::Feedback,
    diagnostics: &resonance_content::diagnostics::Diagnostics,
    paused: bool,
) -> Result<()> {
    diagnostics.attempt("battle HUD update", hud.advance(frame, paused))?;
    for cue in &frame.cues {
        if let resonance_battle::Cue::ItemNotice {
            actor,
            item,
            duration,
        } = cue
        {
            diagnostics.attempt(
                "battle item notice",
                hud.item_notice_request(actor.index(), *item, *duration, frame),
            )?;
        }
        if let Some((actor, text)) = feedback.notice(cue) {
            diagnostics.attempt(
                "battle recovery notice",
                hud.notice_request(actor.index(), text, 90, frame),
            )?;
        }
        if let resonance_battle::Cue::Notice {
            actor,
            action,
            duration,
        } = cue
        {
            let result = (|| {
                let name = action_names
                    .get(action)
                    .with_context(|| format!("action {action} has no prepared notice text"))?;
                hud.notice_request(actor.index(), name, *duration, frame)
            })();
            diagnostics.attempt("battle technique notice", result)?;
        }
    }
    Ok(())
}
impl results::Presentation for Presentation<'_> {
    fn present(&mut self, frame: &BattleFrame) -> Result<()> {
        update_hud(
            self.hud,
            frame,
            self.action_names,
            self.feedback,
            self.diagnostics,
            frame.clock.paused(),
        )
    }

    fn results_on_last_page(&self) -> bool {
        self.hud.results_on_last_page()
    }
    fn request(
        &mut self,
        kind: lifecycle::Request,
        results: Option<&results::Results>,
        _battle: &Battle,
    ) -> Result<()> {
        use lifecycle::Request::*;
        match kind {
            PlayMusic { track, fade_ms } => {
                self.audio.music(Some(i16::try_from(track)?), fade_ms)?
            }
            SynchronizeResults => self.hud.synchronize_results(
                results.context("result display has no reward state")?,
                self.diagnostics,
            )?,
            NextResultPage => self.hud.next_result_page(),
            VictoryBanner | DefeatNotice | EscapeNotice => {
                self.diagnostics
                    .attempt("battle overlay", self.hud.overlay_request(kind))?;
            }
        }
        for cue in self.hud.take_result_sounds() {
            self.audio.menu_cue(cue)?;
        }
        Ok(())
    }
}

/// The page's drawing and command ownership share one success/failure boundary.
fn render_menu(
    scene: &mut Scene,
    tick: u32,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &Assets<Image>,
    server: &AssetServer,
) -> Result<bool> {
    use resonance_game::battle::command::View;
    scene.menu_ready = true;
    let Some(command) = scene.lifecycle.command_frame() else {
        return Ok(false);
    };
    if !matches!(
        command.view,
        View::Inventory(_)
            | View::Strategy(_)
            | View::Tech(_)
            | View::Unison(_)
            | View::Equipment(_)
    ) {
        return Ok(false);
    }
    scene.hud.hide_combat_for_menu(commands);
    let result = (|| {
        let candidate = scene
            .candidate
            .as_ref()
            .context("battle menu lost its result candidate")?;
        match &command.view {
            View::Inventory(list) => scene.hud.render_items(Some(list), tick, commands, meshes),
            View::Strategy(state) => {
                scene
                    .hud
                    .render_strategy(candidate.strategy_page(state), tick, commands, meshes)
            }
            View::Tech(state) => scene.hud.render_tech(
                candidate.battle_tech_view(state, &command.connected, &scene.core),
                tick,
                commands,
                meshes,
            ),
            View::Unison(state) => {
                scene
                    .hud
                    .render_unison(candidate.unison_page(state), tick, commands, meshes)
            }
            View::Equipment(state) => scene.hud.render_equipment(
                candidate.equipment_page(state, scene.lifecycle.equipment_character()),
                tick,
                commands,
                meshes,
            ),
            _ => unreachable!(),
        }
    })()
    .and_then(|()| scene.hud.menu_images_ready(images, server));
    if let Ok(ready) = result {
        scene.menu_ready = ready;
        return Ok(true);
    }
    scene.hud.clear_menu(commands);
    scene.diagnostics.attempt("battle menu page", result)?;
    scene.lifecycle.recover_command_page(
        &mut scene.core,
        scene
            .candidate
            .as_mut()
            .context("battle menu lost its result candidate")?,
    )?;
    Ok(false)
}

fn render_command_strip(
    scene: &mut Scene,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) -> Result<()> {
    let pending = scene.core.pending_item();
    let user_name = pending.and_then(|release| {
        let character = *scene.characters.get(release.user.index())?;
        scene.candidate.as_ref()?.persistent_party().members[usize::from(character - 1)]
            .name
            .as_deref()
    });
    let result = scene.hud.render_commands(
        scene.lifecycle.command_frame().as_ref(),
        pending,
        &scene.characters,
        user_name,
        commands,
        meshes,
    );
    if result.is_err() {
        scene.hud.hide_commands(commands);
        scene.diagnostics.attempt("battle command strip", result)?;
        scene.lifecycle.recover_command_page(
            &mut scene.core,
            scene
                .candidate
                .as_mut()
                .context("battle command lost its result candidate")?,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Apply held poses after propagation without another animation clock.
fn render(
    mut commands: Commands,
    owner: Option<ResMut<Owner>>,
    mut globals: Query<&mut GlobalTransform>,
    mut transforms: Query<&mut Transform>,
    mut surfaces: ResMut<Assets<super::materials::TitleSurface>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut menu_materials: ResMut<Assets<super::menu_backdrop::Material>>,
    images: Res<Assets<Image>>,
    server: Res<AssetServer>,
) {
    let Some(mut owner) = owner else {
        return;
    };
    // `frame.update` is intentionally frozen while a shared menu owns the
    // command visit. Keep menu animation on the presentation input clock so
    // the Tech cursor and popup continue to animate during that pause.
    let Owner {
        phase, input_tick, ..
    } = &mut *owner;
    let Phase::Scene(scene) = phase else {
        return;
    };
    if scene.state == SceneState::Warming || scene.failure.is_some() {
        return;
    }
    let result = (|| -> Result<()> {
        let frame = scene
            .frame
            .as_ref()
            .context("active battle has no held frame")?;
        scene.diagnostics.attempt(
            "battle rendering",
            scene.view.apply(
                (frame, &scene.particles),
                &mut commands,
                &mut globals,
                &mut transforms,
                &mut surfaces,
                &mut meshes,
            ),
        )?;
        let shared_menu = render_menu(
            scene,
            *input_tick,
            &mut commands,
            &mut meshes,
            &images,
            &server,
        )?;
        let frame = scene
            .frame
            .as_ref()
            .context("active battle has no held frame")?;
        let command = scene.lifecycle.command_frame();
        scene.diagnostics.attempt(
            "battle menu backdrop",
            scene.menu_backdrop.show(
                shared_menu,
                scene.camera,
                &mut commands,
                &mut menu_materials,
            ),
        )?;
        if !shared_menu {
            let queued_techniques = frame
                .actors
                .iter()
                .enumerate()
                .filter(|(_, actor)| actor.side == Side::Party)
                .map(|(index, _)| {
                    ActorId::from_index(index)
                        .ok()
                        .and_then(|actor| scene.core.pending_technique(actor))
                        .is_some()
                })
                .collect::<Vec<_>>();
            let names = scene
                .characters
                .iter()
                .map(|&character| {
                    scene.candidate.as_ref().and_then(|candidate| {
                        candidate.persistent_party().members[usize::from(character - 1)]
                            .name
                            .as_deref()
                    })
                })
                .collect::<Vec<_>>();
            scene.diagnostics.attempt(
                "battle HUD",
                scene.hud.render(
                    frame,
                    super::field_ui::PartyHudInput {
                        characters: &scene.characters,
                        queued_techniques: &queued_techniques,
                        names: &names,
                    },
                    command.as_ref(),
                    &mut commands,
                    &mut meshes,
                ),
            )?;
        }
        // Draw the command strip after the ordinary HUD, using the immutable game frame. A
        // closed menu supplies None.
        if !shared_menu {
            render_command_strip(scene, &mut commands, &mut meshes)?;
        }
        let entry_fade = (scene.entry_remaining > 0).then_some(resonance_battle::TransitionFrame {
            color: scene.entry_color,
            alpha: scene.entry_alpha(),
        });
        scene.diagnostics.attempt(
            "battle transition",
            scene.hud.render_transition(
                entry_fade.as_ref().or(scene.core.transition()),
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
    completed.commit(&mut session.events_mut().world, request)?;
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
pub(crate) mod tests {
    use super::*;

    pub(crate) fn regal_encounter() -> Result<Package> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            Into::into,
        );
        let mut cache = Default::default();
        let files = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
            false
        })?;
        let menus: MenuData = files.json("game/menu-data.json")?;
        let mut data: SessionData = files.json("game/session-data.json")?;
        data.rules = Some(Arc::new(menus.clone()));
        let mut party = Party::new(&data, Default::default())?;
        party.formation = vec![8];
        party.field_leader = 8;
        party.settings.battle_controls = [2; 4];
        let member = &mut party.members[7];
        member.techniques.clear();
        member.disabled_techniques.clear();
        member.shortcuts = [0; 4];
        member.assist_shortcuts = [None; 2];
        member.technique_uses.remove(&201);
        party.validate(&data)?;
        let assets = encounter::Assets::load(
            &root,
            &files,
            &menus,
            &data,
            &party,
            resonance_events::battle::Setup {
                route: [0; 5],
                encounter: resonance_events::battle::Encounter::Formation(2),
                arena: 13,
                defeat: resonance_events::battle::DefeatPolicy::GameOver,
                music: None,
            },
            &mut cache,
            || false,
        )?;
        let audio = crate::battle_audio::Assets::load(
            &assets.files,
            assets.audio.as_ref(),
            &mut Default::default(),
        )?;
        let prepared = assets.prepare(
            &menus,
            encounter::PrepareOptions {
                devils_arms_unlocked: false,
                victory_story_flags: [false; 2],
                random_seed: 1,
                map: 332,
                world_music: 0,
                story: 2500,
                story3: false,
                colette_state: 0,
                overlimit_boost: false,
            },
            |sound| audio.bind(sound),
        )?;
        let catalogue = assets.catalogue.clone();
        Ok(Package {
            assets: Arc::new(assets),
            prepared,
            audio,
            party,
            menus: Arc::new(menus),
            data: Arc::new(data),
            catalogue,
            gameplay_random: resonance_events::GameplayRandom::new(1),
            entry_seed: 1,
        })
    }

    #[test]
    #[ignore = "requires current Regal/Mirage encounter and HUD assets; CPU only"]
    fn newly_learned_technique_reaches_the_live_notice_consumer() -> Result<()> {
        use resonance_battle::{ActionRequest, Activity, BattleInput, Cue};
        use resonance_content::diagnostics::Diagnostics;
        let Package {
            assets,
            prepared,
            menus,
            data,
            ..
        } = regal_encounter()?;
        let mut menus = (*menus).clone();
        for key in ["strategy_title", "unison_title"] {
            menus.presentation.labels.remove(key);
        }
        menus.presentation.strategy.as_mut().unwrap().groups[0][0]
            .as_mut()
            .unwrap()
            .details
            .push('\u{e000}');
        menus.validate_gameplay()?;
        // Construct the same notice lookup before learning occurs.
        let names = action_names(&prepared.core, &prepared.results.actors, &menus);
        let owner = prepared.results.actors[0].0;
        let mut battle = prepared.core;
        assert_eq!(battle.technique_is_current(owner, 201), Some(false));
        for _ in 0..300 {
            if battle.phase() != resonance_battle::BattlePhase::Entry
                && battle.activity(owner) == Activity::Idle
            {
                break;
            }
            battle.step(BattleInput::default())?;
        }
        ensure!(
            battle.phase() != resonance_battle::BattlePhase::Entry
                && battle.activity(owner) == Activity::Idle,
            "Regal did not finish entry"
        );
        let action = battle.record_technique_acquisition(owner, 201)?;
        assert_eq!(battle.technique_is_current(owner, 201), Some(true));
        let target = battle
            .target(owner)
            .context("learned technique has no target")?;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        let server = app.world().resource::<AssetServer>().clone();
        let mut materials = Assets::default();
        let mut hud = crate::field_ui::BattleHud::load(
            assets.ui.clone(),
            Arc::new(menus),
            data,
            |path| Ok(assets.files.read(path)?.to_vec()),
            &[],
            &server,
            &mut materials,
            &mut app.world_mut().resource_mut::<Assets<Image>>(),
            assets.files.diagnostics(),
        )?;
        let diagnostics = Diagnostics::new(true);
        for tick in 0..180 {
            let frame = battle.step(BattleInput {
                actions: (tick == 0)
                    .then_some(ActionRequest {
                        actor: owner,
                        action,
                        target,
                    })
                    .into_iter()
                    .collect(),
                ..Default::default()
            })?;
            update_hud(
                &mut hud,
                &frame,
                &names,
                &prepared.feedback,
                &diagnostics,
                battle.is_paused(),
            )?;
            if frame.cues.iter().any(|cue| {
                matches!(cue,
                Cue::Notice { actor, action: used, .. } if *actor == owner && *used == action)
            }) {
                assert_eq!(battle.technique_uses(owner, 201), Some(1));
                assert_eq!(hud.party_notice_text(), Some("Mirage"));
                assert!(!diagnostics.has_errors());
                return Ok(());
            }
        }
        anyhow::bail!("learned Mirage use emitted no notice")
    }
    #[derive(Resource, Default)]
    struct Visits(u32);

    #[test]
    fn command_visibility_reaches_the_first_composed_capture_visit() {
        use bevy::camera::{primitives::Frustum, visibility::VisibilityPlugin};

        #[derive(Resource, Default)]
        struct Publication(Vec<(Entity, Visibility)>);

        fn publish(mut commands: Commands, mut publication: ResMut<Publication>) {
            for (entity, visibility) in publication.0.drain(..) {
                commands.entity(entity).insert(visibility);
            }
        }

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::transform::TransformPlugin,
            VisibilityPlugin,
        ))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .init_resource::<Publication>();
        // Exercise the production publication boundary with Bevy's actual
        // inherited/view visibility systems; no second zero-time draw is used.
        configure_publication(&mut app);
        app.add_systems(PostUpdate, publish.in_set(PublishBattle));
        app.world_mut()
            .spawn((Camera::default(), Frustum::default()));
        let user_label = app
            .world_mut()
            .spawn((Visibility::Visible, Transform::default(), NoFrustumCulling))
            .id();
        let backdrop = app
            .world_mut()
            .spawn((Visibility::Hidden, Transform::default(), NoFrustumCulling))
            .id();
        app.update();
        assert!(
            app.world()
                .get::<InheritedVisibility>(user_label)
                .unwrap()
                .get()
        );
        assert!(app.world().get::<ViewVisibility>(user_label).unwrap().get());
        assert!(
            !app.world()
                .get::<InheritedVisibility>(backdrop)
                .unwrap()
                .get()
        );

        app.world_mut().resource_mut::<Publication>().0 = vec![(user_label, Visibility::Hidden)];
        app.update();
        assert!(
            !app.world()
                .get::<InheritedVisibility>(user_label)
                .unwrap()
                .get()
        );
        assert!(!app.world().get::<ViewVisibility>(user_label).unwrap().get());

        app.world_mut().resource_mut::<Publication>().0 = vec![(backdrop, Visibility::Visible)];
        app.update();
        assert!(
            app.world()
                .get::<InheritedVisibility>(backdrop)
                .unwrap()
                .get()
        );
        assert!(app.world().get::<ViewVisibility>(backdrop).unwrap().get());
        assert!(!app.world().get::<ViewVisibility>(user_label).unwrap().get());
    }

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
    fn positioned_audio_uses_the_final_frame_camera() -> Result<()> {
        use resonance_battle::{CameraPose, project_screen_x};

        let world = CameraPose {
            eye: [0., 300., 1000.],
            focus: [0.; 3],
            pitch: 15.,
            yaw: 90.,
            radius: 1000.,
        };
        let results = CameraPose {
            eye: [-2165.0625, 308.6477, -1250.0018],
            focus: [0., 112.5, 0.],
            pitch: 4.5,
            yaw: 240.,
            radius: 2500.,
        };
        let mut frame = BattleFrame {
            camera: Some(world),
            ..Default::default()
        };
        let point = [262.11926, 0., 32.63396];
        let before = project_screen_x(audio_camera(&frame)?, point);
        assert!(before > 320.);
        frame.camera = Some(results);
        assert_eq!(audio_camera(&frame)?, results);
        assert!(project_screen_x(audio_camera(&frame)?, point) < 320.);
        frame.camera = None;
        assert_eq!(
            audio_camera(&frame).unwrap_err().to_string(),
            "battle audio frame has no camera"
        );
        Ok(())
    }
}
