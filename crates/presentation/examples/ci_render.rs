//! Synthetic, headless rendering smoke test. No game assets or window system.
use anyhow::{Context as ContextExt, Result, ensure};
use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    asset::RenderAssetUsages,
    camera::{CameraOutputMode, RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    pbr::Material,
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        render_resource::{
            BlendState, CachedPipelineState, Extent3d, PipelineCache, PipelineDescriptor, PollType,
            TextureDimension, TextureFormat,
        },
        renderer::{RenderAdapterInfo, RenderDevice},
        settings::{Backends, RenderCreation, WgpuSettings, WgpuSettingsPriority},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    shader::ShaderRef,
    sprite_render::{Material2d, SpriteMeshMaterial},
    window::ExitCondition,
};
use resonance_presentation::{
    TitleOutput, TitleSurface, TitleText, install_output_materials, install_surface_material,
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

#[derive(Resource)]
struct Smoke {
    directory: PathBuf,
    started: Instant,
    target: Option<Handle<Image>>,
    movie_target: Option<Handle<Image>>,
    completed: u8,
    frames: u32,
    ready_frames: u32,
    capture_started: bool,
    overlay_shader_delay: u32,
    delayed_shader: Option<Shader>,
    shader_was_delayed: bool,
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
    let directory = args
        .next()
        .map(PathBuf::from)
        .context("usage: ci_render OUTPUT_DIRECTORY [OVERLAY_SHADER_DELAY_FRAMES]")?;
    let overlay_shader_delay = args
        .next()
        .map(|value| value.to_string_lossy().parse::<u32>())
        .transpose()
        .context("overlay shader delay must be a number of frames")?
        .unwrap_or(0);
    std::fs::create_dir_all(&directory)?;
    let settings = WgpuSettings {
        backends: Some(BACKEND),
        // Exercise the shipped Metal backend on macOS, and software adapters
        // on Linux/Windows where hosted runners do not supply a hardware GPU.
        force_fallback_adapter: REQUIRE_CPU,
        // This basic scene uses the portable feature set, avoiding requests for
        // unrelated experimental extensions on software or virtual adapters.
        priority: WgpuSettingsPriority::WebGPU,
        // Do not allow WGPU_ADAPTER_NAME to override the adapter policy.
        adapter_name: None,
        ..default()
    };
    let mut app = App::new();
    app.insert_resource(Smoke {
        directory,
        started: Instant::now(),
        target: None,
        movie_target: None,
        completed: 0,
        frames: 0,
        ready_frames: 0,
        capture_started: false,
        overlay_shader_delay,
        delayed_shader: None,
        shader_was_delayed: false,
    })
    .add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(ImagePlugin::default_nearest())
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(settings)),
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::gilrs::GilrsPlugin>(),
    )
    .add_plugins((install_surface_material, install_output_materials))
    .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10)))
    .add_systems(Startup, setup)
    .add_systems(Update, capture);
    let required_shaders = [
        ("overlay", SpriteMeshMaterial::vertex_shader()),
        ("surface", <TitleSurface as Material>::fragment_shader()),
        ("movie", TitleText::fragment_shader()),
        ("output", TitleOutput::fragment_shader()),
    ]
    .into_iter()
    .map(|(name, shader)| {
        let ShaderRef::Path(path) = shader else {
            anyhow::bail!("expected the {name} shader path");
        };
        Ok((name, app.world().resource::<AssetServer>().load(path)))
    })
    .collect::<Result<Vec<_>>>()?;
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
        BACKEND,
        "unexpected rendering backend: {:?}",
        &**adapter
    );
    if REQUIRE_CPU {
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

    let target = images.add(Image::new_target_texture(
        128,
        128,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    // All four patches use the production shader with a colored texture, vertex tint,
    // actor lighting, and material tint. Pure green/blue ramps catch swizzling weights
    // before decoding; the normal control catches an unconditional channel swap.
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
            Transform::from_xyz(position[0], position[1], 0.),
        ));
    }
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: Color::srgb_u8(16, 32, 48).into(),
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 128.,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0., 0., 100.).looking_at(Vec3::ZERO, Vec3::Y),
        Tonemapping::Linear,
        DebandDither::Disabled,
        Msaa::Off,
    ));
    // Give the sprite a real image dependency instead of the default fallback.
    // Material preparation must wait for this image before computing its size.
    let white = images.add(pixel([255; 4]));
    // A second camera exercises the sprite/UI overlay path and target compositing.
    commands.spawn((
        Sprite {
            image: white,
            custom_size: Some(Vec2::splat(12.)),
            ..default()
        },
        Transform::from_xyz(-48., 48., 0.),
        RenderLayers::layer(1),
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            // The overlay already shares the scene target; copy its stored alpha as-is.
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::REPLACE),
                clear_color: ClearColorConfig::None,
            },
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        RenderLayers::layer(1),
        Tonemapping::Linear,
        Msaa::Off,
    ));
    // Exercise the actual movie image shader and final output conversion in
    // sequence. The intermediate target stores encoded RGB just like production;
    // the final sRGB target must recover these authored color bytes unchanged.
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
    let movie_source = images.add(Image::new_target_texture(
        128,
        96,
        TextureFormat::Rgba8Unorm,
        None,
    ));
    let movie_output = images.add(Image::new_target_texture(
        128,
        96,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(96., 72.))),
        MeshMaterial2d(movie_materials.add(TitleText {
            source: movie_texture,
            opacity_pulse: Vec4::new(1., 0., 0., 0.),
        })),
        RenderLayers::layer(2),
    ));
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(128., 96.))),
        MeshMaterial2d(output_materials.add(TitleOutput {
            source: movie_source.clone(),
            brightness: Vec4::new(1., 0., 0., 0.),
        })),
        RenderLayers::layer(3),
    ));
    for (layer, target) in [(2, movie_source), (3, movie_output.clone())] {
        commands.spawn((
            Camera2d,
            Camera {
                order: layer as isize,
                clear_color: Color::BLACK.into(),
                ..default()
            },
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed {
                    width: 128.,
                    height: 96.,
                },
                ..OrthographicProjection::default_2d()
            }),
            RenderTarget::Image(target.into()),
            RenderLayers::layer(layer),
            Tonemapping::Linear,
            Msaa::Off,
        ));
    }
    smoke.movie_target = Some(movie_output);
    smoke.target = Some(target);
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
        for (movie, target) in [
            (false, smoke.target.clone().unwrap()),
            (true, smoke.movie_target.clone().unwrap()),
        ] {
            commands.spawn(Screenshot::image(target)).observe(
                move |event: On<ScreenshotCaptured>,
                      mut smoke: ResMut<Smoke>,
                      mut exit: MessageWriter<AppExit>| {
                    let result = if movie {
                        check_movie_image(&event.image, &smoke.directory)
                    } else {
                        check_image(&event.image, &smoke.directory)
                    };
                    match result {
                        Ok(()) => {
                            smoke.completed += 1;
                            if smoke.completed == 2 {
                                println!("Rendering surface and movie/output pixel checks passed");
                                exit.write(AppExit::Success);
                            }
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
}

fn check_movie_image(image: &Image, directory: &std::path::Path) -> Result<()> {
    let pixels = image.clone().try_into_dynamic()?.to_rgba8();
    pixels.save(directory.join("movie-output.png"))?;
    ensure!(
        pixels.dimensions() == (128, 96),
        "wrong movie/output dimensions"
    );
    for (x, y, expected) in [
        (40, 30, [224, 40, 72, 255]),
        (88, 30, [48, 192, 88, 255]),
        (40, 66, [56, 80, 208, 255]),
        (88, 66, [192, 160, 32, 255]),
        (8, 48, [0, 0, 0, 255]),
        (64, 6, [0, 0, 0, 255]),
    ] {
        for py in y - 2..=y + 2 {
            for px in x - 2..=x + 2 {
                let actual = pixels.get_pixel(px, py).0;
                ensure!(
                    actual
                        .iter()
                        .zip(expected)
                        .all(|(&a, b)| a.abs_diff(b) <= 2),
                    "movie/output pixel ({px}, {py}): expected {expected:?}, got {actual:?}"
                );
            }
        }
    }
    Ok(())
}

fn check_image(image: &Image, directory: &std::path::Path) -> Result<()> {
    let pixels = image.clone().try_into_dynamic()?.to_rgba8();
    pixels.save(directory.join("render.png"))?;
    ensure!(pixels.dimensions() == (128, 128), "wrong render dimensions");
    for (x, y, expected) in [
        // Fixed reference pixels after linear-to-sRGB output conversion. Alpha remains
        // linear: texture 204/255 × vertex 128/255 × tint 0.5 gives byte 51.
        (48, 48, [35, 35, 35, 51]), // Unlit: red texture × red vertex × red tint.
        (80, 48, [72, 72, 72, 51]), // Green ramp decodes the second shade.
        (48, 80, [125, 125, 125, 51]), // Blue ramp decodes full highlight.
        (80, 80, [72, 137, 225, 51]), // Same green ramp without RED_CHANNEL.
        (16, 16, [255, 255, 255, 255]),
        (112, 112, [16, 32, 48, 255]),
    ] {
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
