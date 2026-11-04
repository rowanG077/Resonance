use super::*;
use crate::anti_aliasing::{AntiAliasingCapabilities, AntiAliasingPlugin, AntiAliasingSettings};
use bevy::{render::render_resource::CachedRenderPipelineId, sprite_render::Material2dPlugin};
use resonance_content::menu_data::AntiAliasing;

fn pipeline(app: &App, entity: Entity) -> CachedRenderPipelineId {
    let world = app.sub_app(RenderApp).world();
    crate::field_warm::draws(world.resource(), world.resource())
        .find(|(draw, _)| *draw == entity.into())
        .expect("queued transition fragments")
        .1
}

#[test]
#[ignore = "requires a GPU; checks pipeline reuse when the transition becomes visible"]
fn activation_reuses_the_warmed_fragment_pipeline() -> Result<()> {
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins())
        .add_plugins((
            AntiAliasingPlugin,
            Material2dPlugin::<crate::menu_backdrop::Material>::default(),
        ));
    app.world_mut()
        .resource_mut::<bevy::asset::io::embedded::EmbeddedAssetRegistry>()
        .insert_asset(
            std::path::PathBuf::new(),
            std::path::Path::new("resonance_presentation/menu_backdrop.wesl"),
            include_bytes!("../../menu_backdrop.wesl"),
        );
    let warmup = Warmup::default();
    app.sub_app_mut(RenderApp)
        .insert_resource(warmup.clone())
        .add_systems(Render, rendered.in_set(RenderSystems::Cleanup));
    let began = std::time::Instant::now();
    while app.plugins_state() == bevy::app::PluginsState::Adding {
        ensure!(began.elapsed().as_secs() < 180, "renderer setup timed out");
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
        .filter(|mode| *mode == AntiAliasing::Off || mode.samples() > 1)
        .collect::<Vec<_>>();
    let mut images = app.world_mut().resource_mut::<Assets<Image>>();
    // Match the production scene output. OpenGL uses a higher-precision main
    // attachment internally, then converts back into this readable output.
    let target = images.add(Image::new_target_texture(
        64,
        64,
        bevy::render::render_resource::TextureFormat::Bgra8Unorm,
        None,
    ));
    let captured = images.add(Image::default());
    let material = app
        .world_mut()
        .resource_mut::<Assets<crate::menu_backdrop::Material>>()
        .add(crate::menu_backdrop::Material::visible(captured));
    let mesh =
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(mesh(&pieces(7), Vec2::splat(64.), 0));
    let entity = app
        .world_mut()
        .spawn((
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::default(),
            RenderLayers::layer(LAYER),
            NoFrustumCulling,
        ))
        .id();
    app.world_mut().spawn((
        Camera3d::default(),
        Tonemapping::Linear,
        RenderTarget::Image(target.clone().into()),
        Camera {
            order: -3,
            ..default()
        },
    ));
    let projection = Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::Fixed {
            width: 64.,
            height: 64.,
        },
        ..OrthographicProjection::default_2d()
    });
    let camera = warm_camera(app.world_mut(), projection.clone());
    let warm_target = app.world().get::<RenderTarget>(camera).unwrap().clone();
    for mode in modes {
        println!("Checking transition activation with {mode:?}");
        app.world_mut().resource_mut::<AntiAliasingSettings>().0 = mode;
        app.world_mut()
            .entity_mut(camera)
            .insert((warm_target.clone(), projection.clone()));
        *warmup.0.lock().unwrap() = gpu::Report {
            armed: true,
            expected: [entity.into()].into(),
            ..default()
        };
        loop {
            ensure!(
                began.elapsed().as_secs() < 180,
                "{mode:?}: warmup timed out"
            );
            app.update();
            crate::playthrough::check_exit(&app)?;
            let report = warmup.0.lock().unwrap();
            ensure!(report.error.is_none(), "{:?}", report.error);
            if report.completed.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let warmed = pipeline(&app, entity);
        app.world_mut().entity_mut(camera).insert((
            RenderTarget::Image(target.clone().into()),
            projection.clone(),
        ));
        app.world_mut()
            .get_mut::<Camera>(camera)
            .unwrap()
            .clear_color = ClearColorConfig::Custom(Color::WHITE);
        app.update();
        crate::playthrough::check_exit(&app)?;
        assert_eq!(
            pipeline(&app, entity),
            warmed,
            "{mode:?}: activation must not compile a new fragment pipeline"
        );
        // Render errors reach the main world on the following app update.
        app.update();
        crate::playthrough::check_exit(&app)?;
    }
    Ok(())
}
