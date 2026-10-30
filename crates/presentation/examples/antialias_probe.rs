//! GPU regression: spatial coverage, scene/UI composition and frame independence.
use anyhow::{Result, ensure};
use bevy::{
    app::ScheduleRunnerPlugin,
    camera::{CameraOutputMode, RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    image::ImageSampler,
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        camera::TemporalJitter,
        render_resource::{BlendState, Extent3d, PipelineCache, TextureDimension, TextureFormat},
        settings::{Backends, RenderCreation, WgpuSettings, WgpuSettingsPriority},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
};
use resonance_content::menu_data::AntiAliasing;
use resonance_presentation::anti_aliasing::{
    AntiAliasingCapabilities, AntiAliasingPlugin, AntiAliasingSettings,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const SIZE: UVec2 = UVec2::new(192, 144);
const TRIANGLE: [[f32; 3]; 3] = [[-0.85, -0.55, 0.], [0.87, -0.42, 0.], [0.31, 0.65, 0.]];
#[derive(Resource)]
struct Probe {
    choices: Vec<AntiAliasing>,
    directory: PathBuf,
    started: Instant,
    target: Handle<Image>,
    shots: Vec<Image>,
    frames: u32,
    pending: bool,
}
#[derive(Resource, Clone, Default)]
struct Ready(Arc<AtomicBool>);
fn main() -> Result<()> {
    let directory = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "/tmp/resonance-antialias-probe".into());
    std::fs::create_dir_all(&directory)?;
    let mut app = App::new();
    let ready = Ready::default();
    app.insert_resource(ready.clone())
        .insert_resource(Probe {
            choices: Vec::new(),
            directory,
            started: Instant::now(),
            target: Handle::default(),
            shots: Vec::new(),
            frames: 0,
            pending: false,
        })
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                        backends: Some(if cfg!(target_os = "macos") {
                            Backends::METAL
                        } else if cfg!(target_os = "windows") {
                            Backends::DX12
                        } else {
                            Backends::VULKAN
                        }),
                        force_fallback_adapter: !cfg!(target_os = "macos"),
                        priority: WgpuSettingsPriority::WebGPU,
                        ..default()
                    })),
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::gilrs::GilrsPlugin>(),
        )
        .add_plugins((
            AntiAliasingPlugin,
            ScheduleRunnerPlugin::run_loop(Duration::from_millis(10)),
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, capture);
    app.sub_app_mut(RenderApp)
        .insert_resource(ready)
        .add_systems(Render, ready_to_capture.in_set(RenderSystems::Cleanup));
    ensure!(app.run() == AppExit::Success, "antialias probe failed");
    Ok(())
}
fn ready_to_capture(cache: Res<PipelineCache>, ready: Res<Ready>) {
    ready
        .0
        .store(cache.waiting_pipelines().count() == 0, Ordering::Release);
}
fn setup(
    mut commands: Commands,
    mut probe: ResMut<Probe>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ui: ResMut<Assets<ColorMaterial>>,
) {
    probe.target = images.add(Image::new_target_texture(
        SIZE.x,
        SIZE.y,
        TextureFormat::Bgra8UnormSrgb,
        None,
    ));
    let target = RenderTarget::Image(probe.target.clone().into());
    let mut triangle = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    triangle.insert_attribute(Mesh::ATTRIBUTE_POSITION, TRIANGLE.to_vec());
    triangle.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 3]);
    triangle.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; 3]);
    commands.spawn((
        Mesh3d(meshes.add(triangle)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            ..default()
        })),
    ));
    // A texture edge inside a quad cannot be fixed by geometry MSAA alone.
    let mut cutout = Image::new_fill(
        Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 255, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    cutout.data = Some(
        (0..256)
            .flat_map(|i| [255, 255, 255, if i % 16 + i / 16 > 13 { 255 } else { 0 }])
            .collect(),
    );
    cutout.sampler = ImageSampler::nearest();
    let cutout = images.add(cutout);
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(0.35, 0.35))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0., 0., 1.),
            base_color_texture: Some(cutout),
            alpha_mode: AlphaMode::Mask(0.5),
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_xyz(-0.62, 0.25, 0.),
    ));
    commands.spawn((
        Camera3d::default(),
        Msaa::Off,
        Tonemapping::None,
        target.clone(),
        Camera {
            clear_color: Color::BLACK.into(),
            output_mode: CameraOutputMode::Skip,
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: 2.,
                height: 1.5,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0., 0., 10.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let pixels: Vec<u8> = (0..32)
        .flat_map(|i| {
            if (i + i / 8) % 2 == 0 {
                [0, 255, 0, 255]
            } else {
                [0, 0, 0, 255]
            }
        })
        .collect();
    let mut glyph = Image::new_fill(
        Extent3d {
            width: 8,
            height: 4,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    glyph.data = Some(pixels);
    glyph.sampler = ImageSampler::nearest();
    let glyph = images.add(glyph);
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(24., 12.))),
        MeshMaterial2d(ui.add(ColorMaterial::from(glyph))),
        Transform::from_xyz(-72., 58., 0.),
        RenderLayers::layer(1),
    ));
    commands.spawn((
        Camera2d,
        Msaa::Off,
        Tonemapping::None,
        target,
        RenderLayers::layer(1),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::REPLACE),
                clear_color: ClearColorConfig::None,
            },
            ..default()
        },
    ));
}
fn capture(
    mut commands: Commands,
    mut probe: ResMut<Probe>,
    ready: Res<Ready>,
    mut choice: ResMut<AntiAliasingSettings>,
    caps: Res<AntiAliasingCapabilities>,
    jitter: Query<&TemporalJitter>,
    mut exit: MessageWriter<AppExit>,
) {
    assert!(
        jitter.is_empty(),
        "spatial AA must never jitter the projection"
    );
    if probe.started.elapsed() > Duration::from_secs(90) {
        exit.write(AppExit::error());
        return;
    }
    if probe.choices.is_empty() {
        probe.choices = caps.available_modes().to_vec();
        let last = *probe.choices.last().unwrap();
        probe.choices.extend([last, AntiAliasing::Off, last]);
    }
    if probe.pending || probe.shots.len() == probe.choices.len() {
        return;
    }
    choice.0 = probe.choices[probe.shots.len()];
    if !ready.0.load(Ordering::Acquire) {
        probe.frames = 0;
        return;
    }
    probe.frames += 1;
    if probe.frames < 20 {
        return;
    }
    probe.pending = true;
    commands
        .spawn(Screenshot::image(probe.target.clone()))
        .observe(
            |shot: On<ScreenshotCaptured>,
             mut probe: ResMut<Probe>,
             mut exit: MessageWriter<AppExit>| {
                probe.shots.push(shot.image.clone());
                probe.frames = 0;
                probe.pending = false;
                if probe.shots.len() == probe.choices.len() {
                    match validate(&probe) {
                        Ok(()) => {
                            exit.write(AppExit::Success);
                        }
                        Err(error) => {
                            eprintln!("{error:#}");
                            exit.write(AppExit::error());
                        }
                    }
                }
            },
        );
}
fn validate(probe: &Probe) -> Result<()> {
    let shots = probe
        .shots
        .iter()
        .cloned()
        .map(|i| i.try_into_dynamic().map(|d| d.to_rgba8()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let reference = spatial_reference();
    let mut baseline_error = 0.;
    let mut report = Vec::new();
    let mut errors = Vec::new();
    for (index, (choice, image)) in probe.choices.iter().zip(&shots).enumerate() {
        ensure!(
            image.dimensions() == (SIZE.x, SIZE.y),
            "output dimensions changed"
        );
        image.save(probe.directory.join(format!("{index}-{choice:?}.png")))?;
        let intermediate = image
            .enumerate_pixels()
            .filter(|(x, y, p)| {
                !((12..36).contains(x) && (8..20).contains(y)) && p.0[0] > 0 && p.0[0] < 255
            })
            .count();
        let error = image
            .pixels()
            .zip(&reference)
            .map(|(pixel, expected)| (f32::from(pixel.0[0]) - expected).abs())
            .sum::<f32>()
            / reference.len() as f32;
        if *choice == AntiAliasing::Off {
            baseline_error = error;
        } else {
            ensure!(
                error < baseline_error,
                "{choice:?} worsened spatial-reference error: {error} >= {baseline_error}"
            );
        }
        if *choice == AntiAliasing::Off {
            ensure!(intermediate == 0, "baseline must be aliased");
        } else {
            ensure!(
                intermediate > 100,
                "{choice:?} did not smooth geometry ({intermediate} edge pixels)"
            );
        }
        for y in 9..19 {
            for x in 13..35 {
                ensure!(
                    image.get_pixel(x, y) == shots[0].get_pixel(x, y),
                    "{choice:?} altered UI at {x},{y}"
                );
            }
        }
        ensure!(
            image.get_pixel(100, 75) == shots[0].get_pixel(100, 75),
            "{choice:?} blurred a flat interior"
        );
        errors.push(error);
        if let Some(previous) = probe.choices[..index]
            .iter()
            .position(|earlier| earlier == choice)
        {
            ensure!(
                image == &shots[previous],
                "{choice:?} changed after revisiting a static scene"
            );
        }
        report.push(serde_json::json!({"mode":choice,"edge_coverage_pixels":intermediate,"reference_mae":error}));
    }
    if let (Some(s2x), Some(msaa2)) = (
        probe
            .choices
            .iter()
            .position(|mode| *mode == AntiAliasing::SmaaS2x),
        probe
            .choices
            .iter()
            .position(|mode| *mode == AntiAliasing::Msaa2),
    ) {
        ensure!(
            errors[s2x] < errors[msaa2],
            "S2x must improve coverage over plain 2x MSAA"
        );
        let texture_edges = |index: usize| {
            shots[index]
                .enumerate_pixels()
                .filter(|(x, y, p)| {
                    (18..58).contains(x) && (28..68).contains(y) && p.0[2] > 0 && p.0[2] < 255
                })
                .count()
        };
        ensure!(
            texture_edges(s2x) > texture_edges(msaa2),
            "S2x must smooth alpha cutout edges beyond plain MSAA"
        );
    }
    std::fs::write(
        probe.directory.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "Spatial AA coverage, unfiltered UI, live switching and frame-independence checks passed"
    );
    Ok(())
}

// Independent 64-sample spatial reference in linear light. Smoothing must
// improve coverage, rather than merely increase the number of blended pixels.
fn spatial_reference() -> Vec<f32> {
    let vertices = TRIANGLE.map(|p| Vec2::new(p[0], p[1]));
    (0..SIZE.y)
        .flat_map(|y| {
            (0..SIZE.x).map(move |x| {
                let mut covered = 0;
                for sy in 0..8 {
                    for sx in 0..8 {
                        let p = Vec2::new(
                            (x as f32 + (sx as f32 + 0.5) / 8.) / SIZE.x as f32 * 2. - 1.,
                            0.75 - (y as f32 + (sy as f32 + 0.5) / 8.) / SIZE.y as f32 * 1.5,
                        );
                        if (0..3).all(|i| {
                            (vertices[(i + 1) % 3] - vertices[i]).perp_dot(p - vertices[i]) >= 0.
                        }) {
                            covered += 1;
                        }
                    }
                }
                let linear = covered as f32 / 64.;
                let encoded = if linear <= 0.0031308 {
                    linear * 12.92
                } else {
                    1.055 * linear.powf(1. / 2.4) - 0.055
                };
                encoded * 255.
            })
        })
        .collect()
}
