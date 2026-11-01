use super::*;
use bevy::{
    camera::{CameraOutputMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    render::{
        RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        settings::{Backends, RenderCreation, WgpuSettings, WgpuSettingsPriority},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
};

#[test]
fn automatic_writeback_respects_clears_and_explicit_choices() {
    use MsaaWriteback::{Always, Auto, Off};
    use bevy::{core_pipeline::schedule::Core2d, ecs::schedule::ScheduleLabel};
    let mut app = App::new();
    app.add_systems(Update, configure_camera_loads);
    for index in [0, 1, 5] {
        // Model the render camera produced by extraction, including late views.
        let entity = app
            .world_mut()
            .spawn(ExtractedCamera {
                target: None,
                physical_viewport_size: None,
                physical_target_size: None,
                viewport: None,
                schedule: Core2d.intern(),
                order: index as isize,
                output_mode: CameraOutputMode::Skip,
                msaa_writeback: Auto,
                clear_color: ClearColorConfig::None,
                sorted_camera_index_for_target: index,
                exposure: 1.,
                hdr: false,
            })
            .id();
        for (clear, requested, expected) in [
            (ClearColorConfig::None, Auto, Auto),
            (ClearColorConfig::Default, Auto, Off),
            (ClearColorConfig::Custom(Color::BLACK), Auto, Off),
            (ClearColorConfig::Custom(Color::NONE), Auto, Off),
            (ClearColorConfig::None, Off, Off),
            (ClearColorConfig::Custom(Color::BLACK), Always, Always),
        ] {
            // Extraction copies the main camera's choice afresh each frame.
            let mut camera = app.world_mut().get_mut::<ExtractedCamera>(entity).unwrap();
            camera.clear_color = clear;
            camera.msaa_writeback = requested;
            app.update();
            assert_eq!(
                app.world()
                    .get::<ExtractedCamera>(entity)
                    .unwrap()
                    .msaa_writeback,
                expected,
                "camera index {index}, {clear:?}, {requested:?}"
            );
        }
    }
}

#[derive(Resource, Default)]
struct Capture(Option<Image>);

#[test]
#[ignore = "requires a GPU; checks replacement screens and overlays with MSAA"]
fn shared_targets_respect_clear_and_load_with_msaa() -> anyhow::Result<()> {
    let mut app = App::new();
    app.add_plugins(
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
                    priority: WgpuSettingsPriority::WebGPU,
                    ..default()
                })),
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<PipelinedRenderingPlugin>()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::gilrs::GilrsPlugin>(),
    )
    .add_plugins(AntiAliasingPlugin)
    .insert_resource(ClearColor(Color::srgb(1., 1., 0.)))
    .init_resource::<Capture>();
    let began = std::time::Instant::now();
    while app.plugins_state() == bevy::app::PluginsState::Adding {
        anyhow::ensure!(began.elapsed().as_secs() < 180, "GPU setup timed out");
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    app.update();
    let modes = app
        .world()
        .resource::<AntiAliasingCapabilities>()
        .available_modes()
        .iter()
        .copied()
        .filter(|mode| {
            matches!(
                mode,
                AntiAliasing::Off | AntiAliasing::Msaa2 | AntiAliasing::Msaa4 | AntiAliasing::Msaa8
            )
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        modes.iter().any(|mode| mode.samples() > 1),
        "this test requires native MSAA support"
    );
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            64,
            64,
            TextureFormat::Bgra8Unorm,
            None,
        ));
    let render_target = RenderTarget::Image(target.clone().into());
    app.world_mut().spawn((
        Camera3d::default(),
        Tonemapping::Linear,
        render_target.clone(),
        Camera {
            clear_color: Color::srgb(0., 1., 0.).into(),
            output_mode: CameraOutputMode::Skip,
            ..default()
        },
    ));
    let replacement = app
        .world_mut()
        .spawn((
            Camera2d,
            Tonemapping::Linear,
            render_target.clone(),
            RenderLayers::layer(1),
            Camera {
                order: 1,
                output_mode: CameraOutputMode::Skip,
                ..default()
            },
        ))
        .id();
    app.world_mut().spawn((
        Camera2d,
        Tonemapping::Linear,
        render_target.clone(),
        RenderLayers::layer(2),
        Camera {
            order: 2,
            clear_color: ClearColorConfig::None,
            output_mode: crate::renderer::COMPOSITED_OUTPUT,
            ..default()
        },
    ));
    for (layer, color, position, width) in [
        (1, Color::srgb(0., 0., 1.), Vec3::ZERO, 16.),
        (2, Color::srgb(1., 0., 0.), Vec3::new(-20., 20., 0.), 8.),
    ] {
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Rectangle::new(width, width));
        let material = app
            .world_mut()
            .resource_mut::<Assets<ColorMaterial>>()
            .add(color);
        app.world_mut().spawn((
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::from_translation(position),
            RenderLayers::layer(layer),
        ));
    }
    for mode in &modes {
        app.world_mut().resource_mut::<AntiAliasingSettings>().0 = *mode;
        // Reuse the same camera: replacement -> overlay -> replacement,
        // including a white field fade before a black movie/logo border.
        for (clear, expected) in [
            (ClearColorConfig::Custom(Color::WHITE), [255, 255, 255, 255]),
            (ClearColorConfig::Custom(Color::BLACK), [0, 0, 0, 255]),
            (ClearColorConfig::None, [0, 255, 0, 255]),
            (ClearColorConfig::Default, [255, 255, 0, 255]),
            (ClearColorConfig::Custom(Color::NONE), [0, 0, 0, 0]),
            (ClearColorConfig::None, [0, 255, 0, 255]),
        ] {
            app.world_mut()
                .get_mut::<Camera>(replacement)
                .unwrap()
                .clear_color = clear;
            check_capture(
                &mut app,
                &target,
                expected,
                format!("{mode:?}/{clear:?}"),
                began,
            )?;
        }
    }

    Ok(())
}

fn check_capture(
    app: &mut App,
    target: &Handle<Image>,
    background: [u8; 4],
    context: String,
    began: std::time::Instant,
) -> anyhow::Result<()> {
    // Allow camera extraction and shader preparation to settle.
    for _ in 0..20 {
        app.update();
        crate::playthrough::check_exit(app)?;
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    app.world_mut().resource_mut::<Capture>().0 = None;
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |shot: On<ScreenshotCaptured>, mut capture: ResMut<Capture>| {
                capture.0 = Some(shot.image.clone());
            },
        );
    while app.world().resource::<Capture>().0.is_none() {
        anyhow::ensure!(
            began.elapsed().as_secs() < 180,
            "{context}: readback timed out"
        );
        app.update();
        crate::playthrough::check_exit(app)?;
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let image = app
        .world_mut()
        .resource_mut::<Capture>()
        .0
        .take()
        .unwrap()
        .try_into_dynamic()?
        .to_rgba8();
    let (width, height) = image.dimensions();
    assert_eq!(
        image.get_pixel(0, 0).0,
        background,
        "{context}: clear/load background"
    );
    assert_eq!(
        image.get_pixel(width / 2, height / 2).0,
        [0, 0, 255, 255],
        "{context}: replacement surface"
    );
    assert_eq!(
        image.get_pixel(width / 2 - 20, height / 2 - 20).0,
        [255, 0, 0, 255],
        "{context}: UI overlay"
    );
    Ok(())
}
