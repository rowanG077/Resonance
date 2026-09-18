//! Synthetic, headless rendering smoke test. No game assets or window system.
use anyhow::{Context, Result, ensure};
use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    asset::RenderAssetUsages,
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    prelude::*,
    render::{
        RenderPlugin,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        renderer::{RenderAdapterInfo, RenderDevice},
        settings::{Backends, RenderCreation, WgpuSettings, WgpuSettingsPriority},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
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
    let exit = App::new()
        .insert_resource(Smoke {
            directory,
            started: Instant::now(),
            target: None,
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
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10)))
        .add_systems(Startup, setup)
        .add_systems(Update, capture)
        .run();
    ensure!(exit == AppExit::Success, "rendering smoke test failed");
    Ok(())
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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
    // A generated texture exercises upload, UV orientation and nearest sampling.
    let texture = images.add(Image::new(
        Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(64., 64.))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(texture),
            unlit: true,
            ..default()
        })),
    ));
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
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        RenderLayers::layer(1),
        Tonemapping::None,
        Msaa::Off,
    ));
    smoke.target = Some(target);
}

fn capture(
    mut commands: Commands,
    mut smoke: ResMut<Smoke>,
    device: Res<RenderDevice>,
    mut exit: MessageWriter<AppExit>,
) {
    // Screenshot completion still needs polling when there is no window/event loop.
    device
        .poll(bevy::render::render_resource::PollType::Poll)
        .expect("poll renderer");
    if smoke.started.elapsed() > Duration::from_secs(60) {
        error!("Rendering timed out before screenshot completion");
        exit.write(AppExit::error());
        return;
    }
    smoke.frames += 1;
    if smoke.frames == 20 {
        commands
            .spawn(Screenshot::image(smoke.target.clone().unwrap()))
            .observe(
                |event: On<ScreenshotCaptured>,
                 smoke: Res<Smoke>,
                 mut exit: MessageWriter<AppExit>| {
                    match check_image(&event.image, &smoke.directory) {
                        Ok(()) => {
                            println!("Rendering pixel checks passed");
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
    ensure!(pixels.dimensions() == (128, 128), "wrong render dimensions");
    for (x, y, expected) in [
        (48, 48, [255, 0, 0, 255]),
        (80, 48, [0, 255, 0, 255]),
        (48, 80, [0, 0, 255, 255]),
        (80, 80, [255, 255, 0, 255]),
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
