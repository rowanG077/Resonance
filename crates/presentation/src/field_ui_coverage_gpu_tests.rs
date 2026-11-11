//! UI slices must join without holes or neighboring atlas pixels in every AA mode.
use super::super::{Surface, TextureEffect, UiInterpolation};
use super::*;
use crate::anti_aliasing::{AntiAliasingCapabilities, AntiAliasingPlugin, AntiAliasingSettings};
use bevy::{
    camera::{CameraOutputMode, RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
};
use resonance_content::menu_data::AntiAliasing;

#[derive(Resource, Default)]
struct Captures(Vec<Image>);

#[test]
#[ignore = "requires a GPU; checks dialogue masks and scaled atlas joins with SMAA and MSAA"]
fn dialogue_slices_have_no_aa_seams() -> Result<()> {
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins())
        .add_plugins(AntiAliasingPlugin)
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
    app.update();
    let modes = app
        .world()
        .resource::<AntiAliasingCapabilities>()
        .available_modes()
        .to_vec();
    anyhow::ensure!(
        modes.contains(&AntiAliasing::SmaaUltra),
        "SMAA must be covered"
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
    // Use the production scene -> SMAA -> UI order. SMAA deliberately does
    // not filter the UI; its one-sample atlas joins still have to be seamless.
    let scene = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Tonemapping::Linear,
            RenderTarget::Image(target.clone().into()),
            RenderLayers::layer(1),
            Camera {
                clear_color: Color::BLACK.into(),
                output_mode: CameraOutputMode::Skip,
                ..default()
            },
        ))
        .id();
    let ui = app
        .world_mut()
        .spawn((
            Camera2d,
            Msaa::Off,
            Tonemapping::Linear,
            RenderTarget::Image(target.clone().into()),
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                output_mode: crate::renderer::COMPOSITED_OUTPUT,
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
    // Linear-filtered cells must also stay isolated when magnification places
    // a pixel center less than half a texel from a cell boundary. Native-scale
    // testing alone misses this: single-sample UI has an exact texel-center hit.
    for (index, (coordinates, physical, sampler, [top, bottom], edges, clipped)) in [
        (
            [3, 1],
            [3, 1],
            bevy::image::ImageSampler::nearest(),
            [217., 219.],
            [292., 320.25, 348.],
            false,
        ),
        (
            [26, 1],
            [26, 1],
            bevy::image::ImageSampler::linear(),
            [219., 220.],
            [292., 316., 340.],
            false,
        ),
        (
            [26, 4],
            [13, 2],
            bevy::image::ImageSampler::linear(),
            [254., 256.],
            [292., 314., 336.],
            false,
        ),
        (
            [26, 4],
            [52, 8],
            bevy::image::ImageSampler::linear(),
            [256., 258.],
            [292., 314., 336.],
            false,
        ),
        // The second strip is clipped; the first keeps the same source UVs.
        (
            [1, 4],
            [1, 4],
            bevy::image::ImageSampler::linear(),
            [257., 261.],
            [288., 290., 292.],
            true,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut tile = Image::new_fill(
            Extent3d {
                width: physical[0],
                height: physical[1],
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255; 4],
            TextureFormat::Rgba8Unorm,
            default(),
        );
        let replacement = coordinates != physical;
        let left = if clipped {
            0.
        } else if replacement {
            2.
        } else {
            1.
        };
        let right = coordinates[0] as f32 - left;
        let first = (left * physical[0] as f32 / coordinates[0] as f32) as u32;
        let end = (right * physical[0] as f32 / coordinates[0] as f32) as u32;
        let pixels = tile.data.as_mut().unwrap();
        for (i, pixel) in pixels.chunks_exact_mut(4).enumerate() {
            let x = i as u32 % physical[0];
            if clipped {
                pixel[..3].fill(if i < 2 { 0 } else { 255 });
            } else if x < first {
                pixel.copy_from_slice(&[255, 0, 255, 255]);
            } else if x >= end {
                pixel[3] = 0;
            } else if physical[0] > coordinates[0] && (x == first || x == end - 1) {
                // A doubled replacement must retain its first/last physical
                // texel, rather than flattening an entire coordinate texel.
                pixel.copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        tile.sampler = sampler;
        let tile = app.world_mut().resource_mut::<Assets<Image>>().add(tile);
        let surface = super::super::battle_ui::test_tiled_surface(tile, coordinates);
        let material = app
            .world_mut()
            .resource_mut::<Assets<Surface>>()
            .add(surface);
        let mut tiled = Batch::default();
        for (index, edge) in edges.windows(2).enumerate() {
            let [u0, u1] = if replacement && index == 1 {
                [right, left]
            } else {
                [left, right]
            };
            tiled.quad(
                [edge[0], top, edge[1], bottom],
                [u0, 0., u1, coordinates[1] as f32],
                [1.; 4],
            );
        }
        if clipped {
            tiled.clip_vertical(4, 240. - (top + 1.25), 240. - bottom);
        }
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(tiled.mesh(coordinates));
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
                app.world_mut().resource_mut::<AntiAliasingSettings>().0 = mode;
                for _ in 0..40 {
                    app.update();
                    crate::playthrough::check_exit(&app)?;
                }
                for camera in [scene, ui] {
                    anyhow::ensure!(
                        app.world().get::<Msaa>(camera).unwrap().samples() == mode.samples(),
                        "{mode:?}: camera did not use the requested sample count"
                    );
                }
                while !crate::anti_aliasing::test_smaa_ready(&mut app, mode, size) {
                    anyhow::ensure!(
                        began.elapsed().as_secs() < 120,
                        "{mode:?}, {size:?}: SMAA render resources were not prepared"
                    );
                    app.update();
                    crate::playthrough::check_exit(&app)?;
                    std::thread::sleep(std::time::Duration::from_millis(1));
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
                if let Some(directory) = std::env::var_os("RESONANCE_AA_CAPTURE_DIR") {
                    let directory = std::path::Path::new(&directory);
                    std::fs::create_dir_all(directory)?;
                    image.save(
                        directory.join(format!("{mode:?}-{}x{}-{opacity}.png", size.x, size.y)),
                    )?;
                }
                let scale = size.x as f32 / 64.;
                let expected = (opacity * 255.).round() as u8;
                // Clipping moves the quad's triangle diagonal. MSAA centroid
                // UVs can therefore differ even in a fully covered quad pixel.
                // Pixel-center equality isolates the source-bound regression.
                if mode.samples() == 1 {
                    let y = (42.25 * scale - 0.5).ceil() as u32;
                    let reference = image.get_pixel(scale as u32, y).0;
                    let clipped = image.get_pixel((3. * scale) as u32, y).0;
                    anyhow::ensure!(
                        clipped
                            .iter()
                            .zip(reference)
                            .all(|(actual, expected)| actual.abs_diff(expected) <= 1),
                        "{mode:?}, {size:?}, opacity={opacity}: clipping changed atlas sampling at row {y}: {clipped:?}, reference {reference:?}"
                    );
                }
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
                for x in (6. * scale) as u32..(50. * scale) as u32 {
                    let pixel = image.get_pixel(x, (3.5 * scale) as u32).0;
                    anyhow::ensure!(
                        pixel[..3].iter().all(|v| v.abs_diff(expected) <= 1),
                        "{mode:?}, opacity={opacity}: linear atlas bleed at {x} (scale {scale}): {pixel:?}, expected {expected}"
                    );
                }
                if opacity == 1. {
                    // Half-resolution replacements need a larger normalized
                    // guard. Both ordinary and flipped cells must stay white.
                    for x in (4. * scale) as u32..(48. * scale) as u32 {
                        let pixel = image.get_pixel(x, (39. * scale) as u32).0;
                        anyhow::ensure!(
                            pixel[..3].iter().all(|v| *v >= 254),
                            "{mode:?}, {size:?}: half-resolution atlas bleed at {x}: {pixel:?}"
                        );
                    }
                    if size.x == 160 {
                        // At 2.5x the first covered pixel lies before the
                        // doubled image's first texel center, so it clamps to
                        // the black edge texel, including with flipped UVs.
                        for x in [4., 26.] {
                            let pixel = image.get_pixel((x * scale) as u32, (41. * scale) as u32).0;
                            anyhow::ensure!(
                                pixel[..3].iter().all(|v| *v <= 1),
                                "{mode:?}: doubled atlas lost its edge texel at {x}: {pixel:?}"
                            );
                        }
                    }
                }
                if mode == AntiAliasing::Off {
                    baseline = Some(image.clone());
                } else {
                    let reference = baseline.as_ref().unwrap();
                    for y in (44. * scale) as u32 + 1..size.y - 1 {
                        for x in (6. * scale) as u32..(58. * scale) as u32 {
                            anyhow::ensure!(
                                image.get_pixel(x, y) == reference.get_pixel(x, y),
                                "{mode:?}: AA changed pixel-center text sampling at {x},{y}"
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
