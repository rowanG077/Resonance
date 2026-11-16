//! Synthetic rendering smoke test of the native frame renderer, headless or
//! windowed, without game assets. One frame mirrors the game's display: a scene
//! image (surface patches on the left, movie text and blending on the right)
//! shown through the output conversion into an image or the window.
use anyhow::{Context as ContextExt, Result, ensure};
use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    asset::RenderAssetUsages,
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        render_resource::{
            CachedPipelineState, Extent3d, PipelineCache, PipelineDescriptor, PollType,
            TextureDimension, TextureFormat,
        },
        renderer::{RenderAdapterInfo, RenderDevice},
        settings::{Backends, RenderCreation, WgpuSettings, WgpuSettingsPriority},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
};
use resonance_events::effect::Blend;
use resonance_presentation::{
    MeshMaterial2d, MeshMaterial3d, TitleOutput, TitleSurface, TitleText,
    anti_aliasing::AntiAliasingPlugin, install_output_materials, install_surface_material,
    render_plugins, set_display_targets,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const BACKEND: Backends = if cfg!(target_os = "macos") {
    Backends::METAL
} else if cfg!(target_os = "windows") {
    Backends::DX12
} else {
    Backends::VULKAN
};
const REQUIRE_CPU: bool = !cfg!(target_os = "macos");

const WIDTH: u32 = 256;
const HEIGHT: u32 = 128;

#[derive(Resource)]
struct Smoke {
    directory: PathBuf,
    started: Instant,
    /// The image or window the output camera draws into.
    output: Option<RenderTarget>,
    frames: u32,
    ready_frames: u32,
    capture_started: bool,
    overlay_shader_delay: u32,
    delayed_shader: Option<Shader>,
    shader_was_delayed: bool,
    require_cpu: bool,
    backend: Backends,
    windowed: bool,
}

#[derive(Clone, Default)]
struct PipelineStatus {
    ready: bool,
    overlay_seen: bool,
    pending: Vec<String>,
}

#[derive(Resource, Clone)]
struct Readiness {
    overlay_shader: Handle<Shader>,
    required_shaders: Vec<(&'static str, Handle<Shader>)>,
    status: Arc<Mutex<PipelineStatus>>,
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = args.next().map(PathBuf::from).context(
        "usage: ci_render OUTPUT_DIRECTORY [OVERLAY_SHADER_DELAY_FRAMES] [--allow-hardware] [--windowed]",
    )?;
    let mut overlay_shader_delay = None;
    let mut allow_hardware = false;
    let backend = BACKEND;
    let mut windowed = false;
    for value in args {
        if value == "--allow-hardware" {
            allow_hardware = true;
        } else if value == "--windowed" {
            windowed = true;
        } else {
            ensure!(
                overlay_shader_delay.is_none(),
                "unexpected argument: {value:?}"
            );
            overlay_shader_delay = Some(
                value
                    .to_string_lossy()
                    .parse::<u32>()
                    .context("overlay shader delay must be a number of frames")?,
            );
        }
    }
    // Wine/Proton does not implement WARP. Local DX12 checks can explicitly use
    // its hardware adapter; CI omits this flag and still requires the CPU driver.
    let require_cpu = REQUIRE_CPU && !allow_hardware;
    std::fs::create_dir_all(&directory)?;
    let settings = WgpuSettings {
        backends: Some(backend),
        // Exercise the shipped Metal backend on macOS, and software adapters
        // on Linux/Windows where hosted runners do not supply a hardware GPU.
        force_fallback_adapter: require_cpu,
        // The portable feature set, on software or virtual CI adapters.
        priority: WgpuSettingsPriority::WebGPU,
        // Do not allow WGPU_ADAPTER_NAME to override the adapter policy.
        adapter_name: None,
        ..default()
    };
    let mut app = App::new();
    app.insert_resource(Smoke {
        directory,
        started: Instant::now(),
        output: None,
        frames: 0,
        ready_frames: 0,
        capture_started: false,
        overlay_shader_delay: overlay_shader_delay.unwrap_or(0),
        delayed_shader: None,
        shader_was_delayed: false,
        require_cpu,
        backend,
        windowed,
    });
    let mut plugins = render_plugins(RenderPlugin {
        render_creation: RenderCreation::Automatic(Box::new(settings)),
        synchronous_pipeline_compilation: true,
        ..default()
    })
    .set(WindowPlugin {
        primary_window: windowed.then(|| Window {
            title: "Resonance renderer smoke test".into(),
            resolution: (WIDTH, HEIGHT).into(),
            resizable: false,
            ..default()
        }),
        exit_condition: ExitCondition::DontExit,
        ..default()
    })
    .set(ImagePlugin::default_nearest())
    .disable::<bevy::gilrs::GilrsPlugin>();
    if !windowed {
        plugins = plugins.disable::<bevy::winit::WinitPlugin>();
    }
    app.add_plugins(plugins)
        .add_plugins((
            install_surface_material,
            install_output_materials,
            AntiAliasingPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, capture);
    if !windowed {
        app.add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10)));
    }
    // The native frame's shaders: flat quads (movie text, overlay and output
    // conversion) first, which the delay test withholds, then the surfaces.
    let required_shaders = [
        ("flat", "native_flat.wesl"),
        ("surface vertex", "native_surface_vertex.wesl"),
        ("surface", "title_surface.wesl"),
    ]
    .map(|(name, path)| {
        (
            name,
            app.world()
                .resource::<AssetServer>()
                .load(format!("embedded://resonance_presentation/{path}")),
        )
    })
    .to_vec();
    let readiness = Readiness {
        overlay_shader: required_shaders[0].1.clone(),
        required_shaders,
        status: default(),
    };
    app.insert_resource(readiness.clone());
    app.sub_app_mut(RenderApp)
        .insert_resource(readiness)
        .add_systems(Render, wait_for_frame.after(RenderSystems::Render));
    let exit = app.run();
    ensure!(exit == AppExit::Success, "rendering smoke test failed");
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy injects the independently owned render resources."
)]
fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TitleSurface>>,
    mut movie_materials: ResMut<Assets<TitleText>>,
    mut output_materials: ResMut<Assets<TitleOutput>>,
    adapter: Res<RenderAdapterInfo>,
    mut smoke: ResMut<Smoke>,
) {
    // Require the intended backend; never fall back from Metal or use a no-op renderer.
    assert_eq!(
        Backends::from(adapter.backend),
        smoke.backend,
        "unexpected rendering backend: {:?}",
        &**adapter
    );
    if smoke.require_cpu {
        assert_eq!(
            format!("{:?}", adapter.device_type),
            "Cpu",
            "expected a CPU adapter: {:?}",
            &**adapter
        );
    }
    println!("Render smoke adapter: {:?}", &**adapter);
    std::fs::write(
        smoke.directory.join("adapter.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "name": adapter.name, "backend": format!("{:?}", adapter.backend),
            "device_type": format!("{:?}", adapter.device_type), "driver": adapter.driver,
            "driver_info": adapter.driver_info,
        }))
        .unwrap(),
    )
    .unwrap();

    if let Ok(expected) = std::env::var("RESONANCE_CI_WARP_VERSION") {
        assert_eq!(
            adapter.driver, expected,
            "CI must load the pinned WARP DLL beside ci_render.exe"
        );
    }

    // The scene stores encoded color, as the game's does; the output camera
    // converts it into an sRGB image or the window.
    let source = images.add(Image::new_target_texture(
        WIDTH,
        HEIGHT,
        TextureFormat::Bgra8Unorm,
        None,
    ));
    let output = if smoke.windowed {
        RenderTarget::default()
    } else {
        RenderTarget::Image(
            images
                .add(Image::new_target_texture(
                    WIDTH,
                    HEIGHT,
                    TextureFormat::Rgba8UnormSrgb,
                    None,
                ))
                .into(),
        )
    };
    commands.queue({
        let (source, output) = (source.clone(), output.clone());
        move |world: &mut World| set_display_targets(world, source, output)
    });
    let pixel = |rgba: [u8; 4]| {
        Image::new(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            rgba.to_vec(),
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        )
    };
    let fixed = |width: u32, height: u32| {
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: width as f32,
                height: height as f32,
            },
            ..OrthographicProjection::default_3d()
        })
    };
    // Left half: all four patches use the production shader with a colored
    // texture, vertex tint, actor lighting, and material tint. Pure green/blue
    // ramps catch swizzling weights before decoding; the normal control catches
    // an unconditional channel swap.
    let texture = images.add(pixel([17, 193, 241, 204]));
    let mut quad = Mesh::from(Rectangle::new(32., 32.));
    quad.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        vec![[128. / 255., 192. / 255., 224. / 255., 128. / 255.]; 4],
    );
    let quad = meshes.add(quad);
    for (position, red_channel, weights) in [
        ([-16., 16.], true, None),
        ([16., 16.], true, Some([0, 255, 0, 255])),
        ([-16., -16.], true, Some([0, 0, 255, 255])),
        ([16., -16.], false, Some([0, 255, 0, 255])),
    ] {
        commands.spawn((
            Mesh3d(quad.clone()),
            MeshMaterial3d(materials.add(TitleSurface {
                color: Some(texture.clone()),
                sampling: Some(texture.clone()),
                toon_ramp: weights.map(|rgba| images.add(pixel(rgba))),
                field_light: Vec4::new(0., 0., 100., 128.),
                shade_colors: [
                    Vec4::new(32., 64., 96., 255.) / 255.,
                    Vec4::new(80., 112., 144., 255.) / 255.,
                ],
                ambient_scale: Vec3::new(3., 1., 2.),
                tint: Vec4::new(0.5, 0.25, 0.75, 0.5),
                red_channel,
                ..default()
            })),
            Transform::from_xyz(position[0] - 64., position[1], 0.),
        ));
    }
    // Right half: saturate two additive draws, then darken: clamping only at
    // final output would leave 0.8 instead of 0.5.
    let blend_quad = meshes.add(Rectangle::new(8., 8.));
    for (z, tint, blend) in [
        (-2., Vec4::new(0.8, 0.8, 0.8, 1.), Blend::AdditiveUnscaled),
        (-1., Vec4::new(0.8, 0.8, 0.8, 1.), Blend::AdditiveUnscaled),
        (0., Vec4::new(0., 0., 0., 0.5), Blend::Alpha),
    ] {
        commands.spawn((
            Mesh3d(blend_quad.clone()),
            MeshMaterial3d(materials.add(TitleSurface {
                tint,
                blend: Some(blend),
                constant_color: true,
                depth_write: false,
                ..default()
            })),
            Transform::from_xyz(8., 0., z),
        ));
    }
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: Color::srgb_u8(16, 32, 48).into(),
            ..default()
        },
        RenderTarget::Image(source.clone().into()),
        fixed(WIDTH, HEIGHT),
        Transform::from_xyz(0., 0., 100.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // The UI overlay over the scene: the movie image shader for a 2x2 movie
    // frame on the right and a white square in the top-left corner.
    let movie_texture = images.add(Image::new(
        Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![
            224, 40, 72, 255, 48, 192, 88, 255, 56, 80, 208, 255, 192, 160, 32, 255,
        ],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    ));
    let overlay = RenderLayers::layer(1);
    for (source, size, position) in [
        (movie_texture, Vec2::new(96., 72.), Vec2::new(64., 0.)),
        (
            images.add(pixel([255; 4])),
            Vec2::splat(12.),
            Vec2::new(-112., 48.),
        ),
    ] {
        commands.spawn((
            Mesh2d(meshes.add(Rectangle::from_size(size))),
            MeshMaterial2d(movie_materials.add(TitleText {
                source,
                opacity_pulse: Vec4::new(1., 0., 0., 0.),
            })),
            Transform::from_translation(position.extend(0.)),
            overlay.clone(),
        ));
    }
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderTarget::Image(source.clone().into()),
        overlay,
        fixed(WIDTH, HEIGHT),
        Msaa::Off,
    ));
    // The output conversion, as the game's display draws it.
    let output_layer = RenderLayers::layer(2);
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(WIDTH as f32, HEIGHT as f32))),
        MeshMaterial2d(output_materials.add(TitleOutput {
            source,
            brightness: Vec4::new(1., 0., 0., 0.),
            overlay: None,
        })),
        output_layer.clone(),
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 2,
            clear_color: Color::BLACK.into(),
            ..default()
        },
        output.clone(),
        output_layer,
        fixed(WIDTH, HEIGHT),
        Msaa::Off,
    ));
    smoke.output = Some(output);
}

