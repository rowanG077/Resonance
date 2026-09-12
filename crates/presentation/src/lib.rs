//! Desktop presentation for the high-level title controller.
use anyhow::{Context, Result};
use bevy::{
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
    render::render_resource::{TextureFormat, TextureUsages},
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    sprite_render::Material2dPlugin,
    window::ExitCondition,
};
use resonance_content::{HEIGHT, TitleAssets, WIDTH};
use resonance_game::clock::PresentationClock;
use resonance_game::{DirectionRepeat, MenuInput, TitleState};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
mod audio;
mod audio_output;
mod boot;
pub use audio::{CueEvent, record_title_music};
mod camera;
mod display;
mod field_warm;
mod loading;
mod renderer;
pub use display::Resolution;
mod choice_cursor;
mod draw_order;
mod field_animation;
mod field_audio;
pub use field_audio::record_field_audio;
mod field_audit;
mod field_events;
pub use field_events::check_field_events;
mod field_effects;
mod field_pose;
mod field_probe;
mod field_refraction;
mod field_ui;
mod field_view;
mod glow;
mod materials;
mod menu_backdrop;
mod model_preview;
mod movie;
mod new_game;
mod saves;
pub use saves::{
    CheckpointReplay, SaveOptions, prepare_checkpoint_fixture, record_checkpoint,
    record_checkpoint_with_display, run_menu_probe, run_quicksave_probe, run_title_load_probe,
};
mod new_game_capture;
mod secondary_motion;
pub use new_game_capture::{
    record_new_game, record_new_game_display, record_new_game_exploration, record_new_game_until,
    record_new_game_with_gamepad,
};
mod performance;
pub use performance::{PerformanceOptions, run_frame_benchmark, run_movie_probe, run_window_probe};
mod playthrough;
mod scene;
mod screenshot;
pub use field_probe::{ClassroomProbe, ParticleProbe};
pub use field_view::{
    FieldMovement, FieldSequence, capture_classroom, capture_classroom_particles,
    capture_classroom_probe, capture_dialogue, capture_field_sequence, capture_setup,
};
mod timing;
use audio::{GameAudio, PlaybackAssets};
use audio_output::Player as AudioPlayer;
use scene::{FieldAssets, animate_field, prepare_field, update_materials};
#[derive(Resource)]
struct PendingAudio(Option<PlaybackAssets>);
use camera::TitleProjection;
use materials::{TitleOutput, TitleText};

#[derive(Resource)]
pub struct RunOptions {
    pub saves: SaveOptions,
    pub assets: PathBuf,
    pub tick: Option<u32>,
    pub presentation_start: Option<u32>,
    pub capture: Option<PathBuf>,
    pub reveal: bool,
    pub selected: usize,
    pub silent: bool,
    pub replay: Option<PathBuf>,
    pub movie_frame: Option<u32>,
    pub boot_frame: Option<u32>,
    pub skip_intro: bool,
    pub record_playthrough: Option<PathBuf>,
    pub record_title_ticks: u32,
}
impl RunOptions {
    fn headless(&self) -> bool {
        self.capture.is_some() || self.record_playthrough.is_some()
    }
}
#[derive(Resource)]
struct Replay(Option<resonance_game::replay::TitleReplay>);
#[derive(Resource)]
struct Menu(TitleState);
#[derive(Resource, Default)]
struct Clock(PresentationClock);
/// Source video can omit presentations while simulation continues (for
/// example during a field-loading stall). Replay fixtures register that gap.
#[derive(Resource, Default)]
pub(crate) struct PresentationPause(pub(crate) bool);
#[derive(Resource)]
struct Events(resonance_events::EventRuntime);
#[derive(Resource)]
struct Art {
    manifest: TitleAssets,
    images: Vec<Handle<Image>>,
}
#[derive(Component)]
struct TitleQuad {
    index: usize,
    row: Option<usize>,
}
#[derive(Resource, Default)]
struct ReadyFrames(u32);
#[derive(Resource, Default, Clone)]
struct RenderReady(Arc<AtomicBool>);
#[derive(Resource)]
struct CaptureStart(Instant);
#[derive(Resource)]
struct Framebuffer(RenderTarget);
#[derive(Component)]
struct FieldCamera;
#[derive(Resource, Default)]
struct PendingInput {
    held: MenuInput,
    pressed: MenuInput,
    up_repeat: DirectionRepeat,
    down_repeat: DirectionRepeat,
}

