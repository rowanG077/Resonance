//! Field scene instances: shared cooked assets, independent actor state.
#[path = "field_sequence.rs"]
mod sequence;
#[path = "field_shadow.rs"]
mod shadows;
use super::{
    camera::TitleProjection,
    draw_order::{DrawOrder, DrawOrderPlugin},
    field_audit::{self as audit, Applied, Request},
    materials::{TitleOutput, TitleSurface},
};
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    ecs::system::SystemParam,
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
    render::{
        render_resource::{TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    sprite_render::Material2dPlugin,
    window::ExitCondition,
    world_serialization::WorldInstanceReady,
};
use resonance_content::{
    ANIMATION_HZ, HEIGHT, SCENE_HEIGHT, ScenePart, TextureBinding, WIDTH,
    field::{FieldAssets, SCENERY_RESOURCE_BASE},
};
use resonance_game::field::{FieldInput, FieldSession};
pub use sequence::{FieldMovement, FieldSequence};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Resource)]
pub(super) struct Session(pub FieldSession);
#[derive(SystemParam)]
pub(super) struct State<'w> {
    pub(super) checkpoint: Option<Res<'w, Session>>,
    pub(super) live: Option<Res<'w, super::new_game::Session>>,
}
impl State<'_> {
    pub(super) fn get(&self) -> &FieldSession {
        if let Some(session) = &self.checkpoint {
            &session.0
        } else {
            &self
                .live
                .as_ref()
                .expect("field renderer needs a session")
                .field
        }
    }
}

pub(super) struct FieldPlugin;
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct FieldPreparation;
impl Plugin for FieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FieldRendering)
            .init_resource::<Controls>()
            .add_systems(
                Update,
                super::field_ui::subtitles
                    .after(super::movie::update)
                    .after(load_live),
            )
            .add_systems(PreUpdate, gather_controls.after(bevy::input::InputSystems))
            .add_systems(FixedUpdate, advance_live.before(super::new_game::advance))
            .add_systems(
                Update,
                (
                    retire_live,
                    load_live,
                    scene_systems().run_if(resource_exists::<Art>),
                )
                    .chain()
                    .after(super::new_game::movie_handoff)
                    .after(super::new_game::transition)
                    .in_set(FieldPreparation)
                    .after(super::layout),
            );
    }
}

/// Shared by live fields and oracle captures; mesh publication must follow pose updates.
struct FieldRendering;
impl Plugin for FieldRendering {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<super::field_ui::Surface>::default())
            .init_resource::<Applied>()
            .init_resource::<super::field_pose::Authored>()
            .add_systems(
                PostUpdate,
                (
                    super::field_pose::restore,
                    super::secondary_motion::restore,
                    super::field_animation::restore,
                )
                    .chain()
                    .before(bevy::app::AnimationSystems),
            )
            .add_systems(
                PostUpdate,
                (
                    super::field_animation::blend,
                    super::field_pose::bones,
                    super::secondary_motion::apply,
                    super::field_pose::attachments,
                    shadows::pose,
                    super::field_effects::render,
                )
                    .chain()
                    .after(bevy::app::AnimationSystems)
                    // Publish rewritten effect/shadow meshes before render
                    // extraction sees their newly visible entities.
                    .before(bevy::asset::AssetEventSystems)
                    .before(bevy::transform::TransformSystems::Propagate)
                    .run_if(resource_exists::<Art>),
            )
            .add_systems(
                PostUpdate,
                audit::check
                    .after(bevy::transform::TransformSystems::Propagate)
                    .run_if(resource_exists::<Art>),
            );
        bevy::asset::embedded_asset!(app, "field_ui.wgsl");
    }
}
fn scene_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    (
        prepare,
        audit::begin,
        instances,
        pose,
        super::field_animation::bind,
        super::secondary_motion::bind,
        shadows::sync,
        camera,
        ui,
    )
        .chain()
}

#[derive(Resource, Default)]
pub(super) struct Controls {
    input: FieldInput,
    held_accept: bool,
    held_cancel: bool,
}
#[derive(Resource)]
pub(super) struct Art {
    pub(super) map: u32,
    pub(super) models: BTreeMap<u32, Vec<Part>>,
    instances: BTreeMap<i32, Vec<Entity>>,
    pub(super) ready: bool,
    loads: super::loading::LoadTasks,
    loading_since: Instant,
    shadows: shadows::Artwork,
    pub(super) toon_ramp: Handle<Image>,
}
pub(super) struct Part {
    pub(super) spec: ScenePart,
    // Retain all scenes, clips, meshes and skins for the field's lifetime.
    // Resolve labeled assets only after this canonical model load finishes.
    gltf: Handle<bevy::gltf::Gltf>,
    resolved: bool,
    pub(super) scene: Handle<WorldAsset>,
    graph: Handle<AnimationGraph>,
    clips: Vec<Handle<AnimationClip>>,
    nodes: Vec<AnimationNodeIndex>,
    pub(super) materials: Vec<Surface>,
}
pub(super) struct Surface {
    pub(super) template: Handle<StandardMaterial>,
    pub(super) color: Option<(Handle<Image>, TextureBinding)>,
    pub(super) multiply: Option<(Handle<Image>, TextureBinding)>,
}
#[derive(Component)]
pub(super) struct ActorPart {
    pub(super) actor: i32,
    pub(super) resource: u32,
    pub(super) part: usize,
    materials: Vec<Handle<TitleSurface>>,
    pub(super) prepared: bool,
    instantiated: bool,
    animation_players: Vec<Entity>,
    geometry: Vec<(usize, Entity)>,
    pub(super) active_clip: Option<usize>,
    shadow_anchor: Option<Entity>,
}
impl Art {
    /// Warmup and live actors must use the same textures and shader features.
    pub(super) fn surfaces<'a>(
        &'a self,
        resource: u32,
        index: usize,
        images: &'a mut Assets<Image>,
        sampled: &'a mut super::scene::SampledImages,
    ) -> impl Iterator<Item = (AssetId<StandardMaterial>, TitleSurface)> + 'a {
        let part = &self.models[&resource][index];
        part.materials
            .iter()
            .zip(&part.spec.materials)
            .map(move |(material, spec)| {
                (
                    material.template.id(),
                    TitleSurface {
                        color: super::scene::sampled_image(material.color.clone(), images, sampled),
                        multiply: super::scene::sampled_image(
                            material.multiply.clone(),
                            images,
                            sampled,
                        ),
                        toon_ramp: (resource < SCENERY_RESOURCE_BASE
                            && index == 0
                            && part.spec.bone_names.len() > 1
                            && spec.color.is_some())
                        .then(|| self.toon_ramp.clone()),
                        constant_color: part.spec.outline_color.is_some(),
                        blend: spec.blend,
                        depth_write: spec.depth_write,
                        cull: spec.cull,
                        ..default()
                    },
                )
            })
    }

    pub(super) fn shadow_binding(&self) -> Option<(Handle<Mesh>, Handle<TitleSurface>)> {
        self.shadows.binding()
    }
    pub(super) fn secondary_parts(&self, resource: u32) -> impl Iterator<Item = usize> + '_ {
        self.models
            .get(&resource)
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, part)| !part.spec.secondary_motion.is_empty())
            .map(|(index, _)| index)
    }
    pub(super) fn has_mouth(&self, resource: u32) -> bool {
        self.models
            .get(&resource)
            .and_then(|parts| parts.first())
            .and_then(|part| part.spec.appearance.as_ref())
            .is_some_and(|a| a.mouth.is_some())
    }
}
#[derive(Component)]
struct View;
#[derive(Resource)]
struct Checkpoint {
    output: PathBuf,
    since: Instant,
    settled: u32,
    requested: bool,
    probe: Option<super::ClassroomProbe>,
    particle_probe: Option<super::ParticleProbe>,
    dialogue_hold_ticks: u32,
}
#[derive(Resource)]
struct Manifest(FieldAssets);
#[derive(Resource)]
struct Root(PathBuf);

