//! Desktop presentation for the high-level title controller.
mod diagnostics;
#[cfg(test)]
mod test_support;
use anyhow::{Context, Result};
use bevy::{
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
    render::render_resource::{TextureFormat, TextureUsages},
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
mod battle;
mod battle_view;
mod boot;
pub use audio::{CueEvent, record_title_music};
mod camera;
mod debug_font;
mod display;
mod dungeons;
mod field_warm;
mod loading;
mod renderer;
mod testing;
pub use display::Resolution;
mod draw_order;
mod field_animation;
mod field_audio;
#[cfg(test)]
mod field_test;
mod ui_coordinates;
pub(crate) use field_audio::battle as battle_audio;
mod sparse_animation;
pub use field_audio::record_field_audio;
mod field_audit;
mod field_events;
pub use field_events::check_field_events;
mod field_capture;
mod field_dissolve;
mod field_effects;
mod field_model_particles;
mod field_pose;
mod field_probe;
mod field_refraction;
mod field_rumble;
mod field_ui;
use field_ui::{credits, session_screen};
mod field_view;
mod game_over;
mod glow;
mod materials;
pub use materials::{
    TitleOutput, TitleSurface, TitleText, install_output_materials, install_surface_material,
};
mod menu_backdrop;
mod model_preview;
mod movie;
mod new_game;
mod overworld;
pub use overworld::{Probe as OverworldProbe, capture_overworld};
mod saves;
pub use saves::{
    CheckpointRecordingOptions, CheckpointReplay, SaveOptions, prepare_checkpoint_fixture,
    prepare_overworld_test_fixture, record_checkpoint, record_new_game, run_menu_probe,
    run_overworld_field_probe, run_quicksave_probe, run_title_load_probe,
};
mod new_game_capture;
mod performance;
mod secondary_motion;
pub use performance::{PerformanceOptions, run_frame_benchmark, run_movie_probe, run_window_probe};
mod playthrough;
mod scene;
mod screenshot;
pub use field_probe::ClassroomProbe;
pub use field_view::{
    CaptureMoment, FieldControls, FieldScene, FieldSequence, FieldSequenceRenderer,
    capture_field_sequence as capture_effect_sequence,
};

pub use field_probe::{FieldCapture, capture_field, capture_field_sequence};
mod timing;
use audio::{GameAudio, PlaybackAssets};
use audio_output::Player as AudioPlayer;
use scene::{FieldAssets, animate_field, prepare_field, update_materials};
#[derive(Resource)]
struct PendingAudio(Option<PlaybackAssets>);
use camera::TitleProjection;

#[derive(Resource)]
pub struct RunOptions {
    pub saves: SaveOptions,
    pub assets: PathBuf,
    /// Optional editable source project containing explicit fields.json bindings.
    pub script_root: Option<PathBuf>,
    pub capture_at: Option<CaptureAt>,
    pub capture: Option<PathBuf>,
    pub reveal: bool,
    pub selected: usize,
    pub silent: bool,
    /// Stop at recoverable content/runtime errors instead of logging and continuing.
    pub paranoid: bool,
    pub skip_intro: bool,
    /// Temporary exploration: resolve field and world battles as victories.
    pub skip_battles: bool,
    /// Permit unsupported field scripts only in the disposable overworld playground.
    pub allow_incomplete_scripts: bool,
    pub record_playthrough: Option<PathBuf>,
    pub record_title_ticks: u32,
}
impl RunOptions {
    fn headless(&self) -> bool {
        self.capture.is_some() || self.record_playthrough.is_some()
    }
}
/// A CLI checkpoint translated into the shared scenario runner.
#[derive(Clone, Copy, Debug)]
pub enum CaptureAt {
    TitleTick(u32),
    MovieFrame(u32),
    BootTick(u32),
    LoadedField,
}
#[derive(Resource)]
struct Menu(TitleState);
#[derive(Resource, Default)]
struct Clock(PresentationClock);
#[derive(Resource)]
struct TitleActive;
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
#[derive(Resource, Default, Clone)]
struct RenderReady(Arc<std::sync::Mutex<model_preview::gpu::Report>>);
#[derive(Resource)]
struct Framebuffer(RenderTarget);
#[derive(Component)]
// Keep the view's pipeline key stable when a field starts or finishes fog.
#[require(DistanceFog)]
struct FieldCamera;
#[derive(Component)]
struct FieldOverlayCamera;
#[derive(Resource, Default)]
struct PendingInput {
    held: MenuInput,
    pressed: MenuInput,
    up_repeat: DirectionRepeat,
    down_repeat: DirectionRepeat,
}

impl PendingInput {
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
    let checkpoint = options
        .capture_at
        .or_else(|| options.saves.load.as_ref().map(|_| CaptureAt::LoadedField))
        .zip(options.capture.clone());
    let headless = options.headless();
    anyhow::ensure!(
        !headless || resolution == Resolution::default(),
        "oracle recordings require native resolution"
    );
    let (mut app, recording) = build_app_with_display(options, resolution)?;
    performance::install(&mut app, performance, headless)?;
    if let Some((at, output)) = checkpoint {
        return playthrough::capture(app, at, &output);
    }
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
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(options.paranoid);
    anyhow::ensure!(
        options.capture_at.is_none() || options.capture.is_some(),
        "capture target requires an output path"
    );
    options.skip_intro |=
        options.saves.load.is_some() || matches!(options.capture_at, Some(CaptureAt::TitleTick(_)));
    let assets = fs::canonicalize(&options.assets)
        .context("missing cooked assets; run resonance-import cook-all first")?;
    let title_path = assets.join("title.json");
    let mut manifest: TitleAssets = serde_json::from_slice(
        &fs::read(&title_path).with_context(|| format!("reading {}", title_path.display()))?,
    )
    .with_context(|| {
        format!(
            "incompatible cooked title assets at {}; rerun resonance-import cook-all for this asset directory",
            title_path.display()
        )
    })?;
    manifest.validate()?;
    let mut prepared_clips = sparse_animation::Prepared::default();
    let events = if let Some(scene) = &manifest.scene {
        diagnostics.attempt(
            "startup title script",
            (|| {
                use sha2::{Digest, Sha256};
                let bytes = fs::read(assets.join(&scene.script.path))
                    .context("missing SymphoniaScript title resource; recook title assets")?;
                anyhow::ensure!(
                    format!("{:x}", Sha256::digest(&bytes)) == scene.script.sha256,
                    "title script digest mismatch"
                );
                resonance_game::title_events::start(&bytes, scene, |path| {
                    prepared_clips.load(&assets, path)
                })
            })(),
        )?
    } else {
        None
    };
    if events.is_none() {
        manifest.scene = None;
    }
    let state = TitleState {
        selected: options.selected,
        revealed: options.reveal,
        ..Default::default()
    };
    let music =
        Some(PlaybackAssets::load(&assets, diagnostics.clone())?).filter(|audio| !audio.is_empty());
    let movie = diagnostics
        .attempt("startup movie", movie::Playback::load(&assets, &options))?
        .unwrap_or_default();
    let boot = diagnostics
        .attempt("startup logos", boot::Playback::load(&assets, &options))?
        .unwrap_or_default();
    let mut app = App::new();
    app.insert_resource(session_screen::Title {
        events: events
            .as_ref()
            .map(resonance_events::EventRuntime::fresh)
            .transpose()?,
        audio: music.clone(),
    });
    app.insert_resource(crate::diagnostics::Diagnostics(diagnostics));
    app.insert_resource(TitleActive);
    app.insert_resource(prepared_clips);
    saves::install(&mut app, &options.saves)?;
    loading::install(&mut app, &assets);
    let recording = options.record_playthrough.clone();
    let silent = options.silent || options.headless();
    let capture_only = options.headless();
    let mut plugins = DefaultPlugins
        .set(bevy::pbr::PbrPlugin {
            gltf_enable_standard_materials: false,
            ..default()
        })
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
        .insert_resource(PendingAudio(if options.saves.load.is_some() {
            None
        } else {
            music
        }))
        .insert_resource(movie)
        .insert_resource(boot)
        .insert_resource(options)
        .insert_resource(Menu(state))
        .init_resource::<Clock>()
        .insert_resource(Art {
            manifest,
            images: Vec::new(),
        })
        .init_resource::<PendingInput>()
        .init_resource::<FieldAssets>()
        .init_resource::<scene::SampledImages>()
        .init_resource::<timing::Ready>()
        .insert_resource(Time::<Fixed>::from_duration(
            resonance_game::clock::UPDATE_STEP,
        ))
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(plugins)
        .add_plugins(materials::install_output_materials)
        .add_plugins(materials::install_surface_material)
        .add_plugins(MaterialPlugin::<glow::GlowMaterial>::default())
        .add_plugins(draw_order::DrawOrderPlugin)
        .add_plugins(field_view::FieldPlugin)
        .add_plugins(overworld::OverworldPlugin)
        .init_asset::<GameAudio>()
        .init_asset::<field_audio::FieldSource>()
        .init_resource::<audio::MenuSounds>()
        .init_asset::<movie::MovieAudio>()
        .add_systems(
            PreUpdate,
            (
                gather_input.run_if(not(resource_exists::<saves::ScenarioInput>)),
                field_audio::acknowledge,
            )
                .after(bevy::input::InputSystems),
        )
        .add_systems(
            FixedPreUpdate,
            gather_input
                .after(bevy::input::InputSystems)
                .run_if(resource_exists::<saves::ScenarioInput>),
        )
        .add_systems(
            FixedPostUpdate,
            (saves::shortcuts, movie::controls)
                .chain()
                .before(saves::scenario_consumed)
                .run_if(resource_exists::<saves::ScenarioInput>),
        )
        .add_systems(Startup, (setup, glow::setup, display::initialize).chain())
        .add_systems(PostUpdate, loading::black_hold)
        .add_systems(
            PostUpdate,
            timing::prepare_draws
                .after(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        )
        .add_systems(Update, audio::check)
        .add_systems(
            Update,
            saves::release_frame.after(field_view::FieldPreparation),
        )
        .add_systems(
            FixedUpdate,
            (
                timing::advance_clock,
                boot::advance,
                advance.run_if(dungeons::running),
                new_game::advance
                    .run_if(dungeons::running)
                    .run_if(battle::field_running),
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                saves::update.run_if(dungeons::running),
                new_game::enter,
                new_game::skip_test_battles.run_if(dungeons::running),
                (
                    new_game::transition
                        .run_if(dungeons::running)
                        .run_if(battle::field_running),
                    session_screen::update,
                )
                    .chain(),
                scene::bind_animated,
                prepare_field,
                update_materials,
                animate_field,
                skin_bounds,
                field_camera,
                glow::update,
                timing::prepare,
                boot::update,
                movie::controls.run_if(not(resource_exists::<saves::ScenarioInput>)),
                movie::update,
                new_game::movie_handoff,
                start_audio,
                field_audio::update.run_if(battle::field_running),
                layout,
            )
                .chain(),
        );
    app.add_systems(
        Update,
        field_ui::transition_failure.after(new_game::transition),
    );
    dungeons::install(&mut app, capture_only);
    session_screen::install(&mut app);
    credits::install(&mut app);
    testing::install(&mut app);
    if !capture_only {
        audio_output::install(&mut app, silent)?;
    } else {
        app.add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            resonance_game::clock::UPDATE_STEP,
        ));
    }
    audio::validate_startup(&app, silent, capture_only)?;
    bevy::asset::embedded_asset!(app, "title_glow.wgsl");
    let render_diagnostics = app.world().resource::<diagnostics::Diagnostics>().clone();
    app.get_sub_app_mut(bevy::render::RenderApp)
        .context("render application unavailable")?
        .insert_resource(render_ready)
        .insert_resource(render_diagnostics)
        .add_systems(
            bevy::render::Render,
            timing::rendered.in_set(bevy::render::RenderSystems::Cleanup),
        );
    field_warm::install(&mut app);
    battle::install(&mut app);
    game_over::install(&mut app)?;
    renderer::configure(&mut app);
    Ok((app, recording))
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
    // Art and vertex colors are combined in encoded color space.
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
        FieldOverlayCamera,
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
        field.load(scene, &server, &mut commands);
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
    mut music: ResMut<PendingAudio>,
    mut assets: ResMut<Assets<GameAudio>>,
    mut sounds: ResMut<audio::MenuSounds>,
    ready: Res<timing::Ready>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
    new_game: Option<Res<new_game::Session>>,
    game_over: Option<Res<session_screen::GameOver>>,
) {
    if new_game.is_some() || game_over.is_some() || movie.active || boot.active() || !ready.0 {
        return;
    }
    if let Some(music) = music.0.take() {
        let (source, control) = music.session();
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
    let Some((position, target)) = camera.sample(events.0.tick().saturating_sub(2), track) else {
        return;
    };
    for mut camera in &mut cameras {
        *camera = Transform::from_translation(Vec3::from_array(position))
            .looking_at(Vec3::from_array(target), Vec3::Z);
    }
}

fn gather_input(
    input: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut pending: ResMut<PendingInput>,
    dungeons: Option<Res<dungeons::Menu>>,
) {
    if dungeons.is_some_and(|menu| menu.blocked()) {
        *pending = PendingInput::default();
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
    diagnostics: Res<crate::diagnostics::Diagnostics>,
    mut exit: MessageWriter<AppExit>,
    mut menu: ResMut<Menu>,
    clock: Res<Clock>,
    mut events: Option<ResMut<Events>>,
    mut pending: ResMut<PendingInput>,
    ready: Res<timing::Ready>,
    sounds: Res<audio::MenuSounds>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
    new_game: Option<Res<new_game::Session>>,
    loading: Option<Res<loading::Pending>>,
    load_menu: Option<Res<saves::title::LoadMenu>>,
    game_over: Option<Res<game_over::Active>>,
    scenario: Option<ResMut<saves::ScenarioInput>>,
) {
    if game_over.is_some()
        || new_game.is_some()
        || game_over.is_some()
        || load_menu.is_some()
        || loading.is_some()
        || movie.active
        || boot.active()
        || !ready.0
    {
        return;
    }
    let previous_selection = menu.0.selected;
    let input = pending.consume(clock.0);
    if let Some(mut scenario) = scenario {
        scenario.acknowledge_input();
    }
    if let Some(events) = &mut events
        && let Err(error) = events.0.step()
    {
        if diagnostics.0.report("title script", error).is_err() {
            exit.write(AppExit::error());
            return;
        }
        commands.remove_resource::<Events>();
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
    diagnostics: Res<crate::diagnostics::Diagnostics>,
    mut exit: MessageWriter<AppExit>,
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
        let Some(mut material) = materials.get_mut(&handle.0) else {
            if diagnostics
                .0
                .report("title layout", anyhow::anyhow!("missing title material"))
                .is_err()
            {
                exit.write(AppExit::error());
                return;
            }
            continue;
        };
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

#[cfg(test)]
mod input_tests {
    use super::*;

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