impl PendingInput {
    fn record_replay(&mut self, replay: &resonance_game::replay::TitleReplay, tick: u32) {
        let held = replay.held(tick);
        self.pressed = MenuInput {
            up: held.up && !self.held.up,
            down: held.down && !self.held.down,
            accept: held.accept && !self.held.accept,
            reveal: (held.up && !self.held.up)
                || (held.down && !self.held.down)
                || (held.accept && !self.held.accept),
        };
        self.held = held;
    }

    fn consume(&mut self, clock: PresentationClock) -> MenuInput {
        let pressed = std::mem::take(&mut self.pressed);
        MenuInput {
            up: self.up_repeat.step(self.held.up, pressed.up, clock.tick()),
            down: self
                .down_repeat
                .step(self.held.down, pressed.down, clock.tick()),
            reveal: pressed.reveal,
            accept: pressed.accept,
        }
    }
}

pub fn run(options: RunOptions) -> Result<()> {
    run_with_performance(options, PerformanceOptions::default())
}

pub fn run_with_performance(options: RunOptions, performance: PerformanceOptions) -> Result<()> {
    run_with_display(options, performance, Resolution::default())
}

pub fn run_with_display(
    options: RunOptions,
    performance: PerformanceOptions,
    resolution: Resolution,
) -> Result<()> {
    let headless = options.headless();
    anyhow::ensure!(
        !headless || resolution == Resolution::default(),
        "oracle recordings require native resolution"
    );
    let (mut app, recording) = build_app_with_display(options, resolution)?;
    performance::install(&mut app, performance, headless)?;
    if let Some(output) = recording {
        return playthrough::record(app, output);
    }
    anyhow::ensure!(
        app.run() == AppExit::Success,
        "Resonance exited with an error"
    );
    Ok(())
}