fn wait_for_frame(
    device: Res<RenderDevice>,
    pipelines: Res<PipelineCache>,
    readiness: Res<Readiness>,
) {
    // Headless rendering has no vsync to limit queued frames. Wait on the render
    // thread after submission so slow CPU adapters cannot build up a backlog
    // that outlives the screenshot and exceeds wgpu's shutdown timeout.
    device
        .poll(PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(60)),
        })
        .expect("wait for rendered frame");
    let mut status = PipelineStatus::default();
    let mut seen = vec![false; readiness.required_shaders.len()];
    for pipeline in pipelines.pipelines() {
        let label = match &pipeline.descriptor {
            PipelineDescriptor::RenderPipelineDescriptor(descriptor) => {
                for (seen, (_, shader)) in seen.iter_mut().zip(&readiness.required_shaders) {
                    *seen |= descriptor.vertex.shader == *shader
                        || descriptor
                            .fragment
                            .as_ref()
                            .is_some_and(|stage| stage.shader == *shader);
                }
                descriptor.label.as_deref()
            }
            PipelineDescriptor::ComputePipelineDescriptor(descriptor) => {
                descriptor.label.as_deref()
            }
            PipelineDescriptor::MeshPipelineDescriptor(descriptor) => descriptor.label.as_deref(),
        };
        if !matches!(pipeline.state, CachedPipelineState::Ok(_)) {
            status.pending.push(format!(
                "{}: {:?}",
                label.unwrap_or("unnamed pipeline"),
                pipeline.state
            ));
        }
    }
    status.overlay_seen = seen[0];
    for (seen, (name, _)) in seen.into_iter().zip(&readiness.required_shaders) {
        if !seen {
            status
                .pending
                .push(format!("{name}: pipeline not queued yet"));
        }
    }
    status.ready = status.overlay_seen && status.pending.is_empty();
    *readiness.status.lock().unwrap() = status;
}

