//! Later cameras on a shared image either replace it (clear) or draw over it
//! (load), with every MSAA sample count.
use super::*;
use crate::native::MeshMaterial2d;
use bevy::camera::visibility::RenderLayers;

#[test]
#[ignore = "requires a GPU; verifies encoded alpha blending before final display conversion"]
fn output_overlay_preserves_translucency_and_draws_over_scene_fades() -> anyhow::Result<()> {
    use crate::materials::TitleOutput;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureUsages};
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins())
        .add_plugins(crate::materials::install_output_materials);
    let mut image = Image::new_target_texture(32, 32, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    // The output camera draws into `target`, as the game's into its window.
    let scene = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            1,
            1,
            TextureFormat::Bgra8Unorm,
            None,
        ));
    crate::test_support::native_scene(&mut app, scene.clone());
    crate::set_display_targets(
        app.world_mut(),
        scene,
        RenderTarget::Image(target.clone().into()),
    );
    crate::test_support::finish_app(&mut app)?;
    let mut pixel = || {
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::new_fill(
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &[0; 4],
                TextureFormat::Rgba8Unorm,
                default(),
            ))
    };
    let source = pixel();
    let overlay = pixel();
    let material = app
        .world_mut()
        .resource_mut::<Assets<TitleOutput>>()
        .add(TitleOutput {
            source: source.clone(),
            overlay: None,
            brightness: Vec4::X,
        });
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(64., 64.));
    app.world_mut().spawn((
        Mesh2d(mesh),
        MeshMaterial2d(material.clone()),
        Transform::default(),
    ));
    app.world_mut().spawn((
        Camera2d,
        Msaa::Off,
        RenderTarget::Image(target.clone().into()),
        Camera {
            clear_color: Color::BLACK.into(),
            ..default()
        },
    ));
    // Overlay RGB is premultiplied by alpha by the ordinary UI render pass.
    for (background, ui, brightness, expected) in [
        (
            [0, 0, 0, 255],
            Some([128, 128, 128, 128]),
            Vec4::X,
            [128, 128, 128, 255],
        ),
        (
            [255; 4],
            Some([0, 0, 0, 128]),
            Vec4::X,
            [127, 127, 127, 255],
        ),
        (
            [64, 128, 192, 255],
            Some([128; 4]),
            Vec4::X,
            [160, 192, 224, 255],
        ),
        ([255; 4], Some([128; 4]), Vec4::Z, [128, 128, 128, 255]),
        (
            [0, 0, 0, 255],
            Some([0, 0, 0, 128]),
            Vec4::Y,
            [127, 127, 127, 255],
        ),
        ([64, 128, 192, 255], None, Vec4::X, [64, 128, 192, 255]),
    ] {
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .get_mut(&source)
            .unwrap()
            .data = Some(background.to_vec());
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .get_mut(&overlay)
            .unwrap()
            .data = Some(ui.unwrap_or([0; 4]).to_vec());
        {
            let mut outputs = app.world_mut().resource_mut::<Assets<TitleOutput>>();
            let mut output = outputs.get_mut(&material).unwrap();
            output.brightness = brightness;
            output.overlay = ui.map(|_| overlay.clone());
        }
        for _ in 0..10 {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let pixels = crate::test_support::capture_image(&mut app, target.clone())?
            .try_into_dynamic()?
            .to_rgba8();
        assert!(
            pixels.pixels().all(|pixel| pixel
                .0
                .iter()
                .zip(expected)
                .all(|(&actual, expected)| actual.abs_diff(expected) <= 1)),
            "background={background:?}, overlay={ui:?}, brightness={brightness:?}: expected {expected:?}, got {:?}",
            pixels.get_pixel(16, 16).0
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires a GPU with MSAA"]
fn shared_targets_preserve_clear_load_and_scene_history() -> anyhow::Result<()> {
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins())
        .add_plugins(AntiAliasingPlugin)
        .insert_resource(ClearColor(Color::srgb(1., 1., 0.)));
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            64,
            64,
            TextureFormat::Bgra8Unorm,
            None,
        ));
    crate::test_support::native_scene(&mut app, target.clone());
    crate::field_dissolve::install(&mut app);
    crate::test_support::finish_app(&mut app)?;
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
    let render_target = RenderTarget::Image(target.clone().into());
    let scene = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            render_target.clone(),
            Camera {
                clear_color: Color::srgb(0., 1., 0.).into(),
                ..default()
            },
        ))
        .id();
    let replacement = app
        .world_mut()
        .spawn((
            Camera2d,
            render_target.clone(),
            RenderLayers::layer(1),
            Camera {
                order: 1,
                ..default()
            },
        ))
        .id();
    let overlay = app
        .world_mut()
        .spawn((
            Camera2d,
            render_target.clone(),
            RenderLayers::layer(2),
            Camera {
                order: 2,
                clear_color: ClearColorConfig::None,
                ..default()
            },
        ))
        .id();
    // Opaque rectangles: an image fully covered by the backdrop's color.
    let white = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_fill(
            bevy::render::render_resource::Extent3d::default(),
            bevy::render::render_resource::TextureDimension::D2,
            &[255; 4],
            TextureFormat::Rgba8Unorm,
            default(),
        ));
    let [panel, _] = [
        (1, Color::srgb(0., 0., 1.), Vec3::ZERO, 16.),
        (2, Color::srgb(1., 0., 0.), Vec3::new(-20., 20., 0.), 8.),
    ]
    .map(|(layer, color, position, width)| {
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Rectangle::new(width, width));
        let material = app
            .world_mut()
            .resource_mut::<Assets<crate::menu_backdrop::Material>>()
            .add(
                crate::menu_backdrop::Material::visible(white.clone())
                    .with_cover(color.to_linear().to_vec4()),
            );
        app.world_mut().spawn((
            Mesh2d(mesh),
            MeshMaterial2d(material.clone()),
            Transform::from_translation(position),
            RenderLayers::layer(layer),
        ));
        material
    });
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
                [0, 0, 255, 255],
                format!("{mode:?}/{clear:?}"),
            )?;
        }
        // Model replacements render behind the held field, including during readback.
        app.world_mut()
            .get_mut::<Camera>(overlay)
            .unwrap()
            .output_mode = bevy::camera::CameraOutputMode::Skip;
        app.world_mut()
            .get_mut::<Camera>(scene)
            .unwrap()
            .clear_color = Color::BLACK.into();
        check_capture(
            &mut app,
            &target,
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            format!("{mode:?}/held"),
        )?;
        app.world_mut()
            .get_mut::<Camera>(overlay)
            .unwrap()
            .output_mode = default();
        check_capture(
            &mut app,
            &target,
            [0, 0, 0, 255],
            [0, 0, 255, 255],
            format!("{mode:?}/released"),
        )?;
        app.world_mut()
            .get_mut::<Camera>(scene)
            .unwrap()
            .clear_color = Color::srgb(0., 1., 0.).into();
    }

    // The previous frame has a blue panel. Opening a menu must freeze the green
    // field behind it, even when the panel samples that capture in this frame.
    let frozen = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            32,
            32,
            TextureFormat::Bgra8Unorm,
            None,
        ));
    let green = [0, 255, 0, 255];
    let mut capture = crate::menu_backdrop::Capture::once(frozen.clone());
    app.world_mut().entity_mut(scene).insert(capture.clone());
    check_capture(
        &mut app,
        &target,
        green,
        [0, 0, 255, 255],
        "pending resize".into(),
    )?;
    assert!(!capture.ready());
    assert!(capture.fit_source(&target, &mut app.world_mut().resource_mut()));
    app.world_mut().entity_mut(scene).insert(capture.clone());
    *app.world_mut()
        .resource_mut::<Assets<crate::menu_backdrop::Material>>()
        .get_mut(&panel)
        .unwrap() = crate::menu_backdrop::Material::visible(frozen);
    check_capture(&mut app, &target, green, green, "menu backdrop".into())?;
    assert!(capture.ready());
    app.world_mut()
        .get_mut::<Camera>(scene)
        .unwrap()
        .clear_color = Color::BLACK.into();
    let mut field = resonance_events::GameWorld::default();
    app.world_mut()
        .entity_mut(scene)
        .insert(crate::field_dissolve::field_settings(1, &field, None));
    check_capture(
        &mut app,
        &target,
        [0, 0, 0, 255],
        green,
        "frozen backdrop".into(),
    )?;
    // The preceding black scene contributes half the first tick. Repeated
    // renders in check_capture must not keep halving its contribution.
    app.world_mut()
        .get_mut::<Camera>(scene)
        .unwrap()
        .clear_color = Color::srgb(0., 1., 0.).into();
    field.frame_feedback = 1;
    for (tick, expected) in [(1, 127), (2, 191)] {
        field.tick = tick;
        app.world_mut()
            .entity_mut(scene)
            .insert(crate::field_dissolve::field_settings(1, &field, None));
        check_capture(
            &mut app,
            &target,
            [0, expected, 0, 255],
            green,
            format!("field feedback at tick {tick}"),
        )?;
    }
    Ok(())
}

fn check_capture(
    app: &mut App,
    target: &Handle<Image>,
    background: [u8; 4],
    panel: [u8; 4],
    context: String,
) -> anyhow::Result<()> {
    // Allow camera extraction and shader preparation to settle.
    for _ in 0..20 {
        app.update();
        crate::playthrough::check_exit(app)?;
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let image = crate::test_support::capture_image(app, target.clone())?
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
        panel,
        "{context}: replacement surface"
    );
    assert_eq!(
        image.get_pixel(width / 2 - 20, height / 2 - 20).0,
        [255, 0, 0, 255],
        "{context}: UI overlay"
    );
    Ok(())
}
