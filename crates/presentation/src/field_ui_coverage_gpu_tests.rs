//! UI slices must join without holes or neighboring atlas pixels under MSAA.
use super::super::{Surface, TextureEffect, UiInterpolation};
use super::*;
use bevy::{
    camera::{RenderTarget, ScalingMode},
    core_pipeline::tonemapping::Tonemapping,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        renderer::{RenderAdapter, RenderDevice},
        settings::WgpuFeatures,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
};

#[derive(Resource, Default)]
struct Captures(Vec<Image>);

#[test]
#[ignore = "requires a GPU; checks dialogue masks and tiled artwork at subpixel MSAA joins"]
fn dialogue_slices_have_no_msaa_seams() -> Result<()> {
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins())
        .init_resource::<Captures>();
    super::super::install(&mut app);
    let began = std::time::Instant::now();
    while app.plugins_state() == bevy::app::PluginsState::Adding {
        anyhow::ensure!(
            began.elapsed().as_secs() < 120,
            "dialogue GPU setup timed out"
        );
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    let adapter = app.world().resource::<RenderAdapter>();
    let device = app.world().resource::<RenderDevice>();
    let formats = [TextureFormat::Bgra8Unorm, TextureFormat::Depth32Float].map(|format| {
        if device
            .features()
            .contains(WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        {
            adapter.get_texture_format_features(format)
        } else {
            format.guaranteed_format_features(device.features())
        }
    });
    let modes: Vec<_> = [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8]
        .into_iter()
        .filter(|mode| {
            formats
                .iter()
                .all(|f| f.flags.sample_count_supported(mode.samples()))
        })
        .collect();
    anyhow::ensure!(
        modes.iter().any(|mode| mode.samples() > 1),
        "this test requires native MSAA support"
    );
    let white = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_fill(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255; 4],
            TextureFormat::Rgba8Unorm,
            default(),
        ));
    let mut pattern = Image::new_fill(
        Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255; 4],
        TextureFormat::Rgba8Unorm,
        default(),
    );
    pattern.data = Some(vec![
        255, 255, 255, 255, 255, 255, 255, 0, 255, 255, 255, 0, 255, 255, 255, 255,
    ]);
    pattern.sampler = bevy::image::ImageSampler::nearest();
    let pattern = app.world_mut().resource_mut::<Assets<Image>>().add(pattern);
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            64,
            48,
            TextureFormat::Bgra8Unorm,
            None,
        ));
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Msaa::Off,
            Tonemapping::Linear,
            RenderTarget::Image(target.clone().into()),
            Camera {
                clear_color: Color::BLACK.into(),
                ..default()
            },
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed {
                    width: 64.,
                    height: 48.,
                },
                ..OrthographicProjection::default_2d()
            }),
        ))
        .id();
    // Both slices are white, so their interior union must stay white. A dark
    // horizontal seam means the fill was discarded where no border sample drew.
    let seam = [292., 220., 348., 224.75];
    let textured = [296.3, 230.2, 344.6, 252.7];
    let mask = Coverage {
        count: UVec4::new(2, 0, 0, 0),
        quads: {
            let mut quads = [Quad::default(); LIMIT];
            quads[0] = Quad {
                rect: Vec4::from_array(seam),
                uv: Vec4::new(0., 0., 1., 1.),
                properties: Vec4::new(2., 1., 0., 0.),
            };
            quads[1] = Quad {
                rect: Vec4::from_array(textured),
                uv: Vec4::new(0., 0., 1., 1.),
                properties: Vec4::new(0., 1., 0., 0.),
            };
            quads
        },
    };
    let mut layers = Vec::new();
    for (index, (rect, coverage, source, size, interpolation)) in [
        (
            [292., 220., 348., 260.],
            mask,
            white.clone(),
            [1, 1],
            UiInterpolation::Sample,
        ),
        (
            seam,
            Coverage::default(),
            white.clone(),
            [1, 1],
            UiInterpolation::Sample,
        ),
        (
            textured,
            Coverage::default(),
            pattern.clone(),
            [2, 2],
            UiInterpolation::Sample,
        ),
        (
            [292., 260., 348., 264.],
            Coverage::default(),
            pattern.clone(),
            [2, 2],
            UiInterpolation::Center,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut batch = Batch::default();
        batch.quad(rect, [0., 0., size[0] as f32, size[1] as f32], [1.; 4]);
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(batch.mesh(size));
        let material = app
            .world_mut()
            .resource_mut::<Assets<Surface>>()
            .add(Surface {
                source: source.clone(),
                sampling: source,
                frame_mask: pattern.clone(),
                color_mask: white.clone(),
                coverage,
                interpolation,
                opaque: false,
                additive: false,
                texture_effect: TextureEffect::Plain,
            });
        layers.push((mesh.clone(), material.clone()));
        app.world_mut().spawn((
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::from_xyz(0., 0., index as f32),
        ));
    }
    // Nearest-filtered tiles must not read colored or transparent neighbors
    // when a subpixel join puts the pixel center outside the covered primitive.
    // Linear-filtered tiles must also retain clean pixel-aligned joins at native
    // scale; evaluating their UVs at every MSAA sample introduces atlas bleed.
    for (index, (width, sampler, [top, bottom], edges)) in [
        (
            3,
            bevy::image::ImageSampler::nearest(),
            [217., 219.],
            [292., 320.25, 348.],
        ),
        (
            26,
            bevy::image::ImageSampler::linear(),
            [219., 220.],
            [292., 316., 340.],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut tile = Image::new_fill(
            Extent3d {
                width,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255; 4],
            TextureFormat::Rgba8Unorm,
            default(),
        );
        let pixels = tile.data.as_mut().unwrap();
        pixels[..4].copy_from_slice(&[255, 0, 255, 255]);
        *pixels.last_mut().unwrap() = 0;
        tile.sampler = sampler;
        let tile = app.world_mut().resource_mut::<Assets<Image>>().add(tile);
        let surface = super::super::battle_ui::test_tiled_surface(tile, [width, 1]);
        let material = app
            .world_mut()
            .resource_mut::<Assets<Surface>>()
            .add(surface);
        let mut tiled = Batch::default();
        for edge in edges.windows(2) {
            tiled.quad(
                [edge[0], top, edge[1], bottom],
                [1., 0., (width - 1) as f32, 1.],
                [1.; 4],
            );
        }
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(tiled.mesh([width, 1]));
        layers.push((mesh.clone(), material.clone()));
        app.world_mut().spawn((
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::from_xyz(0., 0., 4. + index as f32),
        ));
    }
    for size in [UVec2::new(64, 48), UVec2::new(96, 72), UVec2::new(160, 120)] {
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .get_mut(&target)
            .unwrap()
            .resize(Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            });
        for opacity in [1.0f32, 0.5] {
            for (mesh, material) in &layers {
                use bevy::mesh::VertexAttributeValues;
                {
                    let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
                    let mut mesh = meshes.get_mut(mesh).unwrap();
                    let Some(VertexAttributeValues::Float32x4(colors)) =
                        mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
                    else {
                        unreachable!()
                    };
                    for color in colors {
                        color[3] = opacity;
                    }
                }
                let mut materials = app.world_mut().resource_mut::<Assets<Surface>>();
                let coverage = &mut materials.get_mut(material).unwrap().coverage;
                for quad in &mut coverage.quads[..coverage.count.x as usize] {
                    quad.properties.y = opacity;
                }
            }
            let mut baseline = None;
            for &mode in &modes {
                app.world_mut().entity_mut(camera).insert(mode);
                for _ in 0..40 {
                    app.update();
                    crate::playthrough::check_exit(&app)?;
                }
                let count = app.world().resource::<Captures>().0.len();
                app.world_mut()
                    .spawn(Screenshot::image(target.clone()))
                    .observe(
                        |shot: On<ScreenshotCaptured>, mut captures: ResMut<Captures>| {
                            captures.0.push(shot.image.clone());
                        },
                    );
                while app.world().resource::<Captures>().0.len() == count {
                    anyhow::ensure!(
                        began.elapsed().as_secs() < 120,
                        "dialogue readback timed out"
                    );
                    app.update();
                    crate::playthrough::check_exit(&app)?;
                    std::thread::sleep(std::time::Duration::from_millis(1));
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
                let scale = size.x as f32 / 64.;
                let expected = (opacity * 255.).round() as u8;
                for y in (6. * scale) as u32..(38. * scale) as u32 {
                    for x in (6. * scale) as u32..(58. * scale) as u32 {
                        let pixel = image.get_pixel(x, y).0;
                        anyhow::ensure!(
                            pixel[..3].iter().all(|v| v.abs_diff(expected) <= 1),
                            "{mode:?}, {size:?}, opacity={opacity}: dialogue slice seam at {x},{y}: {pixel:?}, expected {expected}"
                        );
                    }
                }
                for y in (1.5 * scale).ceil() as u32..(3. * scale).floor() as u32 {
                    for x in (6. * scale) as u32..(58. * scale) as u32 {
                        let pixel = image.get_pixel(x, y).0;
                        anyhow::ensure!(
                            pixel[..3].iter().all(|v| v.abs_diff(expected) <= 1),
                            "{mode:?}, {size:?}, opacity={opacity}: tiled HUD artifact at {x},{y}: {pixel:?}, expected {expected}"
                        );
                    }
                }
                if size.x == 64 {
                    for x in 6..50 {
                        let pixel = image.get_pixel(x, 3).0;
                        anyhow::ensure!(
                            pixel[..3].iter().all(|v| v.abs_diff(expected) <= 1),
                            "{mode:?}, opacity={opacity}: linear atlas bleed at {x},3: {pixel:?}, expected {expected}"
                        );
                    }
                }
                if mode == Msaa::Off {
                    baseline = Some(image.clone());
                } else {
                    let reference = baseline.as_ref().unwrap();
                    for y in (44. * scale) as u32 + 1..size.y - 1 {
                        for x in (6. * scale) as u32..(58. * scale) as u32 {
                            anyhow::ensure!(
                                image.get_pixel(x, y) == reference.get_pixel(x, y),
                                "{mode:?}: MSAA changed pixel-center text sampling at {x},{y}"
                            );
                        }
                    }
                }
                println!("{mode:?}, {size:?}, opacity={opacity}: UI slices have no artifacts");
            }
        }
    }
    Ok(())
}