fn capture(
    mut commands: Commands,
    mut smoke: ResMut<Smoke>,
    readiness: Res<Readiness>,
    mut shaders: ResMut<Assets<Shader>>,
    mut exit: MessageWriter<AppExit>,
) {
    let status = readiness.status.lock().unwrap().clone();
    if smoke.started.elapsed() > Duration::from_secs(60) {
        error!(
            "Rendering timed out: overlay pipeline seen={}, pending={:?}",
            status.overlay_seen, status.pending
        );
        exit.write(AppExit::error());
        return;
    }
    smoke.frames += 1;
    // The optional delay reproduces a cold sprite shader arriving after the old
    // fixed 20-frame warm-up. Shader removal/reinsertion exercises real cache retry.
    if smoke.overlay_shader_delay != 0 {
        if !smoke.shader_was_delayed {
            smoke.delayed_shader = shaders.remove(readiness.overlay_shader.id());
            smoke.shader_was_delayed = smoke.delayed_shader.is_some();
            return;
        }
        if smoke.frames < smoke.overlay_shader_delay {
            return;
        }
        if let Some(shader) = smoke.delayed_shader.take() {
            shaders
                .insert(readiness.overlay_shader.id(), shader)
                .unwrap();
            smoke.ready_frames = 0;
            return;
        }
    }
    if smoke.capture_started {
        return;
    }
    smoke.ready_frames = if status.ready {
        smoke.ready_frames + 1
    } else {
        0
    };
    // Two completed ready render frames allow extraction and bind-group updates
    // to settle before readback. A frame count alone says nothing about readiness.
    if smoke.ready_frames >= 2 {
        smoke.capture_started = true;
        println!(
            "Capturing after {} frames with all pipelines ready",
            smoke.frames
        );
        let screenshot = match smoke.output.clone().unwrap() {
            RenderTarget::Image(image) => Screenshot::image(image.handle),
            _ => Screenshot::primary_window(),
        };
        commands.spawn(screenshot).observe(
            |event: On<ScreenshotCaptured>, smoke: Res<Smoke>, mut exit: MessageWriter<AppExit>| {
                match check_image(&event.image, &smoke.directory) {
                    Ok(()) => {
                        println!("Rendering surface, movie and output pixel checks passed");
                        exit.write(AppExit::Success);
                    }
                    Err(error) => {
                        error!("Rendering: {error:#}");
                        exit.write(AppExit::error());
                    }
                }
            },
        );
    }
}