fn build_app_with_display(
    mut options: RunOptions,
    resolution: Resolution,
) -> Result<(App, Option<PathBuf>)> {
    options.skip_intro |= options.saves.load.is_some();
    anyhow::ensure!(
        options.presentation_start.is_none()
            || options.tick.is_some()
            || (options.record_playthrough.is_some()
                && (options.skip_intro || options.replay.is_some())),
        "presentation-start requires a title checkpoint or a title-only playthrough"
    );
    let assets = fs::canonicalize(&options.assets)
        .context("missing cooked assets; run resonance-import cook-title first")?;
    let manifest: TitleAssets = serde_json::from_slice(&fs::read(assets.join("title.json"))?)?;
    manifest.validate()?;
    for texture in &manifest.textures {
        anyhow::ensure!(
            assets.join(&texture.path).is_file(),
            "missing cooked texture {}",
            texture.path
        );
    }
    if let Some(scene) = &manifest.scene {
        anyhow::ensure!(
            assets.join(&scene.glow.texture).is_file(),
            "missing glow texture; recook title assets"
        );
        for part in &scene.parts {
            for path in std::iter::once(&part.mesh).chain(&part.textures) {
                anyhow::ensure!(
                    assets.join(path).is_file(),
                    "missing cooked scene asset {path}"
                );
            }
        }
    }
    let mut events = if let Some(scene) = &manifest.scene {
        use sha2::{Digest, Sha256};
        let bytes = fs::read(assets.join(&scene.script.path))
            .context("missing SymphoniaScript title resource; recook title assets")?;
        anyhow::ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == scene.script.sha256,
            "title script digest mismatch"
        );
        Some(resonance_game::title_events::start(&bytes, scene)?)
    } else {
        None
    };
    let replay = options
        .replay
        .as_ref()
        .map(|p| -> Result<resonance_game::replay::TitleReplay> {
            let replay: resonance_game::replay::TitleReplay =
                serde_json::from_slice(&fs::read(p)?)?;
            replay.validate()?;
            Ok(replay)
        })
        .transpose()?;
    let mut state = TitleState {
        selected: options.selected,
        ..Default::default()
    };
    if options.reveal {
        state.revealed = true;
    }
    let mut pending = PendingInput::default();
    let mut clock = PresentationClock::new(options.presentation_start.unwrap_or(0));
    if let Some(tick) = options.tick {
        for _ in 0..tick {
            clock.advance();
            if let Some(replay) = &replay {
                pending.record_replay(replay, state.tick + 1);
            }
            if let Some(events) = &mut events {
                events.step()?;
            }
            state.step(pending.consume(clock));
        }
    }
    let audio = PlaybackAssets::load(&assets)?;
    let music = (!audio.is_empty()).then_some(audio);
    let movie = movie::Playback::load(&assets, &options)?;
    let boot = boot::Playback::load(&assets, &options)?;
    let mut app = App::new();
    saves::install(&mut app, &options.saves)?;
    loading::install(&mut app, &assets);
    let recording = options.record_playthrough.clone();
    let silent = options.silent || options.headless();
    let capture_only = options.headless();
    if recording.is_some() {
        app.init_resource::<playthrough::Recording>();
    }
    let mut plugins = DefaultPlugins
        .set(AssetPlugin {
            file_path: assets.to_string_lossy().into_owned(),
            ..default()
        })
        .set(WindowPlugin {
            primary_window: (!capture_only).then(|| display::window(resolution)),
            exit_condition: if capture_only {
                ExitCondition::DontExit
            } else {
                ExitCondition::OnAllClosed
            },
            ..default()
        });
    if capture_only {
        // File captures have neither a window/event loop nor an audio device.
        plugins = plugins
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::gilrs::GilrsPlugin>();
    }
    if let Some(events) = events {
        app.insert_resource(Events(events));
    }
    let render_ready = RenderReady::default();
    app.insert_resource(render_ready.clone())
        .insert_resource(display::Display(resolution))
        .insert_resource(if capture_only {
            display::OutputStage::Framebuffer
        } else {
            display::OutputStage::Scanout
        })
        .insert_resource(bevy::winit::WinitSettings::continuous())
        .insert_resource(CaptureStart(Instant::now()))
        .insert_resource(PendingAudio(music))
        .insert_resource(movie)
        .insert_resource(boot)
        .insert_resource(Replay(replay))
        .insert_resource(options)
        .insert_resource(Menu(state))
        .insert_resource(Clock(clock))
        .init_resource::<PresentationPause>()
        .insert_resource(Art {
            manifest,
            images: Vec::new(),
        })
        .init_resource::<ReadyFrames>()
        .init_resource::<PendingInput>()
        .init_resource::<FieldAssets>()
        .init_resource::<scene::SampledImages>()
        .init_resource::<timing::Ready>()
        .insert_resource(Time::<Fixed>::from_duration(
            resonance_game::clock::UPDATE_STEP,
        ))
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(plugins)
        .add_plugins((
            Material2dPlugin::<TitleOutput>::default(),
            Material2dPlugin::<TitleText>::default(),
        ))
        .add_plugins(MaterialPlugin::<materials::TitleSurface>::default())
        .add_plugins(MaterialPlugin::<glow::GlowMaterial>::default())
        .add_plugins(draw_order::DrawOrderPlugin)
        .add_plugins(field_view::FieldPlugin)
        .init_asset::<GameAudio>()
        .init_asset::<field_audio::FieldSource>()
        .init_resource::<audio::MenuSounds>()
        .init_asset::<movie::MovieAudio>()
        .add_systems(
            PreUpdate,
            (gather_input, field_audio::acknowledge).after(bevy::input::InputSystems),
        )
        .add_systems(Startup, (setup, glow::setup, display::initialize).chain())
        .add_systems(PostUpdate, loading::black_hold)
        .add_systems(
            Update,
            (saves::release_frame, saves::capture)
                .chain()
                .after(field_view::FieldPreparation),
        )
        .add_systems(
            FixedUpdate,
            (
                timing::advance_clock,
                boot::advance,
                advance,
                new_game::advance,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                saves::update,
                new_game::enter,
                new_game::transition,
                prepare_field,
                update_materials,
                animate_field,
                skin_bounds,
                field_camera,
                glow::update,
                timing::prepare,
                boot::update,
                movie::update,
                new_game::movie_handoff,
                start_audio,
                field_audio::update,
                layout,
                capture,
                playthrough::capture,
            )
                .chain(),
        );
    if !capture_only {
        audio_output::install(&mut app, silent)?;
    } else {
        app.add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            resonance_game::clock::UPDATE_STEP,
        ));
    }
    audio::validate_startup(&app, silent, capture_only)?;
    bevy::asset::embedded_asset!(app, "title_output.wgsl");
    bevy::asset::embedded_asset!(app, "title_text.wgsl");
    bevy::asset::embedded_asset!(app, "title_surface.wgsl");
    bevy::asset::embedded_asset!(app, "title_surface_vertex.wgsl");
    bevy::shader::load_shader_library!(&mut app, "surface_bindings.wgsl");
    bevy::asset::embedded_asset!(app, "title_glow.wgsl");
    app.get_sub_app_mut(bevy::render::RenderApp)
        .context("render application unavailable")?
        .insert_resource(render_ready)
        .add_systems(
            bevy::render::Render,
            check_pipelines.in_set(bevy::render::RenderSystems::Cleanup),
        );
    field_warm::install(&mut app);
    renderer::configure(&mut app);
    Ok((app, recording))
}