pub(super) fn has_live_shadows(world: &mut World) -> bool {
    world
        .query_filtered::<Entity, With<shadows::Shadow>>()
        .iter(world)
        .next()
        .is_some()
}

fn retire_live(world: &mut World) {
    if world.contains_resource::<Session>() {
        return;
    }
    if let Some(session) = world.get_resource::<super::new_game::Session>()
        && world
            .get_resource::<Art>()
            .is_none_or(|art| art.map == session.assets.map_id)
    {
        return;
    }
    if let Some(art) = world.remove_resource::<Art>() {
        art.shadows.despawn(world);
        for entity in art.instances.into_values().flatten() {
            world.despawn(entity);
        }
    }
    if let Some(ui) = world.remove_resource::<super::field_ui::Artwork>() {
        ui.despawn(world);
    }
    if let Some(effects) = world.remove_resource::<super::field_effects::Artwork>() {
        effects.despawn(world);
    }
}

#[allow(clippy::too_many_arguments)] // Bevy resources for atomic scene artwork preparation.
fn load_live(
    mut commands: Commands,
    session: Option<Res<super::new_game::Session>>,
    art: Option<Res<Art>>,
    root: Res<super::RunOptions>,
    server: Res<AssetServer>,
    mut materials: ResMut<Assets<super::field_ui::Surface>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    resident: Res<super::loading::Resident>,
    mut controls: ResMut<Controls>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(session) = session else {
        return;
    };
    if art.is_some() {
        return;
    }
    let files = resident.files.read().unwrap().clone();
    match super::field_ui::Artwork::load_with(
        &root.assets,
        &server,
        &mut materials,
        files.as_deref(),
    ) {
        Ok(mut ui) => {
            ui.prepare(&mut commands, &mut meshes, &mut materials);
            let mut effects = match super::field_effects::Artwork::load_with(
                &root.assets,
                &session.assets.effects,
                &server,
                files.as_deref(),
            ) {
                Ok(effects) => effects,
                Err(error) => {
                    error!("Could not prepare field effects: {error:#}");
                    exit.write(AppExit::error());
                    return;
                }
            };
            effects.prepare(&mut commands, &mut meshes, &mut surfaces);
            commands.insert_resource(effects);
            commands.insert_resource(ui);
            commands.insert_resource(load_art(&session.assets, &server));
            controls.input.interact = false;
            controls.input.cancel = false;
        }
        Err(error) => {
            error!("Could not prepare field presentation: {error:#}");
            exit.write(AppExit::error());
        }
    }
}

pub(super) fn gather_controls(
    input: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut controls: ResMut<Controls>,
) {
    let axis = |positive: [KeyCode; 2], negative: [KeyCode; 2]| {
        f32::from(positive.into_iter().any(|key| input.pressed(key)))
            - f32::from(negative.into_iter().any(|key| input.pressed(key)))
    };
    let mut direction = Vec2::new(
        axis(
            [KeyCode::ArrowRight, KeyCode::KeyD],
            [KeyCode::ArrowLeft, KeyCode::KeyA],
        ),
        axis(
            [KeyCode::ArrowUp, KeyCode::KeyW],
            [KeyCode::ArrowDown, KeyCode::KeyS],
        ),
    );
    for pad in &pads {
        let stick = pad.left_stick();
        if stick.length() > 0.2 {
            direction += stick;
        }
        direction += Vec2::new(
            f32::from(pad.pressed(GamepadButton::DPadRight))
                - f32::from(pad.pressed(GamepadButton::DPadLeft)),
            f32::from(pad.pressed(GamepadButton::DPadUp))
                - f32::from(pad.pressed(GamepadButton::DPadDown)),
        );
    }
    controls.input.direction = direction.clamp_length_max(1.).to_array();
    controls.input.run = input.pressed(KeyCode::ShiftLeft)
        || input.pressed(KeyCode::ShiftRight)
        || pads.iter().any(|pad| pad.pressed(GamepadButton::East));
    let accept = input.pressed(KeyCode::Enter)
        || input.pressed(KeyCode::Space)
        || pads.iter().any(|pad| pad.pressed(GamepadButton::South));
    controls.input.interact |= accept && !controls.held_accept
        || input.just_pressed(KeyCode::Enter)
        || input.just_pressed(KeyCode::Space)
        || pads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::South));
    controls.held_accept = accept;
    let cancel =
        input.pressed(KeyCode::Escape) || pads.iter().any(|pad| pad.pressed(GamepadButton::East));
    controls.input.cancel |= cancel && !controls.held_cancel
        || input.just_pressed(KeyCode::Escape)
        || pads.iter().any(|pad| pad.just_pressed(GamepadButton::East));
    controls.held_cancel = cancel;
}