fn check_image(image: &Image, directory: &std::path::Path) -> Result<()> {
    let pixels = image.clone().try_into_dynamic()?.to_rgba8();
    pixels.save(directory.join("render.png"))?;
    ensure!(
        pixels.dimensions() == (WIDTH, HEIGHT),
        "wrong render dimensions: {:?}",
        pixels.dimensions()
    );
    for (x, y, expected) in EXPECTED {
        // Sample patches away from edges; tolerate small backend rounding differences.
        for py in y - 2..=y + 2 {
            for px in x - 2..=x + 2 {
                let actual = pixels.get_pixel(px, py).0;
                ensure!(
                    actual
                        .iter()
                        .zip(expected)
                        .all(|(&a, b)| a.abs_diff(b) <= 2),
                    "pixel ({px}, {py}): expected {expected:?}, got {actual:?}"
                );
            }
        }
    }
    Ok(())
}

/// Output pixels: the scene stores the shaders' encoded color and the output
/// conversion reproduces it exactly, opaque.
const EXPECTED: [(u32, u32, [u8; 4]); 11] = [
    (48, 48, [4, 4, 4, 255]),     // Unlit: red texture × red vertex × red tint.
    (80, 48, [16, 16, 16, 255]),  // Green ramp decodes the second shade.
    (48, 80, [52, 52, 52, 255]),  // Blue ramp decodes full highlight.
    (80, 80, [16, 64, 191, 255]), // Same green ramp without RED_CHANNEL.
    (16, 16, [255, 255, 255, 255]), // UI overlay.
    (112, 112, [1, 4, 8, 255]),   // Scene clear.
    (168, 46, [224, 40, 72, 255]), // Movie frame, unchanged by the conversion.
    (216, 46, [48, 192, 88, 255]),
    (168, 82, [56, 80, 208, 255]),
    (216, 82, [192, 160, 32, 255]),
    (136, 64, [127, 127, 127, 255]), // Additive saturation before alpha blending.
];