fn check_pipelines(
    cache: Res<bevy::render::render_resource::PipelineCache>,
    ready: Res<RenderReady>,
) {
    use bevy::render::render_resource::CachedPipelineState;
    let complete = cache.pipelines().next().is_some()
        && cache.waiting_pipelines().next().is_none()
        && cache
            .pipelines()
            .all(|p| matches!(p.state, CachedPipelineState::Ok(_)));
    ready.0.store(complete, Ordering::Relaxed);
}

#[allow(clippy::too_many_arguments)] // Bevy injects independent resources into this startup system.
fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut art: ResMut<Art>,
    mut images: ResMut<Assets<Image>>,
    mut field: ResMut<FieldAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut outputs: ResMut<Assets<TitleOutput>>,
    mut text: ResMut<Assets<TitleText>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut movie: ResMut<movie::Playback>,
    mut boot: ResMut<boot::Playback>,
    options: Res<RunOptions>,
    display: Res<display::Display>,
    device: Res<bevy::render::renderer::RenderDevice>,
    mut exit: MessageWriter<AppExit>,
) {
    let size = match display.0.validate(device.limits().max_texture_dimension_2d) {
        Ok(()) => display.0,
        Err(error) => {
            error!("{error:#}");
            exit.write(AppExit::error());
            // Complete startup with small valid resources so other startup
            // systems can finish while AppExit is handled, without allocating
            // the invalid requested framebuffer.
            Resolution::default()
        }
    };
    // Convert directly into the window. Offscreen captures use the same pass
    // with an image target, without an additional full-screen sprite copy.
    let output = options.headless().then(|| {
        let mut image =
            Image::new_target_texture(size.width, size.height, TextureFormat::Bgra8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        images.add(image)
    });
    let framebuffer = output.as_ref().map_or_else(RenderTarget::default, |image| {
        RenderTarget::Image(image.clone().into())
    });
    commands.insert_resource(Framebuffer(framebuffer.clone()));
    // The original art and vertex colors are combined in encoded color space.
    // Keep that presentation choice in one pass, then convert for modern output.
    let mut source = Image::new_target_texture(
        size.width,
        size.scene_height(),
        TextureFormat::Bgra8Unorm,
        None,
    );
    source.sampler = ImageSampler::linear();
    source.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let source = images.add(source);
    commands.insert_resource(display::Targets {
        source: source.clone(),
        output,
    });
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(WIDTH as f32, HEIGHT as f32))),
        MeshMaterial2d(outputs.add(TitleOutput {
            source: source.clone(),
            brightness: Vec4::new(1., 0., 0., 0.),
            screen_offset: Vec2::ZERO,
        })),
        RenderLayers::layer(2),
        display::OutputQuad,
        Transform::default(),
    ));
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        RenderLayers::layer(2),
        display::OutputCamera,
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        framebuffer,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        Camera {
            order: -3,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        camera::overlay_alignment(),
        RenderTarget::Image(source.clone().into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
    movie::setup(
        &mut commands,
        &mut images,
        &mut meshes,
        &mut text,
        &mut movie,
        source.clone(),
    );
    boot::setup(
        &mut commands,
        &server,
        &mut meshes,
        &mut text,
        &mut boot,
        source.clone(),
    );
    art.images = art
        .manifest
        .textures
        .iter()
        .map(|t| {
            server
                .load_builder()
                .with_settings(|s: &mut ImageLoaderSettings| s.is_srgb = false)
                .load(t.path.clone())
        })
        .collect();
    if let Some(scene) = &art.manifest.scene {
        commands.spawn((
            Camera3d {
                depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT
                    | TextureUsages::TEXTURE_BINDING)
                    .into(),
                ..default()
            },
            Tonemapping::None,
            Msaa::Off,
            Camera {
                order: -4,
                ..default()
            },
            RenderTarget::Image(source.into()),
            Projection::custom(TitleProjection(PerspectiveProjection {
                fov: scene.fov_degrees.to_radians(),
                aspect_ratio: size.aspect(),
                near: 100.,
                far: 40000.,
                ..default()
            })),
            FieldCamera,
        ));
        field.load(scene, &server, &mut commands, &mut graphs);
    }
    let quad = meshes.add(Rectangle::new(1., 1.));
    for (index, row, top) in [
        (2, None, 24.),
        (3, None, 417.),
        (16, None, 384.),
        (5, Some(0), 295.),
        (7, Some(1), 324.),
        (9, Some(2), 353.),
    ] {
        let texture = &art.manifest.textures[index];
        commands.spawn((
            Mesh2d(quad.clone()),
            MeshMaterial2d(text.add(TitleText {
                source: art.images[index].clone(),
                opacity_pulse: Vec4::ZERO,
            })),
            Transform::from_xyz(
                0.,
                HEIGHT as f32 / 2. - top - texture.height as f32 / 2.,
                1.,
            ),
            TitleQuad { index, row },
        ));
    }
}

