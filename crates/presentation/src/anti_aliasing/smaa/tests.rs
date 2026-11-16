use super::*;
use crate::native::{MeshMaterial2d, MeshMaterial3d};

/// SMAA (or S2x) on the native frame: a white quad over a red clear, a green
/// UI glyph that SMAA must not filter, a resize and three simultaneous target
/// sizes retaining their GPU resources. Returns false when the adapter does
/// not support `mode`.
fn render_native_scene(mode: AntiAliasing) -> anyhow::Result<bool> {
    use bevy::camera::{RenderTarget, visibility::RenderLayers};
    const SIZE: u32 = 96;
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins_with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
    ))
    .add_plugins(super::super::AntiAliasingPlugin);
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            SIZE,
            SIZE,
            crate::renderer::SCENE_COLOR_FORMAT,
            None,
        ));
    crate::test_support::native_scene(&mut app, target.clone());
    crate::test_support::finish_app(&mut app)?;
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    app.update();
    let caps = app
        .world()
        .resource::<super::super::AntiAliasingCapabilities>();
    if !caps.supports(mode) {
        return Ok(false);
    }
    app.world_mut()
        .resource_mut::<super::super::AntiAliasingSettings>()
        .0 = mode;
    let white = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_fill(
            Extent3d::default(),
            TextureDimension::D2,
            &[255; 4],
            TextureFormat::Rgba8Unorm,
            default(),
        ));
    app.world_mut().spawn((
        Camera3d::default(),
        RenderTarget::Image(target.clone().into()),
        Camera {
            clear_color: Color::srgb(1., 0., 0.).into(),
            ..default()
        },
    ));
    // Preview and reflection cameras can keep sizes different from the scene
    // after a live resolution change. Both remain active under the menu.
    let offscreen_sizes = [UVec2::new(80, 64), UVec2::splat(48)];
    for (index, size) in offscreen_sizes.into_iter().enumerate() {
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::new_target_texture(
                size.x,
                size.y,
                crate::renderer::SCENE_COLOR_FORMAT,
                None,
            ));
        app.world_mut().spawn((
            Camera3d::default(),
            RenderTarget::Image(image.into()),
            crate::native::Offscreen,
            Camera {
                order: -10 + index as isize,
                ..default()
            },
        ));
    }
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(1., 1.));
    // The glow material draws its texture with a fourfold gain: white.
    let material = app
        .world_mut()
        .resource_mut::<Assets<crate::glow::GlowMaterial>>()
        .add(crate::glow::GlowMaterial {
            texture: white.clone(),
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
        .resource_mut::<Assets<crate::menu_backdrop::Material>>()
        .add(crate::menu_backdrop::Material::visible(white).with_cover(Vec4::new(0., 1., 0., 1.)));
    let ui_layer = RenderLayers::layer(1);
    app.world_mut().spawn((
        Mesh2d(glyph),
        MeshMaterial2d(green),
        Transform::from_xyz(-32., 32., 0.),
        ui_layer.clone(),
    ));
    app.world_mut().spawn((
        Camera2d,
        Msaa::Off,
        RenderTarget::Image(target.clone().into()),
        ui_layer,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
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
        crate::test_support::render_until(
            &mut app,
            "SMAA render resources were not prepared",
            |app| crate::native::render::smaa_ready(app, mode, size),
        )?;
        let textures = |app: &App| {
            crate::native::render::smaa(app)
                .unwrap()
                .0
                .iter()
                .map(|view| {
                    (
                        view.size,
                        [
                            view.edges.texture.id(),
                            view.stencil.texture.id(),
                            view.weights.texture.id(),
                        ],
                    )
                })
                .collect::<Vec<_>>()
        };
        let retained = textures(&app);
        for expected in offscreen_sizes.into_iter().chain([size]) {
            anyhow::ensure!(
                retained.iter().any(|(size, _)| *size == expected),
                "{mode:?}: simultaneous SMAA target {expected:?} was evicted"
            );
        }
        for _ in 0..8 {
            app.update();
            crate::playthrough::check_exit(&app)?;
            anyhow::ensure!(
                textures(&app) == retained,
                "{mode:?}: SMAA textures changed while all three target sizes stayed active"
            );
        }
        let image = crate::test_support::capture_image(&mut app, target.clone())?
            .try_into_dynamic()?
            .to_rgba8();
        anyhow::ensure!(image.dimensions() == (size.x, size.y), "resize failed");
        anyhow::ensure!(
            image
                .get_pixel(size.x / 2, size.y / 2)
                .0
                .iter()
                .all(|channel| *channel >= 254),
            "{mode:?} {size:?}: failed to render opaque geometry: {:?}",
            image.get_pixel(size.x / 2, size.y / 2).0
        );
        anyhow::ensure!(
            image
                .enumerate_pixels()
                // Keep the UI square out of the scene-edge check. Filtering
                // that square must not masquerade as smoothing the geometry.
                .filter(|(x, y, p)| {
                    (*x > size.x / 2 - 24 || *y > size.y / 2 - 24) && p.0[1] > 1 && p.0[1] < 254
                })
                .count()
                > 10,
            "{mode:?}: SMAA did not smooth the scene edge"
        );
        anyhow::ensure!(
            image.get_pixel(0, 0).0 == [255, 0, 0, 255],
            "{mode:?}: lost the scene clear color"
        );
        let glyph_center = UVec2::new(size.x / 2 - 32, size.y / 2 - 32);
        for y in glyph_center.y - 7..=glyph_center.y + 6 {
            for x in glyph_center.x - 7..=glyph_center.x + 6 {
                let inside = (glyph_center.x - 6..glyph_center.x + 6).contains(&x)
                    && (glyph_center.y - 6..glyph_center.y + 6).contains(&y);
                let expected = if inside {
                    [0, 255, 0, 255]
                } else {
                    [255, 0, 0, 255]
                };
                anyhow::ensure!(
                    image.get_pixel(x, y).0 == expected,
                    "{mode:?}: UI edge ({x}, {y}) was filtered: {:?}, expected {expected:?}",
                    image.get_pixel(x, y).0
                );
            }
        }
    }
    app.world_mut()
        .resource_mut::<super::super::AntiAliasingSettings>()
        .0 = AntiAliasing::Off;
    app.update();
    anyhow::ensure!(
        crate::native::render::smaa(&app).unwrap().0.is_empty(),
        "{mode:?}: disabling SMAA retained its render textures"
    );
    Ok(true)
}
#[test]
#[ignore = "requires a GPU; validates SMAA and S2x edges, UI, resize and three-target resource reuse"]
fn native_smaa_smooths_the_scene_and_keeps_ui() -> anyhow::Result<()> {
    for mode in [
        AntiAliasing::SmaaLow,
        AntiAliasing::SmaaMedium,
        AntiAliasing::SmaaHigh,
        AntiAliasing::SmaaUltra,
    ] {
        anyhow::ensure!(render_native_scene(mode)?, "{mode:?} must be supported");
    }
    // S2x needs two-sample MSAA, which software adapters may lack.
    if !render_native_scene(AntiAliasing::SmaaS2x)? {
        println!("S2x unsupported on this adapter; skipped");
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
