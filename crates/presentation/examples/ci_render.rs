//! Synthetic, headless rendering smoke test. No game assets or window system.
use anyhow::{Context, Result, ensure};
use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    asset::RenderAssetUsages,
    camera::{CameraOutputMode, RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        render_resource::{BlendState, Extent3d, PollType, TextureDimension, TextureFormat},
        renderer::{RenderAdapterInfo, RenderDevice},
        settings::{Backends, RenderCreation, WgpuSettings, WgpuSettingsPriority},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
};
use resonance_presentation::{
    TitleOutput, TitleSurface, TitleText, install_output_materials, install_surface_material,
};
use std::{
    path::PathBuf,
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
}

fn main() -> Result<()> {
    let directory = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: ci_render OUTPUT_DIRECTORY")?;
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
    app.sub_app_mut(RenderApp)
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
        *adapter.0
    );
    if REQUIRE_CPU {
        assert_eq!(
            format!("{:?}", adapter.device_type),
            "Cpu",
            "expected a CPU adapter: {:?}",
            *adapter.0
        );
    }
    println!("Render smoke adapter: {:?}", *adapter.0);
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
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
    ));
    // A second camera exercises the sprite/UI overlay path and target compositing.
    commands.spawn((
        Sprite::from_color(Color::WHITE, Vec2::splat(12.)),
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
        Tonemapping::None,
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
            screen_offset: Vec2::ZERO,
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
            Tonemapping::None,
            Msaa::Off,
        ));
    }
    smoke.movie_target = Some(movie_output);
    smoke.target = Some(target);
}

fn wait_for_frame(device: Res<RenderDevice>) {
    // Headless rendering has no vsync to limit queued frames. Wait on the render
    // thread after submission so slow CPU adapters cannot build up a backlog
    // that outlives the screenshot and exceeds wgpu's shutdown timeout.
    device
        .poll(PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(60)),
        })
        .expect("wait for rendered frame");
}

fn capture(mut commands: Commands, mut smoke: ResMut<Smoke>, mut exit: MessageWriter<AppExit>) {
    if smoke.started.elapsed() > Duration::from_secs(60) {
        error!("Rendering timed out before screenshot completion");
        exit.write(AppExit::error());
        return;
    }
    smoke.frames += 1;
    if smoke.frames == 20 {
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