#[allow(clippy::too_many_arguments)] // Readiness, movie handoff, and audio asset ownership.
fn start_audio(
    mut commands: Commands,
    options: Res<RunOptions>,
    mut music: ResMut<PendingAudio>,
    mut assets: ResMut<Assets<GameAudio>>,
    mut sounds: ResMut<audio::MenuSounds>,
    ready: Res<timing::Ready>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
    recording: Option<Res<playthrough::Recording>>,
    new_game: Option<Res<new_game::Session>>,
) {
    if new_game.is_some()
        || options.saves.load.is_some()
        || recording.is_some_and(|r| !r.started)
        || movie.active
        || boot.active()
        || options.capture.is_some()
        || !ready.0
    {
        return;
    }
    if let Some(music) = music.0.take() {
        let (source, control) = music.session(movie.completed_naturally);
        sounds.control = Some(control);
        commands.spawn(AudioPlayer(assets.add(source)));
    }
}

fn skin_bounds(
    mut commands: Commands,
    meshes: Query<
        Entity,
        (
            With<bevy::mesh::skinning::SkinnedMesh>,
            Without<bevy::camera::visibility::NoFrustumCulling>,
        ),
    >,
) {
    // Bind-pose bounds do not cover the title's widely separated foliage joints.
    // This small scene does not need per-frame skinned-bounds computation.
    for mesh in &meshes {
        commands
            .entity(mesh)
            .insert(bevy::camera::visibility::NoFrustumCulling);
    }
}