#[allow(clippy::too_many_arguments)] // The fixed update waits for CPU and GPU preparation.
fn advance_live(
    mut session: Option<ResMut<super::new_game::Session>>,
    art: Option<Res<Art>>,
    ui: Option<Res<super::field_ui::Artwork>>,
    images: Res<Assets<Image>>,
    parts: Query<&ActorPart>,
    mut controls: ResMut<Controls>,
    mut exit: MessageWriter<AppExit>,
    resident: Res<super::loading::Resident>,
) {
    let Some(session) = &mut session else {
        controls.input.interact = false;
        controls.input.cancel = false;
        return;
    };
    if !resident.active.load(std::sync::atomic::Ordering::Acquire)
        || !session.ready_for_field
        || session.field.events.world.field_transition.is_some()
    {
        controls.input.interact = false;
        controls.input.cancel = false;
        return;
    }
    let Some(art) = art.filter(|a| a.ready && a.map == session.assets.map_id) else {
        return;
    };
    if ui.is_none_or(|ui| !ui.ready(&images))
        || parts.iter().any(|part| !part.prepared)
        || session.field.events.world.actors.iter().any(|(id, actor)| {
            art.models.contains_key(&actor.resource) && !art.instances.contains_key(id)
        })
    {
        return;
    }
    let input = controls.input;
    controls.input.interact = false;
    controls.input.cancel = false;
    if let Err(error) = session.field.step(input) {
        error!("Field update failed: {error:#}");
        session.field.events.cancel();
        exit.write(AppExit::error());
    }
}

#[allow(clippy::type_complexity)] // Both the live camera and isolated capture use this adapter.
fn camera(
    state: State,
    display: Option<Res<super::display::Display>>,
    mut cameras: Query<
        (&mut Transform, &mut Projection),
        Or<(With<View>, With<super::FieldCamera>)>,
    >,
    mut outputs: ResMut<Assets<TitleOutput>>,
    mut applied: ResMut<Applied>,
) {
    if state.live.as_ref().is_some_and(|s| !s.ready_for_field) {
        return;
    }
    let Some(camera) = &state.get().events.world.field_camera else {
        return;
    };
    for (mut transform, mut projection) in &mut cameras {
        *transform = Transform::from_translation(Vec3::from_array(camera.position))
            .looking_at(Vec3::from_array(camera.target), Vec3::Z);
        *projection = Projection::custom(TitleProjection(PerspectiveProjection {
            fov: camera.fov_degrees().to_radians(),
            aspect_ratio: display.as_ref().map_or(4. / 3., |d| d.0.aspect()),
            near: 100.,
            far: 40000.,
            ..default()
        }));
        applied.ack(Request::Camera);
    }
    // Field fades affect the room/actors. Speech remains readable while the
    // script deliberately holds the scene black at the beginning of the lesson.
    TitleOutput::update(&mut outputs, |b| {
        b.x = 1.;
        b.y = state
            .get()
            .events
            .world
            .fade
            .as_ref()
            .filter(|fade| fade.white)
            .map_or(0., |fade| {
                fade.alpha(state.get().events.tick()).clamp(0., 255.) / 255.
            });
    });
    applied.ack(Request::Fade);
}

#[allow(clippy::too_many_arguments)] // Dialogue state, glyph meshes, and projected actor attachments.
fn ui(
    mut commands: Commands,
    state: State,
    mut art: ResMut<super::field_ui::Artwork>,
    display: Option<Res<super::display::Display>>,
    images: Res<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<super::field_ui::Surface>>,
    roots: Query<(Entity, &ActorPart, &super::field_animation::Rig)>,
    children: Query<&Children>,
    bones: Query<(&Name, &GlobalTransform)>,
    mut exit: MessageWriter<AppExit>,
    mut applied: ResMut<Applied>,
) {
    art.resolution = display.map_or(super::Resolution::default(), |d| d.0);
    if state.live.as_ref().is_some_and(|s| !s.ready_for_field) {
        return;
    }
    if !art.ready(&images) {
        for &slot in state.get().events.world.dialogue.keys() {
            applied.loading(Request::Dialogue(slot));
            applied.loading(Request::Choice(slot));
        }
        return;
    }
    let heads = roots
        .iter()
        // A streamed actor's bind pose is not the dialogue attachment pose.
        // Wait for animation + propagation before retaining its head height.
        .filter(|(_, part, rig)| part.part == 0 && rig.sampled)
        .filter_map(|(root, part, _)| {
            children
                .iter_descendants(root)
                .filter_map(|entity| bones.get(entity).ok())
                .find(|(name, _)| name.as_str().starts_with("Bone_atama"))
                .map(|(_, transform)| (part.actor, transform.translation()))
        })
        .collect();
    if let Err(error) = art.render(
        state.get(),
        &heads,
        &mut commands,
        &mut meshes,
        &mut materials,
    ) {
        error!("Field dialogue rendering failed: {error:#}");
        exit.write(AppExit::error());
    } else {
        for (&slot, request) in &state.get().events.world.dialogue {
            if request.operation.is_pending() {
                if request.opening_actor.is_some() {
                    // The original attached window intentionally stays hidden
                    // until its speaker finishes turning. Text/voice playback
                    // is deferred by the same request, not dropped by the UI.
                    applied.ack(Request::Dialogue(slot));
                } else if state
                    .get()
                    .dialogue
                    .get(&slot)
                    .is_some_and(|p| !p.closed && p.operation.id() == request.operation.id())
                {
                    applied.ack(Request::Dialogue(slot));
                    // Artwork renders the choice after text reveal; before
                    // that it explicitly suppresses the highlight and cursor.
                    applied.ack(Request::Choice(slot));
                } else {
                    // The page player is created at the following fixed update.
                    applied.loading(Request::Dialogue(slot));
                    applied.loading(Request::Choice(slot));
                }
            }
        }
    }
}

