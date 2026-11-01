use super::*;

#[derive(Resource, Default)]
struct Captures(Vec<Image>);

fn render_native_scene(format: TextureFormat, mode: AntiAliasing) -> anyhow::Result<Vec<Image>> {
    let s2x = mode == AntiAliasing::SmaaS2x;
    use bevy::{
        camera::{CameraOutputMode, RenderTarget, visibility::RenderLayers},
        core_pipeline::tonemapping::Tonemapping,
        render::{
            RenderPlugin,
            pipelined_rendering::PipelinedRenderingPlugin,
            settings::{Backends, RenderCreation, WgpuSettings, WgpuSettingsPriority},
            view::screenshot::{Screenshot, ScreenshotCaptured},
        },
        window::ExitCondition,
    };
    const SIZE: u32 = 96;
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
            .disable::<bevy::log::LogPlugin>()
            .disable::<PipelinedRenderingPlugin>()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::gilrs::GilrsPlugin>(),
    )
    .add_plugins(bevy_post_process::msaa_writeback::MsaaWritebackPlugin)
    .init_resource::<Captures>();
    static LOGGING: std::sync::Once = std::sync::Once::new();
    LOGGING.call_once(|| {
        app.add_plugins(bevy::log::LogPlugin::default());
    });
    install(&mut app);
    let began = std::time::Instant::now();
    while app.plugins_state() == bevy::app::PluginsState::Adding {
        anyhow::ensure!(began.elapsed().as_secs() < 120, "S2x GPU setup timed out");
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    let caps = super::super::AntiAliasingCapabilities::detect(
        app.world()
            .resource::<bevy::render::renderer::RenderAdapter>(),
        app.world().resource::<RenderDevice>(),
    );
    anyhow::ensure!(
        !s2x || caps.supports(resonance_content::menu_data::AntiAliasing::SmaaS2x),
        "this test requires native 2x MSAA support"
    );
    let msaa = if s2x { Msaa::Sample2 } else { Msaa::Off };
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(SIZE, SIZE, format, None));
    // The S2x camera loads a background published by an earlier camera. Its
    // readable attachment must preserve that scene before drawing geometry.
    app.world_mut().spawn((
        Camera2d,
        msaa,
        Tonemapping::Linear,
        RenderTarget::Image(target.clone().into()),
        RenderLayers::layer(2),
        Camera {
            order: -1,
            clear_color: Color::srgb(1., 0., 0.).into(),
            output_mode: CameraOutputMode::Skip,
            ..default()
        },
    ));
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            msaa,
            Tonemapping::Linear,
            RenderTarget::Image(target.clone().into()),
            Camera {
                clear_color: ClearColorConfig::None,
                output_mode: CameraOutputMode::Skip,
                ..default()
            },
        ))
        .id();
    app.world_mut().entity_mut(camera).insert(Smaa(mode));
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(1., 1.));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            ..default()
        });
    app.world_mut().spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(0., 0., -2.).with_rotation(Quat::from_rotation_z(0.31)),
    ));
    let glyph = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(12., 12.));
    let green = app
        .world_mut()
        .resource_mut::<Assets<ColorMaterial>>()
        .add(Color::srgb(0., 1., 0.));
    let ui_layer = bevy::camera::visibility::RenderLayers::layer(1);
    app.world_mut().spawn((
        Mesh2d(glyph),
        MeshMaterial2d(green),
        Transform::from_xyz(-32., 32., 0.),
        ui_layer.clone(),
    ));
    app.world_mut().spawn((
        Camera2d,
        msaa,
        Tonemapping::Linear,
        RenderTarget::Image(target.clone().into()),
        ui_layer,
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
    for size in [UVec2::splat(SIZE), UVec2::new(128, 80)] {
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .get_mut(&target)
            .unwrap()
            .resize(size.to_extents());
        for _ in 0..40 {
            app.update();
            crate::playthrough::check_exit(&app)?;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let render = app.sub_app_mut(RenderApp).world_mut();
        let mut resources = render.query::<&ViewResources>();
        anyhow::ensure!(
            resources
                .iter(render)
                .any(|resources| resources.size == size
                    && resources.mode == Smaa(mode)
                    && resources.spatial.as_ref().is_none_or(|spatial| spatial
                        .calibrated
                        .load(std::sync::atomic::Ordering::Relaxed))),
            "{mode:?}: SMAA render resources were not prepared"
        );
        let count = app.world().resource::<Captures>().0.len();
        app.world_mut()
            .spawn(Screenshot::image(target.clone()))
            .observe(
                |shot: On<ScreenshotCaptured>, mut captures: ResMut<Captures>| {
                    captures.0.push(shot.image.clone())
                },
            );
        while app.world().resource::<Captures>().0.len() == count {
            anyhow::ensure!(
                began.elapsed().as_secs() < 120,
                "S2x native readback timed out"
            );
            app.update();
            crate::playthrough::check_exit(&app)?;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let image = app
            .world()
            .resource::<Captures>()
            .0
            .last()
            .unwrap()
            .clone()
            .try_into_dynamic()?
            .to_rgba8();
        anyhow::ensure!(image.dimensions() == (size.x, size.y), "S2x resize failed");
        anyhow::ensure!(
            image
                .get_pixel(size.x / 2, size.y / 2)
                .0
                .iter()
                .all(|channel| *channel >= 254),
            "{format:?} {size:?}: S2x failed to render opaque geometry: {:?}",
            image.get_pixel(size.x / 2, size.y / 2).0
        );
        anyhow::ensure!(
            image
                .pixels()
                .filter(|p| p.0[1] > 1 && p.0[1] < 254)
                .count()
                > 10,
            "{mode:?}: SMAA did not smooth the scene edge"
        );
        anyhow::ensure!(
            image.get_pixel(0, 0).0 == [255, 0, 0, 255],
            "S2x lost the scene clear color"
        );
        anyhow::ensure!(
            image.get_pixel(size.x / 2 - 32, size.y / 2 - 32).0 == [0, 255, 0, 255],
            "S2x overwrote UI composition"
        );
    }
    app.world_mut().entity_mut(camera).remove::<Smaa>();
    for _ in 0..4 {
        app.update();
        crate::playthrough::check_exit(&app)?;
    }
    let render = app.sub_app_mut(RenderApp).world_mut();
    anyhow::ensure!(
        render
            .query::<&ViewResources>()
            .iter(render)
            .next()
            .is_none(),
        "S2x retained attachments after disabling the mode"
    );
    Ok(app.world().resource::<Captures>().0.clone())
}
#[test]
#[ignore = "requires native 2x MSAA; validates S2x color space, loaded scenes, UI, resize and cleanup"]
fn native_s2x_preserves_loaded_scene_ui_resize_and_cleanup() -> anyhow::Result<()> {
    // Test independent, fixed-format cameras, as used by the game. The native
    // renderer does not change a camera's color format during its lifetime.
    compare_scene_formats(AntiAliasing::SmaaS2x)
}
#[test]
#[ignore = "requires a GPU; validates SMAA color space, loaded scenes, UI and resize"]
fn regular_smaa_preserves_scene_colors() -> anyhow::Result<()> {
    for mode in [
        AntiAliasing::SmaaLow,
        AntiAliasing::SmaaMedium,
        AntiAliasing::SmaaHigh,
        AntiAliasing::SmaaUltra,
    ] {
        compare_scene_formats(mode)?;
    }
    Ok(())
}
fn compare_scene_formats(mode: AntiAliasing) -> anyhow::Result<()> {
    let srgb = render_native_scene(TextureFormat::Bgra8UnormSrgb, mode)?;
    let unorm = render_native_scene(TextureFormat::Bgra8Unorm, mode)?;
    for (srgb, unorm) in srgb.into_iter().zip(unorm) {
        let srgb = srgb.try_into_dynamic()?.to_rgba8();
        let unorm = unorm.try_into_dynamic()?.to_rgba8();
        anyhow::ensure!(
            srgb.pixels().zip(unorm.pixels()).all(|(a, b)| a
                .0
                .into_iter()
                .zip(b.0)
                .all(|(a, b)| a.abs_diff(b) <= 1)),
            "{mode:?}: SMAA color-space mismatch between sRGB and encoded-unorm scene targets"
        );
    }
    Ok(())
}
#[test]
fn all_spatial_shader_variants_validate() {
    use bevy::shader::ShaderCache;
    let mut cache = ShaderCache::new((), crate::test_support::validate_shader);
    let mut shaders = Assets::<Shader>::default();
    for source in [
        include_str!("../smaa.wesl"),
        include_str!("../spatial_io.wesl"),
    ] {
        let shader = Shader::from_wesl(source, "s2x shader validation.wesl");
        let handle = shaders.add(shader.clone());
        cache.set_shader(handle.id(), shader);
        for mode in [
            AntiAliasing::SmaaLow,
            AntiAliasing::SmaaMedium,
            AntiAliasing::SmaaHigh,
            AntiAliasing::SmaaUltra,
            AntiAliasing::SmaaS2x,
        ] {
            for srgb in [false, true] {
                for phase in [
                    "SMAA_EDGE_DETECTION",
                    "SMAA_BLENDING_WEIGHT_CALCULATION",
                    "SMAA_NEIGHBORHOOD_BLENDING",
                    "SPLIT_SAMPLES",
                ] {
                    let mut defs = vec![phase.into(), Smaa(mode).preset().into()];
                    if mode == AntiAliasing::SmaaS2x {
                        defs.push("SMAA_S2X".into());
                    }
                    if srgb {
                        defs.push("SMAA_INPUT_SRGB".into());
                    }
                    cache
                        .get(0, handle.id(), &defs)
                        .unwrap_or_else(|error| panic!("{mode:?} {phase} sRGB={srgb}: {error:?}"));
                }
            }
        }
    }
}