fn field_camera(
    art: Res<Art>,
    events: Option<Res<Events>>,
    mut cameras: Query<&mut Transform, With<FieldCamera>>,
) {
    let Some(scene) = &art.manifest.scene else {
        return;
    };
    let Some(events) = events else {
        return;
    };
    let Some(camera) = &events.0.world.camera else {
        return;
    };
    let track = &scene.cameras[camera.resource as usize];
    // The title camera samples two updates behind the title counter.
    let time = (events.0.tick() - camera.start_tick).saturating_sub(2) as f32 * 0.5;
    let time = time.min(track.last().unwrap().time);
    let right = track
        .partition_point(|k| k.time < time)
        .min(track.len() - 1);
    let left = right.saturating_sub(1);
    let a = &track[left];
    let b = &track[right];
    let fraction = if a.time == b.time {
        0.
    } else {
        (time - a.time) / (b.time - a.time)
    };
    let position = Vec3::from_array(a.position).lerp(Vec3::from_array(b.position), fraction);
    let target = Vec3::from_array(a.target).lerp(Vec3::from_array(b.target), fraction);
    for mut camera in &mut cameras {
        *camera = Transform::from_translation(position).looking_at(target, Vec3::Z);
    }
}

fn gather_input(
    input: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut pending: ResMut<PendingInput>,
    replay: Option<Res<Replay>>,
) {
    if replay.is_some_and(|r| r.0.is_some()) {
        return;
    }
    // Preserve presses across render frames with no fixed update; consume them
    // only once if the next render frame contains several fixed updates.
    let up = input.pressed(KeyCode::ArrowUp)
        || gamepads
            .iter()
            .any(|p| p.pressed(GamepadButton::DPadUp) || p.left_stick().y > 0.75);
    let down = input.pressed(KeyCode::ArrowDown)
        || gamepads
            .iter()
            .any(|p| p.pressed(GamepadButton::DPadDown) || p.left_stick().y < -0.75);
    let accept = input.pressed(KeyCode::Enter)
        || gamepads
            .iter()
            .any(|p| p.pressed(GamepadButton::South) || p.pressed(GamepadButton::Start));
    // A press and release can both arrive before this frame. Bevy retains the
    // press event even when `pressed` is already false; do not lose that tap.
    pending.pressed.up |= (up && !pending.held.up)
        || input.just_pressed(KeyCode::ArrowUp)
        || gamepads
            .iter()
            .any(|p| p.just_pressed(GamepadButton::DPadUp));
    pending.pressed.down |= (down && !pending.held.down)
        || input.just_pressed(KeyCode::ArrowDown)
        || gamepads
            .iter()
            .any(|p| p.just_pressed(GamepadButton::DPadDown));
    pending.pressed.accept |= (accept && !pending.held.accept)
        || input.just_pressed(KeyCode::Enter)
        || gamepads
            .iter()
            .any(|p| p.just_pressed(GamepadButton::South) || p.just_pressed(GamepadButton::Start));
    pending.pressed.reveal |= pending.pressed.up || pending.pressed.down || pending.pressed.accept;
    pending.held = MenuInput {
        up,
        down,
        accept,
        reveal: up || down || accept,
    };
}

#[allow(clippy::too_many_arguments)] // Input, clock, readiness, and cue playback resources.
fn advance(
    mut commands: Commands,
    options: Res<RunOptions>,
    mut menu: ResMut<Menu>,
    clock: Res<Clock>,
    mut events: Option<ResMut<Events>>,
    mut pending: ResMut<PendingInput>,
    ready: Res<timing::Ready>,
    sounds: Res<audio::MenuSounds>,
    replay: Res<Replay>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
    recording: Option<Res<playthrough::Recording>>,
    new_game: Option<Res<new_game::Session>>,
    loading: Option<Res<loading::Pending>>,
    load_menu: Option<Res<saves::title::LoadMenu>>,
) {
    if new_game.is_some()
        || load_menu.is_some()
        || loading.is_some()
        || movie.active
        || boot.active()
        || options.tick.is_some()
        || !ready.0
        || recording.is_some_and(|r| !r.started)
    {
        return;
    }
    if let Some(replay) = &replay.0 {
        pending.record_replay(replay, menu.0.tick + 1);
    }
    let previous_selection = menu.0.selected;
    let input = pending.consume(clock.0);
    if let Some(events) = &mut events {
        events.0.step().unwrap_or_else(|e| panic!("{e:#}"));
    }
    match menu.0.step(input) {
        Some(resonance_game::TitleAction::NewGame) => {
            commands.insert_resource(new_game::Request(None));
        }
        Some(resonance_game::TitleAction::Load) => {
            commands.insert_resource(saves::title::LoadMenu::new());
            if let Some(control) = &sounds.control
                && let Err(error) = control.play("confirm")
            {
                error!("could not play confirmation cue: {error:#}");
            }
        }
        None => {}
    }
    if menu.0.selected != previous_selection
        && let Some(control) = &sounds.control
        && let Err(error) = control.play("navigate")
    {
        error!("could not play navigation cue: {error:#}");
    }
}

