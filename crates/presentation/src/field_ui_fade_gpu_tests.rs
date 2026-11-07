//! Opaque fades must cover every sample of the aligned overlay viewport.
use super::*;
use crate::anti_aliasing::{
    AntiAliasingCapabilities, AntiAliasingPlugin, AntiAliasingSettings, SceneMsaa,
};
use bevy::{
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use resonance_content::menu_data::AntiAliasing;

#[derive(Resource, Default)]
struct Capture(Option<Image>);

#[test]
#[ignore = "requires a GPU; checks full-screen fade coverage with the production camera alignment"]
fn full_screen_fades_cover_aligned_viewport_with_msaa() -> Result<()> {
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins())
        .add_plugins(AntiAliasingPlugin)
        .init_resource::<Capture>();
    super::super::install(&mut app);
    let began = std::time::Instant::now();
    while app.plugins_state() == bevy::app::PluginsState::Adding {
        anyhow::ensure!(began.elapsed().as_secs() < 180, "fade GPU setup timed out");
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
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
            640,
            448,
            TextureFormat::Bgra8Unorm,
            None,
        ));
    let (_, material) = app
        .world_mut()
        .resource_scope(|world, mut images: Mut<Assets<Image>>| {
            surface(&mut world.resource_mut::<Assets<Surface>>(), &mut images)
        });
    let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(
        batch(
            &resonance_events::GameWorld::default(),
            crate::Resolution::default(),
        )
        .mesh([1, 1]),
    );
    app.world_mut()
        .spawn((Mesh2d(mesh.clone()), MeshMaterial2d(material)));
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Tonemapping::Linear,
            SceneMsaa,
            crate::camera::overlay_alignment(),
            RenderTarget::Image(target.clone().into()),
            Camera {
                clear_color: Color::WHITE.into(),
                ..default()
            },
        ))
        .id();
    for (width, height) in [(640, 480), (1279, 719)] {
        let resolution = crate::Resolution { width, height };
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .get_mut(&target)
            .unwrap()
            .resize(Extent3d {
                width,
                height: resolution.scene_height(),
                depth_or_array_layers: 1,
            });
        *app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .get_mut(&mesh)
            .unwrap() = batch(&resonance_events::GameWorld::default(), resolution).mesh([1, 1]);
        app.world_mut()
            .entity_mut(camera)
            .insert(crate::battle::overlay_projection(resolution));
        for &mode in &modes {
            app.world_mut().resource_mut::<AntiAliasingSettings>().0 = mode;
            for _ in 0..20 {
                app.update();
                crate::playthrough::check_exit(&app)?;
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
                anyhow::ensure!(began.elapsed().as_secs() < 180, "fade readback timed out");
                app.update();
                crate::playthrough::check_exit(&app)?;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let image = app
                .world_mut()
                .resource_mut::<Capture>()
                .0
                .take()
                .unwrap()
                .try_into_dynamic()?
                .to_rgba8();
            for (x, y, pixel) in image.enumerate_pixels() {
                anyhow::ensure!(
                    pixel.0 == [0, 0, 0, 255],
                    "{mode:?}, {resolution:?}: opaque fade leaked at {x},{y}: {pixel:?}"
                );
            }
            println!("{mode:?}, {resolution:?}: opaque fade covers the viewport");
        }
    }
    Ok(())
}