/// Development-only capture of an equivalent script state. No audio, window,
/// controller, movie decoder, or original asset parser is constructed here.
pub fn capture_classroom(root: &Path, output: &Path, tick: Option<u32>) -> Result<()> {
    capture_field(root, output, CaptureTarget::Tick(tick))
}
/// Isolate registered observer positions and authored clip phases for diagnosis.
pub fn capture_classroom_probe(
    root: &Path,
    output: &Path,
    probe: &super::ClassroomProbe,
) -> Result<()> {
    capture_field(root, output, CaptureTarget::Probe(probe))
}
/// Compare a seeded effect without injecting observed particles or actor poses.
pub fn capture_classroom_particles(
    root: &Path,
    output: &Path,
    probe: &super::ParticleProbe,
) -> Result<()> {
    probe.anchor.validate()?;
    ensure!(
        probe.anchor.accept_updates.is_empty(),
        "particle probe cannot supply dialogue input"
    );
    capture_field(root, output, CaptureTarget::Particles(probe))
}
/// Consecutive frames from real scripts; no audio device or movie decoder.
pub fn capture_field_sequence(root: &Path, output: &Path, sequence: &FieldSequence) -> Result<()> {
    sequence.validate()?;
    capture_field(root, output, CaptureTarget::Sequence(sequence))
}
/// Setup prompt checkpoint, using the normal fresh-game entry.
pub fn capture_setup(root: &Path, output: &Path, tick: u32) -> Result<()> {
    capture_field(root, output, CaptureTarget::Setup(tick))
}
pub fn capture_dialogue(root: &Path, output: &Path, prefix: &str, hold_ticks: u32) -> Result<()> {
    ensure!(
        !prefix.is_empty(),
        "dialogue checkpoint needs a text prefix"
    );
    ensure!(hold_ticks <= 3600, "dialogue hold exceeds one minute");
    capture_field(root, output, CaptureTarget::Dialogue(prefix, hold_ticks))
}
#[derive(Clone, Copy)]
enum CaptureTarget<'a> {
    Tick(Option<u32>),
    Probe(&'a super::ClassroomProbe),
    Particles(&'a super::ParticleProbe),
    Sequence(&'a FieldSequence),
    Setup(u32),
    Dialogue(&'a str, u32),
}
fn capture_field(root: &Path, output: &Path, target: CaptureTarget<'_>) -> Result<()> {
    let setup_prompt = matches!(target, CaptureTarget::Setup(_));
    let (dialogue_prefix, dialogue_hold_ticks) = match target {
        CaptureTarget::Dialogue(prefix, hold) => (Some(prefix), hold),
        _ => (None, 0),
    };
    let probe = match target {
        CaptureTarget::Probe(probe) => Some(probe),
        CaptureTarget::Sequence(sequence) => sequence.probe.as_ref(),
        _ => None,
    };
    let root = fs::canonicalize(root)?;
    let (assets, mut session) = if setup_prompt {
        let entry = super::new_game::Session::load(&root)?;
        (entry.assets, entry.field)
    } else {
        let assets: FieldAssets =
            serde_json::from_slice(&fs::read(root.join("fields/iselia-classroom.json"))?)?;
        let messages = serde_json::from_slice(&fs::read(root.join(&assets.messages))?)?;
        let session = FieldSession::new(
            &fs::read(root.join(&assets.script.path))?,
            messages,
            &assets,
        )?;
        (assets, session)
    };
    session.voice_durations = super::field_audio::Assets::load(&root)?.voice_durations();
    let target_dialogue = |player: &resonance_game::dialogue::DialoguePlayer| {
        dialogue_prefix.is_some_and(|prefix| {
            !player.closed
                && player.operation.is_pending()
                && player.current().text().starts_with(prefix)
        })
    };
    let mut particle_probe_start = None;
    let reached = |session: &FieldSession, particle_probe_start: Option<u32>| {
        if dialogue_prefix.is_some() {
            session
                .dialogue
                .values()
                .any(|p| target_dialogue(p) && p.fully_revealed() && p.accepts_input())
        } else {
            match target {
                CaptureTarget::Tick(Some(tick)) | CaptureTarget::Setup(tick) => {
                    session.events.tick() >= tick
                }
                CaptureTarget::Tick(None)
                | CaptureTarget::Probe(_)
                | CaptureTarget::Dialogue(..) => session.events.world.input_enabled,
                CaptureTarget::Particles(probe) => particle_probe_start.is_some_and(|start| {
                    session.events.tick() - start >= probe.anchor.duration_updates
                }),
                CaptureTarget::Sequence(sequence) => sequence
                    .start_tick
                    .map_or(session.events.world.input_enabled, |tick| {
                        session.events.tick() >= tick
                    }),
            }
        }
    };
    let mut ready_since = BTreeMap::new();
    for update in 0..20000 {
        if reached(&session, particle_probe_start) {
            break;
        }
        if let CaptureTarget::Particles(probe) = target
            && particle_probe_start.is_none()
            && probe.anchor.matches(&session)
        {
            session.events.world.random_state = probe.random_state;
            particle_probe_start = Some(session.events.tick());
        }
        if let Some(movie) = &session.events.world.movie
            && movie.operation.is_pending()
        {
            movie.operation.complete(None).map_err(anyhow::Error::msg)?;
        }
        let interact = !setup_prompt
            && particle_probe_start.is_none()
            && session.dialogue.values().any(|player| {
                if player.closed
                    || player.persistent
                    || target_dialogue(player)
                    || !player.voice_finished()
                    || !player.fully_revealed()
                {
                    return false;
                }
                let since = ready_since
                    .entry((player.operation.id(), player.page))
                    .or_insert(update);
                update - *since >= 120
            });
        session.events.world.audio_commands.clear();
        session.step(FieldInput {
            interact,
            ..Default::default()
        })?;
    }
    ensure!(
        reached(&session, particle_probe_start),
        "classroom checkpoint was not reached"
    );
    for _ in 0..dialogue_hold_ticks {
        session.step(FieldInput::default())?;
        session.events.world.audio_commands.clear();
    }
    if let Some(probe) = probe {
        probe.apply(&mut session)?;
    }
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::gilrs::GilrsPlugin>(),
    )
    .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
        resonance_game::clock::UPDATE_STEP,
    ))
    .add_plugins(MaterialPlugin::<TitleSurface>::default())
    .add_plugins(Material2dPlugin::<TitleOutput>::default())
    .add_plugins(FieldRendering)
    .add_plugins(DrawOrderPlugin)
    .init_resource::<super::scene::SampledImages>()
    .insert_resource(Session(session))
    .insert_resource(Manifest(assets))
    .insert_resource(Root(root))
    .insert_resource(Checkpoint {
        output: output.into(),
        since: Instant::now(),
        settled: 0,
        requested: false,
        probe: probe.cloned(),
        particle_probe: match target {
            CaptureTarget::Particles(probe) => Some(probe.clone()),
            _ => None,
        },
        dialogue_hold_ticks,
    })
    .insert_resource(ClearColor(Color::BLACK))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            scene_systems(),
            super::skin_bounds,
            capture.run_if(not(resource_exists::<sequence::Recording>)),
        )
            .chain(),
    );
    // Embedded paths are relative to the module file, hence these live beside
    // this file and retain the same shader identifiers as the main application.
    bevy::asset::embedded_asset!(app, "title_surface.wgsl");
    bevy::asset::embedded_asset!(app, "title_surface_vertex.wgsl");
    bevy::shader::load_shader_library!(&mut app, "surface_bindings.wgsl");
    super::renderer::configure(&mut app);
    bevy::asset::embedded_asset!(app, "title_output.wgsl");
    let ready = super::RenderReady::default();
    app.insert_resource(ready.clone());
    app.get_sub_app_mut(bevy::render::RenderApp)
        .context("render application unavailable")?
        .insert_resource(ready)
        .add_systems(
            bevy::render::Render,
            super::check_pipelines.in_set(bevy::render::RenderSystems::Cleanup),
        );
    if let CaptureTarget::Sequence(sequence) = target {
        sequence::install(&mut app, output, sequence)?;
    }
    ensure!(app.run() == AppExit::Success, "classroom capture failed");
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Device-free capture's asset and render-target setup.
fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    manifest: Res<Manifest>,
    session: Res<Session>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut outputs: ResMut<Assets<TitleOutput>>,
    root: Res<Root>,
    mut ui_materials: ResMut<Assets<super::field_ui::Surface>>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
) {
    let mut effects = super::field_effects::Artwork::load(&root.0, &manifest.0.effects, &server)
        .expect("validated cooked field effects");
    effects.prepare(&mut commands, &mut meshes, &mut surfaces);
    commands.insert_resource(effects);
    let mut ui = super::field_ui::Artwork::load(&root.0, &server, &mut ui_materials)
        .expect("validated cooked dialogue artwork");
    ui.prepare(&mut commands, &mut meshes, &mut ui_materials);
    commands.insert_resource(ui);
    let mut final_image =
        Image::new_target_texture(WIDTH, HEIGHT, TextureFormat::Bgra8UnormSrgb, None);
    final_image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let final_image = images.add(final_image);
    commands.insert_resource(super::Framebuffer(RenderTarget::Image(
        final_image.clone().into(),
    )));
    let mut source =
        Image::new_target_texture(WIDTH, SCENE_HEIGHT, TextureFormat::Bgra8Unorm, None);
    source.sampler = ImageSampler::linear();
    let source = images.add(source);
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderTarget::Image(source.clone().into()),
        super::camera::overlay_alignment(),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(WIDTH as f32, HEIGHT as f32))),
        MeshMaterial2d(outputs.add(TitleOutput {
            source: source.clone(),
            brightness: Vec4::new(1., 0., 0., 0.),
        })),
        RenderLayers::layer(2),
    ));
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        RenderLayers::layer(2),
        Camera {
            order: 1,
            ..default()
        },
        RenderTarget::Image(final_image.into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
    let camera = session.0.events.world.field_camera.as_ref().unwrap();
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Msaa::Off,
        RenderTarget::Image(source.into()),
        View,
        Camera {
            order: -1,
            ..default()
        },
        Transform::from_translation(Vec3::from_array(camera.position))
            .looking_at(Vec3::from_array(camera.target), Vec3::Z),
        Projection::custom(TitleProjection(PerspectiveProjection {
            fov: camera.fov_degrees().to_radians(),
            aspect_ratio: 4. / 3.,
            near: 100.,
            far: 40000.,
            ..default()
        })),
    ));
    commands.insert_resource(load_art(&manifest.0, &server));
}