#[allow(clippy::too_many_arguments)] // Scene layout and output material resources.
fn layout(
    menu: Res<Menu>,
    clock: Res<Clock>,
    events: Option<Res<Events>>,
    art: Res<Art>,
    mut quads: Query<(&TitleQuad, &MeshMaterial2d<TitleText>, &mut Transform)>,
    mut materials: ResMut<Assets<TitleText>>,
    mut outputs: ResMut<Assets<TitleOutput>>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
    load_menu: Option<Res<saves::title::LoadMenu>>,
) {
    let state = &menu.0;
    TitleOutput::update(&mut outputs, |b| {
        b.x = if movie.active || boot.active() || load_menu.is_some() {
            1.
        } else {
            events.as_ref().map_or(1., |e| e.0.world.brightness())
        };
    });
    for (quad, handle, mut transform) in &mut quads {
        let mut material = materials.get_mut(&handle.0).expect("title material exists");
        let selected = quad.row == Some(state.selected);
        let source = &art.images[quad.index - usize::from(selected)];
        let opacity_pulse = Vec4::new(
            if quad.index == 16 && !state.disc_label_visible(clock.0) {
                0.
            } else {
                state.opacity as f32 / 255.
            },
            if selected {
                state.pulse_alpha() as f32 / 255.
            } else {
                0.
            },
            0.,
            0.,
        );
        if material.source != *source || material.opacity_pulse != opacity_pulse {
            material.source = source.clone();
            material.opacity_pulse = opacity_pulse;
        }
        let texture = &art.manifest.textures[quad.index];
        let expand = quad.row.map_or(0., |r| state.expansion[r].trunc());
        transform.scale = Vec3::new(
            texture.width as f32 + expand * 2.,
            texture.height as f32 + expand * 2.,
            1.,
        );
    }
}

