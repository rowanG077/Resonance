//! The battle scene borrows presentation while retaining the complete field owner.
#[cfg(test)]
use resonance_battle::TechniqueId;
pub(crate) mod appearance;
mod capture;
mod event_models;
mod feedback;
mod input;
#[cfg(test)]
mod oracle;
mod rumble;
mod transition;
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::{
        RenderTarget, ScalingMode,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
};
use resonance_battle::{ActorId, Battle, BattleFrame, Character, Side};
#[cfg(test)]
use resonance_content::prepared::Files;
use resonance_content::{menu_data::MenuData, session::SessionData};
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
    pub snapshot: resonance_content::library::Snapshot,
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
        request: &resonance_events::battle::Request,
    ) -> Result<Self> {
        let events = session.events();
        let party = events.world.party.clone().context("battle has no party")?;
        let mut gameplay_random = events.world.gameplay_random;
        let mut snapshot = session
            .files()
            .library()
            .context("battle requires a named content library")?
            .snapshot();
        use resonance_events::battle::Encounter;
        let mut setup = request.setup.clone();
        let mut selected = Vec::new();
        let table = if std::iter::once(&setup.encounter)
            .chain(&request.additional_encounters)
            .any(|e| matches!(e, Encounter::Pool(_)))
        {
            Some(resonance_content::battle_rules::capture_formations(
                &mut snapshot,
            )?)
        } else {
            None
        };
        for encounter in std::iter::once(&setup.encounter).chain(&request.additional_encounters) {
            match encounter {
                Encounter::Formation(id) => selected.push(*id),
                Encounter::Combined(ids) => selected.extend(ids),
                Encounter::Pool(pool) => selected.push(
                    table
                        .as_ref()
                        .unwrap()
                        .select(*pool, gameplay_random.next_u32())?,
                ),
            }
        }
        setup.encounter = if selected.len() == 1 {
            Encounter::Formation(selected[0])
        } else {
            Encounter::Combined(selected)
        };
        let options = encounter::PrepareOptions {
            hunting_symbol: request.hunting_symbol,
            event_flags: events.world.event_flags.clone(),
            random_seed: gameplay_random.next_u64(),
            map: u16::try_from(session.map_id())?,
            world_music: events.memory().read(0x50, symphonia_script::Width::S32)?,
            story: events.memory().read(0x40, symphonia_script::Width::S32)?,
            colette_state: events.memory().read(0x4c, symphonia_script::Width::S32)?,
            devils_arms_unlocked: events.world.event_flags.contains(&0x3fa),
            victory_story_flags: [27, 28].map(|id| events.world.event_flags.contains(&id)),
            overlimit_boost: party
                .new_game_plus
                .benefits
                .contains(&resonance_content::grade::Benefit::IncreasedTension),
        };
        Ok(Self {
            snapshot,
            party,
            setup,
            options,
            data: session.data().clone(),
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
    pub scenery_models: BTreeMap<u8, resonance_game::battle::stage::Model>,
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
    pub(super) fn prompt_party(&self) -> Option<&resonance_events::party::Party> {
        let Phase::Scene(scene) = &self.phase else {
            return None;
        };
        scene
            .candidate
            .as_ref()
            .map(|candidate| candidate.persistent_party())
    }

    pub(super) fn republish(&mut self) {
        if let Phase::Scene(scene) = &mut self.phase {
            scene.published_tick = None;
        }
    }

    pub(super) fn failed(&self) -> bool {
        matches!(self.phase, Phase::Failed)
    }

    pub(super) fn presenting(&self) -> bool {
        matches!(&self.phase, Phase::Scene(scene) if scene.state == SceneState::Active
            && scene.menu_ready && scene.failure.is_none())
    }

    pub(super) fn model_particle_stats(&self) -> Option<super::battle_view::ModelParticleStats> {
        match &self.phase {
            Phase::Scene(scene) => Some(scene.view.model_particle_stats()),
            _ => None,
        }
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
    dialogue: Option<super::field_ui::DialogueOverlay>,
    event_models: Option<event_models::State>,
    game_over: Option<super::field_ui::GameOverArt>,
    audio: Arc<super::battle_audio::Assets>,
    settings: resonance_content::menu_data::CustomizeSettings,
    core: Battle,
    feedback: feedback::Feedback,
    effects: resonance_battle::Effects,
    particles: Vec<resonance_battle::ParticleFrame>,
    lifecycle: lifecycle::Lifecycle,
    candidate: Option<results::Candidate>,
    characters: Vec<Character>,
    action_names: BTreeMap<resonance_battle::ActionKey, String>,
    music: u16,
    frame: Option<BattleFrame>,
    published_tick: Option<u32>,
    camera: Option<Entity>,
    target: RenderTarget,
    state: SceneState,
    menu_ready: bool,
    failure: Option<String>,
    completed: Option<results::Completed>,
    entry_seed: u64,
    preparing_since: std::time::Instant,
    diagnostics: resonance_content::diagnostics::Diagnostics,
}

impl Scene {
    fn feedback_paused(&self) -> bool {
        self.frame
            .as_ref()
            .is_none_or(|frame| frame.clock.feedback_paused())
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
    transition::install(app);
}

/// Move the exact request only after the worker is owned. Neither preparation
/// failure nor cancellation retires the retained event VM or its party state.
fn manage(world: &mut World) {
    retry_audio(world);
    if !world.contains_resource::<Owner>() {
        if world.contains_resource::<super::battle_audio::Playback>() {
            return;
        }
        let Some(mut request) = world
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
        let result = Entry::capture(world.resource::<super::new_game::Session>(), &request);
        let result = result.and_then(|entry| {
            request.setup = entry.setup.clone();
            let seed = entry.options.random_seed;
            let pending = super::loading::BattlePending::battle(
                entry,
                world.resource::<super::loading::Resident>(),
            )?;
            transition::begin(world, seed, request.formation_count())?;
            Ok(pending)
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
    assets: &encounter::Assets,
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
    let mut active_groups = BTreeMap::new();
    for resource in assets
        .enemies
        .spawns
        .iter()
        .map(|spawn| spawn.resource)
        .collect::<std::collections::BTreeSet<_>>()
    {
        let group = active_groups.len();
        active_groups
            .entry(assets.enemies.resources[resource].monster.id)
            .or_insert(group);
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
                actor: ActorId::from_index(party_count + index)?,
                group: u8::try_from(
                    active_groups[&assets.enemies.resources[spawn.resource].monster.id],
                )?,
                name: name.clone(),
                icon: icon.clone(),
            })
        })
        .collect()
}

fn action_names(
    battle: &Battle,
    actors: &[(ActorId, resonance_battle::Character)],
    menus: &MenuData,
    feedback: &resonance_game::battle::feedback::Feedback,
) -> BTreeMap<resonance_battle::ActionKey, String> {
    actors
        .iter()
        .flat_map(|(actor, _)| battle.prepared_techniques(*actor))
        .map(|prepared| (prepared.action, prepared.catalogue))
        .chain(
            feedback
                .action_catalogues
                .iter()
                .map(|(&action, &catalogue)| (action, catalogue)),
        )
        .chain(
            feedback
                .casting
                .iter()
                .map(|(&action, cast)| (action, cast.catalogue)),
        )
        .filter_map(|(action, catalogue)| {
            // Missing optional text is reported if this action emits a notice.
            menus
                .technique_text(catalogue.0)
                .ok()
                .map(|text| (action, text.name.clone()))
        })
        .collect()
}

fn scene(world: &mut World, package: Package) -> Result<Scene> {
    let started = std::time::Instant::now();
    let toon = world
        .resource::<AssetServer>()
        .load_builder()
        .with_settings(|settings: &mut bevy::image::ImageLoaderSettings| {
            settings.is_srgb = false;
            settings.sampler = bevy::image::ImageSampler::linear();
        })
        .load(resonance_content::model_preview::document::TOON_RAMP);
    let target = world
        .query_filtered::<&RenderTarget, With<super::FieldCamera>>()
        .single(world)?
        .clone();
    let result = construct_scene(world, package, toon, target);
    debug!(
        target: "resonance_presentation::battle_entry",
        elapsed_ms = started.elapsed().as_secs_f64() * 1000.,
        success = result.is_ok(),
        "Battle scene adoption"
    );
    result
}

fn construct_scene(
    world: &mut World,
    package: Package,
    toon: Handle<Image>,
    target: RenderTarget,
) -> Result<Scene> {
    let phase_timing = |phase: &'static str, started: std::time::Instant| {
        debug!(
            target: "resonance_presentation::battle_entry",
            phase,
            elapsed_ms = started.elapsed().as_secs_f64() * 1000.,
            "Battle scene adoption phase"
        );
    };
    let started = std::time::Instant::now();
    let diagnostics = diagnostics(world);
    let Package {
        assets,
        scenery_models,
        mut prepared,
        audio,
        party,
        menus,
        data,
        catalogue,
        gameplay_random,
        entry_seed,
    } = package;
    let enemies = enemy_hud(&assets, &prepared)?;
    let action_names = action_names(
        &prepared.core,
        &prepared.results.actors,
        &menus,
        &prepared.feedback,
    );
    *world
        .resource::<super::loading::Resident>()
        .files
        .write()
        .unwrap() = Some(Arc::new(assets.files.clone()));
    let server = world.resource::<AssetServer>().clone();
    phase_timing("metadata/resident snapshot", started);
    let (mut hud, game_over, dialogue) = world.resource_scope(
        |world, mut materials: Mut<Assets<super::field_ui::Surface>>| {
            let mut images = world.resource_mut::<Assets<Image>>();
            let started = std::time::Instant::now();
            let mut hud = super::field_ui::BattleHud::load(
                assets.ui.clone(),
                menus.clone(),
                data.clone(),
                |path| Ok(assets.files.read(path)?.to_vec()),
                &enemies,
                prepared.results.formation_count > 1,
                &server,
                &mut materials,
                &mut images,
                &diagnostics,
            )?;
            phase_timing("HUD", started);
            let started = std::time::Instant::now();
            let game_over = assets
                .game_over
                .as_ref()
                .map(|art| super::field_ui::GameOverArt::load(art, &mut materials, &mut images))
                .transpose()?;
            let dialogue = prepared
                .lifecycle
                .has_events()
                .then(|| hud.load_event_dialogue(&assets.files, &mut materials, &mut images))
                .transpose()?;
            phase_timing("game-over/dialogue", started);
            let started = std::time::Instant::now();
            hud.load_cutins(
                &assets.files,
                assets
                    .skill_scenes
                    .keys()
                    .map(|&catalogue| {
                        (
                            catalogue,
                            resonance_content::battle_scene::document::Kind::Skill,
                            catalogue,
                        )
                    })
                    .chain(assets.catalogue.definitions.iter().enumerate().filter_map(
                        |(catalogue, _)| {
                            let catalogue = catalogue as u16;
                            let scene = resonance_game::battle::stored_spells::scene(catalogue)?;
                            assets.spell_scenes.contains_key(&scene).then_some((
                                catalogue,
                                resonance_content::battle_scene::document::Kind::Technique,
                                scene,
                            ))
                        },
                    )),
                &mut materials,
                &server,
            )?;
            phase_timing("cutins", started);
            Ok::<_, anyhow::Error>((hud, game_over, dialogue))
        },
    )?;
    let started = std::time::Instant::now();
    let event_models = prepared
        .lifecycle
        .has_events()
        .then(|| {
            event_models::State::new(
                &assets.files,
                prepared.lifecycle.event_scenes(),
                &prepared.core,
                &prepared.characters,
                &mut prepared.models,
            )
        })
        .transpose()?;
    phase_timing("event models", started);
    let started = std::time::Instant::now();
    let view = super::battle_view::View::load_with_diagnostics(
        assets.stage.clone(),
        scenery_models,
        prepared.models,
        prepared.accessories,
        toon,
        prepared.effects,
        prepared.trails,
        &server,
        diagnostics.clone(),
    )?;
    phase_timing("View load", started);
    let started = std::time::Instant::now();
    let settings = party.settings.preferences.clone();
    hud.settings(settings.clone());
    hud.configure_unison_names(&assets.unison)?;
    let candidate = results::Candidate::new(
        prepared.results,
        prepared.core.actors(),
        party,
        gameplay_random,
        data,
        menus,
        catalogue,
    )?;
    phase_timing("Candidate/settings", started);
    let started = std::time::Instant::now();
    let core = prepared.core;
    let mut lifecycle = prepared.lifecycle;
    let memory = world.resource::<MenuMemory>();
    lifecycle.restore_menu_memory(resonance_game::battle::command::Memory {
        inventory: memory.inventory,
        unison_character: memory.unison_character,
        tech_character: memory.character,
    });
    let overlimit_anchors = prepared
        .feedback
        .actors
        .iter()
        .map(|actor| {
            actor
                .overlimit
                .as_ref()
                .map(|effect| (effect.bone, effect.scale))
        })
        .collect();
    let scene = Scene {
        menu_backdrop: super::menu_backdrop::BattleBackdrop::new(world)?,
        view,
        hud,
        dialogue,
        event_models,
        game_over,
        audio,
        settings,
        core,
        feedback: feedback::Feedback::new(prepared.feedback, !entry_seed),
        effects: resonance_battle::Effects::new(
            prepared.effect_banks,
            Some(prepared.poison_effect),
            !entry_seed,
            diagnostics.clone(),
        )?
        .with_stun_particle(prepared.stun_particle)?
        .with_head_anchors(prepared.head_anchors)
        .with_holy_song(prepared.holy_song_effect)?
        .with_overlimit_aura(
            resonance_battle::EffectAppearance {
                member: 46,
                ..prepared.holy_song_effect
            },
            overlimit_anchors,
        )?
        .with_ambient(prepared.ambient_effect)?
        .with_passive(prepared.passive_effects)?,
        particles: Vec::new(),
        lifecycle,
        candidate: Some(candidate),
        characters: prepared.characters,
        action_names,
        music: prepared.music,
        frame: None,
        published_tick: None,
        camera: None,
        target,
        state: SceneState::Warming,
        menu_ready: true,
        failure: None,
        completed: None,
        entry_seed,
        preparing_since: std::time::Instant::now(),
        diagnostics,
    };
    phase_timing("runtime owners/backdrop", started);
    Ok(scene)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Shared renderer assets and the retained camera/audio owners.
fn prepare(
    mut commands: Commands,
    owner: Option<ResMut<Owner>>,
    server: Res<AssetServer>,
    display: Res<super::display::Display>,
    mut assets: super::battle_view::AssetsForView,
    mut ui_surfaces: ResMut<Assets<super::field_ui::Surface>>,
    entities: super::battle_view::Entities,
    warmup: Res<super::battle_view::Warmup>,
    resident: Res<super::loading::Resident>,
    transition: Option<Res<transition::State>>,
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
        if !scene.hud.images_ready(
            &server,
            &mut assets.images,
            &mut ui_surfaces,
            &scene.diagnostics,
        )? {
            return Ok(());
        }
        scene
            .hud
            .prepare(&mut commands, &mut assets.meshes, display.0);
        if let Some(dialogue) = &mut scene.dialogue {
            dialogue.prepare(&mut commands, &mut assets.meshes, &mut ui_surfaces);
        }
        if let Some(art) = &mut scene.game_over {
            art.prepare(&mut commands, &mut assets.meshes);
        }
        let draws: Vec<_> = scene
            .hud
            .entities()
            .chain(
                scene
                    .dialogue
                    .iter()
                    .flat_map(|dialogue| dialogue.entities()),
            )
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
                            output_mode: super::renderer::COMPOSITED_OUTPUT,
                            ..default()
                        },
                        RenderTarget::Image(target.into()),
                        RenderLayers::layer(super::battle_view::WARM_LAYER),
                        super::camera::overlay_alignment(),
                        overlay_projection(display.0),
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
            if let Some(transition) = &transition
                && !transition.ready()?
            {
                return Ok(());
            }
            super::materials::TitleOutput::update(&mut outputs, |brightness| {
                brightness.x = 1.;
                brightness.y = 0.;
            });
            scene.view.activate(&mut commands, scene.target.clone())?;
            super::battle_view::activate_camera(
                &mut commands,
                scene.camera.context("battle HUD camera was not warmed")?,
                scene.target.clone(),
                overlay_projection(display.0),
                -3,
            );
            for entity in scene
                .hud
                .entities()
                .chain(
                    scene
                        .dialogue
                        .iter()
                        .flat_map(|dialogue| dialogue.entities()),
                )
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
                .core
                .advance_models(&mut frame, resonance_battle::BattleClock::Held, false)?;
            if let Some(event) = scene.lifecycle.event() {
                resonance_game::battle::event::apply_view(&event.events, &mut frame)?;
                scene
                    .event_models
                    .as_mut()
                    .context("battle event models are not prepared")?
                    .apply(event, &mut frame)?;
            }
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

pub(super) fn overlay_projection(resolution: super::Resolution) -> Projection {
    let size = resolution.ui_size();
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::Fixed {
            width: size.x,
            height: size.y,
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
    transition::dispose(world);
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
            dialogue,
            camera,
            game_over,
            menu_backdrop,
            ..
        } = *scene;
        view.despawn(&mut world.commands());
        hud.despawn(world);
        if let Some(dialogue) = dialogue {
            dialogue.despawn(world);
        }
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
    testing: Option<Res<super::testing::Controls>>,
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
    scene
        .core
        .set_instant_kill(testing.is_some_and(|testing| testing.instant_kill));
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
        session.advance_play_time();
        let command = input.command_input(
            scene.lifecycle.command_controller(),
            scene.core.actors(),
            scene.lifecycle.command_input_kind(),
        );
        let battle_input = resonance_battle::BattleInput {
            unison: input.unison_input(
                scene.core.actors(),
                scene.frame.as_ref().and_then(|frame| frame.unison.as_ref()),
            ),
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
                    order: input.order(),
                    confirm: input.confirm(),
                    cook: input.cook(),
                },
                candidate,
                &mut presentation,
            )?
        };
        let clock = frame.clock;
        let paused = clock.feedback_paused();
        let settle = matches!(
            clock,
            resonance_battle::BattleClock::Running | resonance_battle::BattleClock::EntrySettle
        ) && matches!(
            scene.core.phase(),
            resonance_battle::BattlePhase::Entry | resonance_battle::BattlePhase::Combat
        );
        if scene.lifecycle.event().is_none() {
            let core_cues = frame.cues.len();
            scene.feedback.resolve(&mut frame);
            scene.core.record_voice_requests(&frame.cues[core_cues..]);
        }
        scene.core.advance_models(&mut frame, clock, settle)?;
        if let Some(event) = scene.lifecycle.event() {
            scene
                .event_models
                .as_mut()
                .context("battle event models are not prepared")?
                .apply(event, &mut frame)?;
        }
        if scene.lifecycle.event().is_none() {
            scene.feedback.resolve_model_effects(&mut frame)?;
        }
        let effects = scene.effects.advance(&frame, paused)?;
        frame.cues.extend(effects);
        scene.effects.write_frames(&mut scene.particles);
        update_hud(
            &mut scene.hud,
            &frame,
            &scene.action_names,
            &scene.feedback.resources,
            &scene.diagnostics,
            paused,
        )?;
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
        let event_audio = scene.lifecycle.take_event_audio();
        if !event_audio.is_empty() {
            playback.event_audio(event_audio)?;
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
        scene.view.advance_feedback(&frame, paused)?;
        if scene.lifecycle.event().is_none() {
            scene
                .feedback
                .advance(&mut frame, paused, scene.effects.actor_effect_ticks());
        }
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
    diagnostics.attempt(
        "Unison HUD update",
        hud.advance_unison(frame, action_names, paused),
    )?;
    for cue in &frame.cues {
        if let resonance_battle::Cue::EffectNotice {
            actor,
            text,
            duration,
        } = cue
        {
            diagnostics.attempt(
                "battle effect notice",
                hud.notice_request(*actor, text, *duration, frame),
            )?;
        }
        if let resonance_battle::Cue::ItemNotice {
            actor,
            item,
            duration,
        } = cue
        {
            diagnostics.attempt(
                "battle item notice",
                hud.item_notice_request(*actor, *item, *duration, frame),
            )?;
        }
        if let Some((actor, text)) = feedback.notice(cue) {
            diagnostics.attempt(
                "battle recovery notice",
                hud.notice_request(actor, text, 90, frame),
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
                hud.notice_request(*actor, name, *duration, frame)
            })();
            diagnostics.attempt("battle technique notice", result)?;
        }
    }
    Ok(())
}
impl results::Presentation for Presentation<'_> {
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
    tick: u32,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) -> Result<()> {
    let pending = scene.core.pending_item();
    let result = scene.hud.render_commands(
        scene.lifecycle.command_frame().as_ref(),
        tick,
        pending,
        &scene.characters,
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
    mut projections: Query<&mut Projection>,
    mut ui_surfaces: ResMut<Assets<super::field_ui::Surface>>,
    display: Res<super::display::Display>,
    mut surfaces: ResMut<Assets<super::materials::TitleSurface>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut menu_materials: ResMut<Assets<super::menu_backdrop::Material>>,
    images: Res<Assets<Image>>,
    server: Res<AssetServer>,
    prompt_device: Res<crate::input_icons::DeviceTracker>,
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
    if scene.hud.set_prompt_device(prompt_device.device()) {
        scene.published_tick = None;
    }
    if scene.state == SceneState::Warming || scene.failure.is_some() {
        return;
    }
    let result = (|| -> Result<()> {
        if scene.published_tick == Some(*input_tick)
            && scene.menu_ready
            && !scene.menu_backdrop.awaiting_draw()
        {
            return scene.view.restore_pose(&mut globals);
        }
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
                &mut projections,
                &mut surfaces,
                &mut meshes,
            ),
        )?;
        let event_active = scene.lifecycle.event().is_some();
        let shared_menu = if event_active {
            scene.hud.hide_for_event(&mut commands);
            false
        } else {
            render_menu(
                scene,
                *input_tick,
                &mut commands,
                &mut meshes,
                &images,
                &server,
            )?
        };
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
        if !shared_menu && !event_active {
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
                        candidate.persistent_party().members[character.index()]
                            .name
                            .as_deref()
                    })
                })
                .collect::<Vec<_>>();
            let marker_anchors = (0..frame.actors.len())
                .map(|index| {
                    let actor = resonance_battle::ActorId::from_index(index)?;
                    Ok(super::field_ui::ActorMarkers {
                        target: scene.view.target_anchor(frame, actor),
                        head: scene.effects.head_position(actor, frame)?,
                        effect_tick: scene
                            .effects
                            .actor_effect_ticks()
                            .get(index)
                            .copied()
                            .unwrap_or(0),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
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
                    scene
                        .hud
                        .inspection_target(frame, command.as_ref())
                        .and_then(|target| {
                            scene
                                .view
                                .body_center(frame, target)
                                .map(|center| (target, center))
                        }),
                    &marker_anchors,
                    &mut commands,
                    &mut meshes,
                ),
            )?;
            let order = scene.lifecycle.order_selection();
            let order_label = match scene.candidate.as_ref() {
                Some(candidate) => candidate.order_name(order)?,
                None => "Cancel orders",
            };
            let target = scene.hud.inspection_target(frame, command.as_ref());
            scene.diagnostics.attempt(
                "battle information ribbons",
                scene.hud.render_ribbons(
                    frame,
                    super::field_ui::RibbonInput {
                        tick: *input_tick,
                        menu_open: command.is_some() || frame.target_selector.is_some(),
                        target,
                        order,
                        order_revision: scene.lifecycle.order_revision(),
                        order_label,
                    },
                    &mut commands,
                    &mut meshes,
                ),
            )?;
        }
        // Draw the command strip after the ordinary HUD, using the immutable game frame. A
        // closed menu supplies None.
        if !shared_menu
            && !event_active
            && !frame.summon.as_ref().is_some_and(|summon| summon.hide_hud)
        {
            render_command_strip(scene, *input_tick, &mut commands, &mut meshes)?;
        }
        if let Some(dialogue) = &mut scene.dialogue {
            if let Some(event) = scene.lifecycle.event() {
                scene.hud.render_event_dialogue(
                    dialogue,
                    &event.events.world,
                    &event.dialogue,
                    scene
                        .frame
                        .as_ref()
                        .context("battle dialogue has no held frame")?,
                    display.0,
                    &mut commands,
                    &mut meshes,
                    &mut ui_surfaces,
                )?;
            } else {
                dialogue.hide(&mut commands);
            }
        }
        let summon_fade = scene
            .frame
            .as_ref()
            .and_then(|frame| frame.summon.as_ref())
            .filter(|summon| summon.flash_active)
            .map(|summon| resonance_battle::TransitionFrame {
                color: [255; 3],
                alpha: summon.flash,
            });
        scene.diagnostics.attempt(
            "battle transition",
            scene.hud.render_transition(
                scene
                    .frame
                    .as_ref()
                    .and_then(|frame| frame.event_fade.as_ref())
                    .or(summon_fade.as_ref())
                    .or_else(|| {
                        scene
                            .frame
                            .as_ref()?
                            .enemy_presentation
                            .as_ref()?
                            .flash
                            .as_ref()
                    })
                    .or(scene.core.transition()),
                scene
                    .frame
                    .as_ref()
                    .and_then(|frame| frame.enemy_presentation.as_ref())
                    .map_or(0, |opening| opening.letterbox),
                &mut commands,
                &mut meshes,
            ),
        )?;
        scene.published_tick = Some(*input_tick);
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
mod named_tests;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn regal_encounter() -> Result<Package> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            Into::into,
        );
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        let library = resonance_content::library::Library::open(&root, &[])?;
        let mut snapshot = library.snapshot();
        resonance_content::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
        let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
        let data = SessionData::load(&files)?;
        let menus = data
            .rules
            .as_ref()
            .context("missing prepared menu rules")?
            .clone();
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
        let (assets, prepared, audio) = {
            let mut snapshot = (&files)
                .library()
                .context("named library is required")?
                .snapshot();
            let catalogue = resonance_content::battle_audio::document::Catalogue::capture(
                &mut snapshot,
                &(|| false),
            )?;
            let inputs = resonance_game::battle::encounter::Assets::capture(
                &mut snapshot,
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
                (&files).diagnostics().clone(),
                &(|| false),
            )?;
            let (mut assets, prepared, selected) = inputs.prepare_named(
                &menus,
                encounter::PrepareOptions {
                    hunting_symbol: None,
                    event_flags: Default::default(),
                    devils_arms_unlocked: false,
                    victory_story_flags: [false; 2],
                    random_seed: 1,
                    map: 332,
                    world_music: 0,
                    story: 2500,
                    colette_state: 0,
                    overlimit_boost: false,
                },
                &catalogue,
            )?;
            let audio = crate::battle_audio::Assets::capture_named(
                &catalogue,
                &selected,
                &mut snapshot,
                files.diagnostics().clone(),
                &mut Default::default(),
                &|| false,
            )?;
            assets.files = Files::from_snapshot(snapshot, || false, files.diagnostics().clone())?;
            Ok::<_, anyhow::Error>((assets, prepared, audio))
        }?;
        let catalogue = assets.catalogue.clone();
        // View retains its diagnostic and skip policy for an invalid stage.
        let scenery_models = if assets.stage.validate().is_ok() {
            resonance_game::battle::stage::Model::load(&assets.files, &assets.stage)?
        } else {
            Default::default()
        };
        Ok(Package {
            assets: Arc::new(assets),
            scenery_models,
            prepared,
            audio,
            party,
            menus,
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
        let names = action_names(
            &prepared.core,
            &prepared.results.actors,
            &menus,
            &prepared.feedback,
        );
        let owner = prepared.results.actors[0].0;
        let mut battle = prepared.core;
        assert_eq!(
            battle.technique_is_current(owner, TechniqueId(201)),
            Some(false)
        );
        let mut ready = false;
        for _ in 0..300 {
            let frame = battle.step(BattleInput::default())?;
            ready = battle.phase() == resonance_battle::BattlePhase::Combat
                && frame.clock == resonance_battle::BattleClock::Running
                && battle.activity(owner) == Activity::Idle;
            if ready {
                break;
            }
        }
        ensure!(ready, "Regal did not finish entry");
        let action = battle.record_technique_acquisition(owner, TechniqueId(201))?;
        assert_eq!(
            battle.technique_is_current(owner, TechniqueId(201)),
            Some(true)
        );
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
            false,
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
                assert_eq!(battle.technique_uses(owner, TechniqueId(201)), Some(1));
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
