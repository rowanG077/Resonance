//! Opaque fades must cover every sample of the aligned overlay viewport.
use super::*;
use crate::anti_aliasing::{
    AntiAliasingCapabilities, AntiAliasingPlugin, AntiAliasingSettings, SceneMsaa,
};
use crate::native::MeshMaterial2d;
use bevy::camera::RenderTarget;
use resonance_content::menu_data::AntiAliasing;

#[test]
#[ignore = "requires a GPU; checks full-screen fade coverage with the production camera alignment"]
fn full_screen_fades_cover_aligned_viewport_with_msaa() -> Result<()> {
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins())
        .add_plugins(AntiAliasingPlugin);
    super::super::install(&mut app);
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            640,
            448,
            TextureFormat::Bgra8Unorm,
            None,
        ));
    crate::test_support::native_scene(&mut app, target.clone());
    crate::test_support::finish_app(&mut app)?;
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
    let (_, material) = app
        .world_mut()
        .resource_scope(|world, mut images: Mut<Assets<Image>>| {
            surface(&mut world.resource_mut::<Assets<Surface>>(), &mut images)
        });
    let geometry = batch(
        &resonance_events::GameWorld::default(),
        crate::Resolution::default(),
    )
    .geometry([1, 1]);
    app.world_mut()
        .spawn((geometry.clone(), MeshMaterial2d(material)));
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
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
        geometry
            .replace(batch(&resonance_events::GameWorld::default(), resolution).vertices([1, 1]));
        app.world_mut()
            .entity_mut(camera)
            .insert(crate::battle::overlay_projection(resolution));
        for &mode in &modes {
            app.world_mut().resource_mut::<AntiAliasingSettings>().0 = mode;
            for _ in 0..20 {
                app.update();
                crate::playthrough::check_exit(&app)?;
            }
            let image = crate::test_support::capture_image(&mut app, target.clone())?
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