#[allow(clippy::too_many_arguments)] // Readiness gates are independent Bevy resources.
fn capture(
    mut commands: Commands,
    options: Res<RunOptions>,
    art: Res<Art>,
    server: Res<AssetServer>,
    mut ready: ResMut<ReadyFrames>,
    menu: Res<Menu>,
    clock: Res<Clock>,
    events: Option<Res<Events>>,
    framebuffer: Res<Framebuffer>,
    field: Res<FieldAssets>,
    renderer: Res<RenderReady>,
    started: Res<CaptureStart>,
    mut exit: MessageWriter<AppExit>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
) {
    let Some(path) = options.capture.clone() else {
        return;
    };
    if options.saves.load.is_some() {
        return;
    }
    if started.0.elapsed().as_secs() > 60 {
        error!("title capture timed out waiting for assets, render pipelines, or GPU readback");
        exit.write(AppExit::error());
        return;
    }
    if !field.ready
        || !boot.ready(&server)
        || !renderer.0.load(Ordering::Relaxed)
        || !art
            .images
            .iter()
            .all(|h| server.is_loaded_with_dependencies(h.id()))
    {
        ready.0 = 0;
        return;
    }
    ready.0 += 1;
    // Let asset extraction and render pipeline preparation settle before capture.
    if ready.0 != 30 {
        return;
    }
    let mut metadata = serde_json::to_value(&menu.0).expect("title state serializes");
    metadata["presentation_counter"] = serde_json::json!(clock.0.tick());
    metadata["presentation_start"] = serde_json::json!(options.presentation_start.unwrap_or(0));
    if let Some(index) = options.movie_frame {
        metadata = serde_json::json!({"movie_frame": index, "movie": movie.asset,
            "audio_playback": false, "output_width": WIDTH, "output_height": HEIGHT});
    }
    if options.boot_frame.is_some() {
        metadata = serde_json::json!({"boot":boot.logos, "audio_playback":false,
            "output_width":WIDTH, "output_height":HEIGHT});
    }
    metadata["capture"] = serde_json::json!({"headless": true, "audio_device": false});
    if let Some(events) = events {
        let world = &events.0.world;
        metadata["events"] = serde_json::json!({
            "runtime": "SymphoniaScript",
            "script_sha256": art.manifest.scene.as_ref().map(|s| &s.script.sha256),
            "tick": events.0.tick(),
            "active_instances": events.0.active_instances(),
            "camera": world.camera.as_ref().map(|c| serde_json::json!({"resource":c.resource,"start_tick":c.start_tick})),
            "actors": world.actors.iter().map(|(id, a)| serde_json::json!({
                "id": id, "resource": a.resource, "visible": a.visible,
                "position": a.position, "depth_write": a.depth_write,
                "animation": a.animation.as_ref().map(|a| serde_json::json!({
                    "slot": a.slot, "start_tick": a.start_tick,
                })),
            })).collect::<Vec<_>>(),
            "particles": world.particles.len(),
        });
    }
    commands.spawn(Screenshot(framebuffer.0.clone())).observe(
        move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            match screenshot::write(&event.image, &path, Some(&metadata)) {
                Ok(()) => {
                    exit.write(AppExit::Success);
                }
                Err(error) => {
                    error!("capture failed: {error:#}");
                    exit.write(AppExit::error());
                }
            }
        },
    );
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn navigation_hold_matches_saved_presentation_phase() {
        let replay = serde_json::from_str::<resonance_game::replay::TitleReplay>(include_str!(
            "../../../tools/oracle/cases/native-navigation.json"
        ))
        .unwrap();
        let mut pending = PendingInput::default();
        let mut menu = TitleState::default();
        let mut clock = PresentationClock::new(2365);
        let mut changes = Vec::new();
        for tick in 1..=968 {
            clock.advance();
            pending.record_replay(&replay, tick);
            let previous = menu.selected;
            menu.step(pending.consume(clock));
            if menu.selected != previous {
                changes.push(tick);
            }
        }
        assert_eq!(changes, [912, 943, 947]);
        // Independently recorded Dolphin checkpoint: selection 0, pulse 111.
        assert_eq!((menu.selected, menu.pulse_tick), (0, 111));
        assert_eq!(clock.tick(), 3333);
    }

    #[test]
    fn repeat_uses_the_continuing_clock_after_a_different_title_entry() {
        // A held direction begins at the same scene age on both paths, but the
        // application's repeat boundary depends on time spent before entry.
        for (start, expected) in [(2365, vec![31, 63, 67]), (8651, vec![31, 61, 65])] {
            let mut pending = PendingInput::default();
            let mut clock = PresentationClock::new(start);
            let mut fired = Vec::new();
            for tick in 1..=70 {
                clock.advance();
                pending.held.down = (31..=68).contains(&tick);
                pending.pressed.down = tick == 31;
                if pending.consume(clock).down {
                    fired.push(tick);
                }
            }
            assert_eq!(fired, expected);
        }
    }

    #[test]
    fn quick_tap_survives_frames_without_a_fixed_update() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<PendingInput>()
            .add_systems(Update, gather_input);
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ArrowDown);
        keys.release(KeyCode::ArrowDown);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        let mut pending = app.world_mut().resource_mut::<PendingInput>();
        assert!(!pending.held.down);
        let pressed = std::mem::take(&mut pending.pressed);
        assert!(pressed.down && pressed.reveal);
        assert!(pending.down_repeat.step(false, pressed.down, 100));
        let consumed = pending.pressed.down;
        assert!(!pending.down_repeat.step(false, consumed, 101));
    }
}