fn load_art(manifest: &FieldAssets, server: &AssetServer) -> Art {
    let loads = super::loading::LoadTasks::default();
    let mut models = BTreeMap::new();
    for (resource, parts) in manifest
        .actors
        .iter()
        .map(|a| (a.resource, a.parts.clone()))
        .chain(manifest.parts.iter().map(|p| {
            (
                SCENERY_RESOURCE_BASE + u32::from(p.resource),
                vec![p.clone()],
            )
        }))
    {
        let parts = parts
            .into_iter()
            .map(|spec| {
                let gltf = server
                    .load_builder()
                    .with_guard(loads.ticket())
                    .load(spec.mesh.clone());
                let load = |b: &TextureBinding| {
                    (
                        server
                            .load_builder()
                            .with_guard(loads.ticket())
                            .with_settings(|s: &mut ImageLoaderSettings| s.is_srgb = false)
                            .load(spec.textures[b.texture].clone()),
                        b.clone(),
                    )
                };
                let materials = spec
                    .materials
                    .iter()
                    .map(|m| Surface {
                        template: Handle::default(),
                        color: m.color.as_ref().map(load),
                        multiply: m.multiply.as_ref().map(load),
                    })
                    .collect();
                Part {
                    spec,
                    gltf,
                    resolved: false,
                    scene: Handle::default(),
                    graph: Handle::default(),
                    clips: Vec::new(),
                    nodes: Vec::new(),
                    materials,
                }
            })
            .collect();
        models.insert(resource, parts);
    }
    Art {
        map: manifest.map_id,
        models,
        instances: BTreeMap::new(),
        ready: false,
        loading_since: Instant::now(),
        shadows: shadows::Artwork::load(&manifest.contact_shadow, server),
        toon_ramp: server
            .load_builder()
            .with_guard(loads.ticket())
            .with_settings(|s: &mut ImageLoaderSettings| {
                s.is_srgb = false;
                s.sampler = ImageSampler::linear();
            })
            .load(manifest.toon_ramp.clone()),
        loads,
    }
}
fn prepare(
    mut art: ResMut<Art>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    templates: Res<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    mut exit: MessageWriter<AppExit>,
) {
    // A field owns these handles until teardown. Once its load jobs and
    // dependencies have finished, do not lock the asset server for every clip
    // on every rendered frame. A field change creates a new Art and gate.
    if art.ready {
        return;
    }
    if art.loads.complete() {
        let Art { models, loads, .. } = &mut *art;
        for part in models.values_mut().flatten().filter(|p| !p.resolved) {
            let Some(gltf) = gltfs.get(&part.gltf) else {
                continue;
            };
            if !server.is_loaded_with_dependencies(part.gltf.id()) {
                continue;
            }
            let Some(scene) = gltf.scenes.first() else {
                error!("Field model has no scene: {}", part.spec.mesh);
                exit.write(AppExit::error());
                return;
            };
            if gltf.animations.len() != part.spec.clips.len() {
                error!(
                    "Field model animation inventory mismatch: {}",
                    part.spec.mesh
                );
                exit.write(AppExit::error());
                return;
            }
            part.scene = scene.clone();
            part.clips = gltf.animations.clone();
            let (graph, nodes) = AnimationGraph::from_clips(part.clips.iter().cloned());
            part.graph = graphs.add(graph);
            part.nodes = nodes;
            for (index, material) in part.materials.iter_mut().enumerate() {
                // Usually already held by the scene. An unused material may
                // require its own load; that job must finish before warmup too.
                material.template = server
                    .load_builder()
                    .with_guard(loads.ticket())
                    .load(format!("{}#Material{index}/std", part.spec.mesh));
            }
            part.resolved = true;
        }
    }
    art.ready = art.loads.complete()
        && images.contains(&art.toon_ramp)
        && images.contains(&art.shadows.texture)
        && art.models.values().flatten().all(|part| {
            part.resolved
                && server.is_loaded_with_dependencies(part.gltf.id())
                && server.is_loaded_with_dependencies(part.scene.id())
                && part
                    .clips
                    .iter()
                    .all(|clip| server.is_loaded_with_dependencies(clip.id()))
                && part.materials.iter().all(|m| {
                    templates.contains(m.template.id())
                        && m.color
                            .iter()
                            .chain(&m.multiply)
                            .all(|(h, _)| images.contains(h.id()))
                })
        });
    if !art.ready {
        let assets = art
            .models
            .values()
            .flatten()
            .flat_map(|p| {
                [p.gltf.id().untyped(), p.scene.id().untyped()]
                    .into_iter()
                    .chain(p.clips.iter().map(|c| c.id().untyped()))
                    .chain(p.materials.iter().flat_map(|m| {
                        std::iter::once(m.template.id().untyped()).chain(
                            m.color
                                .iter()
                                .chain(&m.multiply)
                                .map(|(h, _)| h.id().untyped()),
                        )
                    }))
            })
            .chain([
                art.toon_ramp.id().untyped(),
                art.shadows.texture.id().untyped(),
            ]);
        for id in assets {
            if let Some((bevy::asset::LoadState::Failed(error), _, _)) = server.get_load_states(id)
            {
                error!(
                    "Field asset failed to load: {:?}: {error}",
                    server.get_path(id)
                );
                exit.write(AppExit::error());
                return;
            }
            if let Some(bevy::asset::RecursiveDependencyLoadState::Failed(error)) =
                server.get_recursive_dependency_load_state(id)
            {
                error!(
                    "Field asset dependency failed: {:?}: {error}",
                    server.get_path(id)
                );
                exit.write(AppExit::error());
                return;
            }
        }
        if art.loading_since.elapsed().as_secs() >= 30 {
            error!(
                "Field {} presentation assets did not become ready within 30 seconds",
                art.map
            );
            exit.write(AppExit::error());
        }
    }
}
fn instances(
    mut commands: Commands,
    mut art: ResMut<Art>,
    session: State,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut images: ResMut<Assets<Image>>,
    mut sampled: ResMut<super::scene::SampledImages>,
) {
    if !art.ready {
        return;
    }
    for (&id, actor) in &session.get().events.world.actors {
        if art.instances.contains_key(&id) {
            continue;
        }
        let Some(parts) = art.models.get(&actor.resource) else {
            continue;
        };
        let entities = parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                let materials = art
                    .surfaces(actor.resource, index, &mut images, &mut sampled)
                    .map(|(_, surface)| surfaces.add(surface))
                    .collect();
                commands
                    .spawn((
                        WorldAssetRoot(part.scene.clone()),
                        Transform::default(),
                        ActorPart {
                            actor: id,
                            resource: actor.resource,
                            part: index,
                            materials,
                            prepared: false,
                            instantiated: false,
                            animation_players: Vec::new(),
                            geometry: Vec::new(),
                            active_clip: None,
                            shadow_anchor: None,
                        },
                    ))
                    .observe(
                        |event: On<WorldInstanceReady>,
                         mut actors: Query<&mut ActorPart>,
                         mut commands: Commands,
                         mut applied: ResMut<Applied>| {
                            if let Ok(mut actor) = actors.get_mut(event.entity) {
                                // glTF subasset loads can replace a scene instance.
                                // Entity caches belong to that instance, not just
                                // to the outer actor's lifetime.
                                actor.instantiated = true;
                                actor.prepared = false;
                                // Scene readiness can arrive after Update's
                                // bind pass. Rebuilding the rig is a real load
                                // dependency until the next pass, not a lost
                                // secondary-motion request. Keep the same
                                // bounded allowance used by initial loading.
                                applied.loading(Request::SecondaryMotion(actor.actor, actor.part));
                                actor.animation_players.clear();
                                actor.geometry.clear();
                                actor.active_clip = None;
                                actor.shadow_anchor = None;
                                commands
                                    .entity(event.entity)
                                    .remove::<super::secondary_motion::Rig>()
                                    .remove::<super::field_animation::Rig>();
                            }
                        },
                    )
                    .id()
            })
            .collect();
        art.instances.insert(id, entities);
    }
    let removed: Vec<_> = art
        .instances
        .keys()
        .filter(|id| !session.get().events.world.actors.contains_key(id))
        .copied()
        .collect();
    for id in removed {
        for entity in art.instances.remove(&id).unwrap() {
            commands.entity(entity).despawn();
        }
    }
}
#[allow(clippy::too_many_arguments)] // Independent scene instances, materials, and animation players.
fn pose(
    mut commands: Commands,
    session: State,
    art: Res<Art>,
    children: Query<&Children>,
    mut roots: Query<(Entity, &mut ActorPart, &mut Transform, &mut Visibility)>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    names: Query<&Name>,
    mut players: Query<(&mut AnimationPlayer, Option<&AnimationGraphHandle>)>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut node_visibility: Query<&mut Visibility, Without<ActorPart>>,
    mut applied: ResMut<Applied>,
    effects: Res<super::field_effects::Artwork>,
) {
    let session = session.get();
    for (root, mut instance, mut transform, mut visibility) in &mut roots {
        if !instance.instantiated {
            applied.loading(Request::SecondaryMotion(instance.actor, instance.part));
            if instance.part == 0 {
                applied.loading(Request::Mouth(instance.actor));
            }
            applied.loading(Request::Actor(instance.actor, instance.part));
            if let Some(actor) = session.events.world.actors.get(&instance.actor) {
                for request in audit::actor_requests(instance.actor, instance.part, actor) {
                    applied.loading(request);
                }
            }
            continue;
        }
        let Some(actor) = session.events.world.actors.get(&instance.actor) else {
            continue;
        };
        debug_assert_eq!(
            instance.resource,
            actor.resource,
            "VM actor {} changed its model without replacing the scene instance at tick {}",
            instance.actor,
            session.events.tick()
        );
        let part = &art.models[&instance.resource][instance.part];
        for (index, material) in part.spec.materials.iter().enumerate() {
            let mut offset = [0.; 2];
            if let Some(channels) = &part.spec.appearance
                && let Some(binding) = &material.color
            {
                if let Some(variant) = &channels.variant
                    && variant.texture == binding.texture
                {
                    offset[1] += f32::from(actor.appearance.expression % variant.frames)
                        / f32::from(variant.frames);
                }
                if channels.eyes == Some(binding.texture)
                    && let resonance_events::Face::Frame(frame) = actor.appearance.face
                {
                    offset[1] += f32::from(frame) / 16.;
                }
                if channels.mouth == Some(binding.texture) {
                    let frame = match actor.appearance.mouth {
                        Some(resonance_events::Face::Frame(frame)) => frame,
                        _ if session.talking.contains_key(&instance.actor) => effects.mouth_frame(
                            session
                                .events
                                .tick()
                                .saturating_sub(session.talking[&instance.actor]),
                        ),
                        Some(resonance_events::Face::Blink) => {
                            effects.mouth_frame(session.events.tick())
                        }
                        _ => 0,
                    };
                    offset[1] += f32::from(frame) / 8.;
                    applied.ack(Request::Mouth(instance.actor));
                }
                if channels.costume == Some(binding.texture) {
                    // Initial costume variants for the party.
                    let frame = match actor.resource {
                        2..=4 => 3,
                        7 => 1,
                        _ => 0,
                    };
                    offset[1] += frame as f32 / 4.;
                }
            }
            let offsets = Vec4::new(offset[0], offset[1], 0., 0.);
            let brightness = session.events.world.brightness();
            let tint = Vec4::new(brightness, brightness, brightness, 1.)
                * part.spec.outline_color.map_or(Vec4::ONE, |color| {
                    Vec4::from_array(color.map(|c| f32::from(c) / 255.))
                });
            let depth_write = material.depth_write && actor.depth_write;
            let light = session.character_light(instance.actor);
            let light_position = match light.position {
                resonance_events::effect::LightPosition::Relative(p) => {
                    Vec3::from_array(p) + Vec3::from_array(actor.position)
                }
                resonance_events::effect::LightPosition::World(p) => Vec3::from_array(p),
                resonance_events::effect::LightPosition::Actor { id, height } => session
                    .events
                    .world
                    .actors
                    .get(&id)
                    .map_or(Vec3::ZERO, |a| {
                        Vec3::from_array(a.position) + Vec3::Z * height
                    }),
            }
            .extend(f32::from(light.strength));
            // Quantize light colors to five bits per channel before shading.
            let shades = [light.shade, light.bright].map(|c| {
                Vec3::from_array(c.map(|v| {
                    let v = v >> 3;
                    f32::from((v << 3) | (v >> 2)) / 255.
                }))
                .extend(1.)
            });
            if surfaces.get(&instance.materials[index]).is_some_and(|s| {
                s.uv_offsets != offsets
                    || s.tint != tint
                    || s.depth_write != depth_write
                    || s.field_light != light_position
                    || s.shade_colors != shades
            }) {
                let mut surface = surfaces.get_mut(&instance.materials[index]).unwrap();
                surface.uv_offsets = offsets;
                surface.tint = tint;
                surface.depth_write = depth_write;
                surface.field_light = light_position;
                surface.shade_colors = shades;
            }
        }
        transform.translation = Vec3::from_array(actor.position);
        transform.rotation = Quat::from_rotation_z(
            actor
                .appearance
                .fixed_heading
                .unwrap_or(actor.heading)
                .to_radians(),
        );
        *visibility = if actor.visible && !actor.appearance.model_hidden {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let animation = actor.animation.as_ref().and_then(|a| {
            part.spec
                .clips
                .iter()
                .position(|c| {
                    c.resource_slot == a.slot
                        && c.animation_resource.unwrap_or(actor.resource) == a.resource
                })
                .map(|index| (a, index))
        });
        if !instance.prepared {
            let mut descendants = 0;
            for entity in children.iter_descendants(root) {
                descendants += 1;
                if instance.part == 0
                    && names.get(entity).is_ok_and(|name| {
                        part.spec
                            .bone_names
                            .get(usize::from(art.shadows.spec.anchor_node))
                            // Single-node props use their root as the shadow anchor.
                            .or_else(|| part.spec.bone_names.first())
                            .is_some_and(|bone| name.as_str() == bone)
                    })
                {
                    instance.shadow_anchor = Some(entity);
                }
                if let Ok(template) = meshes.get(entity)
                    && let Some(index) = part
                        .materials
                        .iter()
                        .position(|m| m.template.id() == template.id())
                {
                    instance.geometry.push((index, entity));
                    commands
                        .entity(entity)
                        .remove::<MeshMaterial3d<StandardMaterial>>()
                        .insert((
                            MeshMaterial3d(instance.materials[index].clone()),
                            DrawOrder(part.spec.materials[index].draw_order),
                        ));
                }
                if let Ok((_, graph)) = players.get_mut(entity) {
                    if graph.is_none() {
                        commands
                            .entity(entity)
                            .insert(AnimationGraphHandle(part.graph.clone()));
                    }
                    instance.animation_players.push(entity);
                }
            }
            instance.prepared = descendants > 0;
        }
        for &(material, entity) in &instance.geometry {
            // Hide only geometry attached to the script node; its transform
            // and child bones must remain active.
            let hidden = part.spec.material_nodes.get(material).is_some_and(|nodes| {
                nodes
                    .iter()
                    .any(|node| actor.appearance.hidden_nodes.contains(node))
            });
            let visibility = if hidden {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            if let Ok(mut current) = node_visibility.get_mut(entity)
                && *current != visibility
            {
                *current = visibility;
            }
        }
        if let Some((a, index)) = animation {
            for &entity in &instance.animation_players {
                let Ok((mut player, _)) = players.get_mut(entity) else {
                    continue;
                };
                if instance.active_clip != Some(index) {
                    player.stop_all();
                }
                let duration = part.spec.clips[index].duration_seconds * ANIMATION_HZ;
                player
                    .play(part.nodes[index])
                    .pause()
                    .set_seek_time(a.sample(session.events.tick(), 0, duration) / ANIMATION_HZ);
                applied.ack(Request::Animation {
                    actor: instance.actor,
                    part: instance.part,
                    resource: a.resource,
                    slot: a.slot,
                });
            }
            instance.active_clip = Some(index);
        }
        if instance.prepared
            && (0..part.spec.materials.len()).all(|index| {
                instance
                    .geometry
                    .iter()
                    .any(|(material, _)| *material == index)
            })
        {
            applied.ack(Request::Actor(instance.actor, instance.part));
        }
    }
}
#[allow(clippy::too_many_arguments)] // Snapshot readiness, actor evidence, and GPU readback.
fn capture(
    mut commands: Commands,
    mut checkpoint: ResMut<Checkpoint>,
    art: Res<Art>,
    parts: Query<&ActorPart>,
    framebuffer: Res<super::Framebuffer>,
    ready: Res<super::RenderReady>,
    session: Res<Session>,
    dialogue_art: Res<super::field_ui::Artwork>,
    mut exit: MessageWriter<AppExit>,
    roots: Query<(Entity, &ActorPart)>,
    children: Query<&Children>,
    bones: Query<(&Name, &Transform, &GlobalTransform)>,
) {
    if checkpoint.requested {
        return;
    }
    if checkpoint.since.elapsed().as_secs() > 120 {
        error!("classroom capture timed out");
        exit.write(AppExit::error());
        return;
    }
    if !art.ready
        || parts.is_empty()
        || parts.iter().any(|p| !p.prepared)
        || !ready.0.load(std::sync::atomic::Ordering::Relaxed)
    {
        checkpoint.settled = 0;
        return;
    }
    checkpoint.settled += 1;
    if checkpoint.settled < 20 {
        return;
    }
    checkpoint.requested = true;
    let path = checkpoint.output.clone();
    let pose_nodes:Vec<_>=roots.iter().filter(|(_,p)|p.actor==1 && p.part==0).flat_map(|(root,_)|children.iter_descendants(root))
        .filter_map(|entity|bones.get(entity).ok()).map(|(name,local,global)|serde_json::json!({"name":name.as_str(),"translation":local.translation.to_array(),"rotation":local.rotation.to_array(),"world":global.to_matrix().to_cols_array()})).collect();
    let actor_poses: Vec<_> = roots.iter().filter(|(_,p)|p.part==0).map(|(root,p)|serde_json::json!({"actor":p.actor,"nodes":children.iter_descendants(root).filter_map(|entity|bones.get(entity).ok()).map(|(name,local,global)|serde_json::json!({"name":name.as_str(),"translation":local.translation.to_array(),"rotation":local.rotation.to_array(),"world":global.to_matrix().to_cols_array()})).collect::<Vec<_>>()})).collect();
    let state = serde_json::json!({"kind":"classroom-development-checkpoint","audio_device":false,"tick":session.0.events.tick(),
        "registered_probe":checkpoint.probe,
        "registered_particle_probe":checkpoint.particle_probe,
        "billboards":session.0.events.world.billboards.iter().map(|(id,p)|serde_json::json!({"id":id,"age":session.0.events.tick()-p.born,"position":p.position,"size":p.size,"size_delta":p.size_delta,"rotation":p.rotation,"angular_velocity":p.angular_velocity,"alpha":p.alpha(session.0.events.tick())})).collect::<Vec<_>>(),
        "dialogue_hold_ticks":checkpoint.dialogue_hold_ticks,
        "dialogue_layouts":dialogue_art.diagnostic_layouts(&session.0),
        "actor_poses":actor_poses,
        "controlled_pose":pose_nodes,
        "dialogue": session.0.dialogue.iter().filter(|(_,p)| !p.closed).map(|(slot,p)|serde_json::json!({"slot":slot,"visible":p.visible,"text":p.current().glyphs.iter().take(p.visible).map(|g|g.character).collect::<String>()})).collect::<Vec<_>>(),
        "actors":session.0.events.world.actors.iter().map(|(id,a)|serde_json::json!({"id":id,"position":a.position,"heading":a.heading,"animation":a.animation.as_ref().map(|a|serde_json::json!({"resource":a.resource,"slot":a.slot,"start_tick":a.start_tick,"sample":a.sample(session.0.events.tick(),0,a.duration_ticks as f32)}))})).collect::<Vec<_>>(),
        "input_enabled":session.0.events.world.input_enabled,"camera":session.0.events.world.field_camera.as_ref().map(|c|serde_json::json!({"position":c.position,"target":c.target,"fov_degrees":c.fov_degrees()}))});
    commands.spawn(Screenshot(framebuffer.0.clone())).observe(
        move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            let result = (|| -> Result<()> {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                event.image.clone().try_into_dynamic()?.save(&path)?;
                fs::write(
                    path.with_extension("json"),
                    serde_json::to_vec_pretty(&state)?,
                )?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    exit.write(AppExit::Success);
                }
                Err(error) => {
                    error!("classroom capture failed: {error:#}");
                    exit.write(AppExit::error());
                }
            }
        },
    );
}
