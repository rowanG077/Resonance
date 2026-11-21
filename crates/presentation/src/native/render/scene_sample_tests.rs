//! Restore the captured-mesh regression against native draw compilation and encoding.
use super::*;
use bevy::{
    asset::RenderAssetUsages,
    camera::{RenderTarget, visibility::NoFrustumCulling},
    mesh::Indices,
};

const DEPTH_SHADER: &str = r#"
@fragment
fn fragment(@builtin(position) position:vec4<f32>,@builtin(sample_index) sample:u32)->@builtin(frag_depth) f32 {
    // Equal depth, full occlusion, partial coverage, and no occlusion.
    if position.y < 2. {return 0.5;}
    if position.x < 4. {return 0.75;}
    if position.x < 8. {return select(0.25,0.75,sample % 2u == 1u);}
    return 0.25;
}
"#;

#[test]
#[ignore = "requires a GPU; checks native captured-mesh depth, culling, and masked battle samples"]
fn captured_meshes_obey_per_sample_depth_in_every_aa_mode() -> anyhow::Result<()> {
    use crate::anti_aliasing::{AntiAliasingCapabilities, AntiAliasingSettings};
    let mut app = App::new();
    app.add_plugins(crate::test_support::render_plugins_with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
    ));
    let scene = super::tests::image(&mut app, 12);
    let output = super::tests::image(&mut app, 12);
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&output)
        .unwrap()
        .texture_descriptor
        .usage |= TextureUsages::COPY_SRC;
    crate::test_support::native_scene(&mut app, scene.clone());
    crate::gpu_prepare::install(&mut app);
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
            RenderAssetUsages::default(),
        ));
    let shader = app
        .world_mut()
        .resource_mut::<Assets<Shader>>()
        .add(Shader::from_wgsl(
            DEPTH_SHADER,
            "captured-mesh synthetic depth",
        ));
    let positions = [[-1., -1., 0.5], [3., -1., 0.5], [-1., 3., 0.5]];
    let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.to_vec())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.; 3]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.5; 2]; 3])
        .with_inserted_indices(Indices::U16(vec![0, 1, 2])),
    );
    let preparation = crate::gpu_prepare::Shared::new(app.world());
    {
        let mut report = preparation.0.lock().unwrap();
        for samples in [1, 2] {
            report.prepare(
                crate::gpu_prepare::View::Field,
                mesh.id(),
                super::super::Prepare::SceneSample {
                    masked: false,
                    depth_test: true,
                },
                false,
                Some(samples),
            );
        }
        report.armed = true;
    }
    app.world_mut().spawn((
        Camera3d::default(),
        Msaa::Off,
        RenderTarget::Image(scene.into()),
    ));
    crate::test_support::finish_app(&mut app)?;
    crate::test_support::render_until(&mut app, "capture layouts did not prewarm", |_| {
        preparation
            .0
            .lock()
            .unwrap()
            .completed
            .load(std::sync::atomic::Ordering::Acquire)
    })?;
    let warmed = preparation.0.lock().unwrap().pipelines.clone();
    assert_eq!(warmed.len(), 2);
    // No draw entity existed during preparation. Its first use, including
    // after an AA change, must select one of those completed pipelines.
    let entity = app
        .world_mut()
        .spawn((
            Mesh3d(mesh.clone()),
            super::super::SceneSample::default(),
            Transform::IDENTITY,
            NoFrustumCulling,
        ))
        .id();
    let modes = app
        .world()
        .resource::<AntiAliasingCapabilities>()
        .available_modes()
        .to_vec();
    for mode in modes {
        app.world_mut().resource_mut::<AntiAliasingSettings>().0 = mode;
        let samples = mode.samples();
        let depth_pipeline = {
            let world = app.sub_app(RenderApp).world();
            world
                .resource::<PipelineCache>()
                .queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("captured-mesh test depth".into()),
                    vertex: world
                        .resource::<bevy::core_pipeline::FullscreenShader>()
                        .to_vertex_state(),
                    fragment: Some(FragmentState {
                        shader: shader.clone(),
                        entry_point: Some("fragment".into()),
                        targets: vec![],
                        ..default()
                    }),
                    depth_stencil: Some(DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(CompareFunction::Always),
                        stencil: default(),
                        bias: default(),
                    }),
                    multisample: MultisampleState {
                        count: samples,
                        ..default()
                    },
                    ..default()
                })
        };
        for (masked, depth_test, reversed) in [
            (false, true, false),
            (false, true, true),
            (true, true, true),
            (true, false, false),
        ] {
            app.world_mut()
                .entity_mut(entity)
                .insert(super::super::SceneSample {
                    mask: masked.then(|| white.clone()),
                    depth_test,
                    tint: Vec4::new(0., 0., 0., 1.),
                    ..default()
                });
            app.world_mut()
                .resource_mut::<Assets<Mesh>>()
                .get_mut(&mesh)
                .unwrap()
                .insert_indices(Indices::U16(if reversed {
                    vec![0, 2, 1]
                } else {
                    vec![0, 1, 2]
                }));
            if masked {
                app.world_mut()
                    .resource_mut::<Assets<Mesh>>()
                    .get_mut(&mesh)
                    .unwrap()
                    .remove_indices();
                let geometry = crate::materials::GeometryData::default();
                // The stream owns its draw count: the mesh has only three vertices,
                // while the visible triangle follows three degenerate ones here.
                geometry.replace(
                    [None, None, None]
                        .into_iter()
                        .chain((if reversed { [0, 2, 1] } else { [0, 1, 2] }).map(Some))
                        .map(|i| crate::materials::GeometryVertex {
                            position: Vec3::from_array(i.map_or([2., 2., 0.5], |i| positions[i]))
                                .extend(1.)
                                .to_array(),
                            normal: [0.; 4],
                            uv: [0.5; 4],
                            color: [1.; 4],
                        }),
                );
                app.world_mut().entity_mut(entity).insert(geometry);
            } else {
                app.world_mut()
                    .entity_mut(entity)
                    .remove::<crate::materials::GeometryData>();
            }
            crate::test_support::render_until(
                &mut app,
                "scene-sample test pipelines did not prepare",
                |app| {
                    let world = app.sub_app(RenderApp).world();
                    let frame = world.resource::<FrameState>();
                    let cache = world.resource::<PipelineCache>();
                    let images = world.resource::<RenderAssets<GpuImage>>();
                    frame.frame.materials.iter().any(|material| matches!(material, Material::SceneSample { mask, depth_test: depth } if mask.is_some() == masked && *depth == depth_test))
                        && frame.frame.draws.iter().any(|draw| draw.geometry.is_some() == masked)
                        && cache.get_render_pipeline(depth_pipeline).is_some()
                        && images.get(&white).is_some()
                        && images.get(&output).is_some()
                        && frame
                            .frame
                            .passes
                            .iter()
                            .any(|pass| pass.samples == samples)
                        && frame
                            .resolved
                            .iter()
                            .flatten()
                            .any(|draw| cache.get_render_pipeline(draw.pipeline).is_some())
                },
            )?;
            if !masked {
                let state = app.sub_app(RenderApp).world().resource::<FrameState>();
                let draw = state
                    .resolved
                    .iter()
                    .flatten()
                    .next()
                    .expect("captured mesh resolved");
                assert!(
                    warmed.contains(&draw.pipeline),
                    "{mode:?} compiled on first use"
                );
            }
            let capture = |app: &App| -> anyhow::Result<Vec<u8>> {
                let world = app.sub_app(RenderApp).world();
                let renderer = world.resource::<Renderer>();
                let state = world.resource::<FrameState>();
                let cache = world.resource::<PipelineCache>();
                let device = world.resource::<RenderDevice>();
                let queue = world.resource::<RenderQueue>();
                let images = world.resource::<RenderAssets<GpuImage>>();
                // The synthetic triangle is already in clip space. All other
                // geometry and bindings are those compiled by the native renderer.
                queue.write_buffer(
                    &renderer.buffers.as_ref().unwrap().uniforms,
                    0,
                    bytemuck::cast_slice(&[Mat4::IDENTITY.to_cols_array(); 2]),
                );
                let size = images.get(&output).unwrap().texture.size();
                let depth = device
                    .create_texture(&TextureDescriptor {
                        label: Some("captured-mesh test depth"),
                        size,
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: TextureDimension::D2,
                        format: DEPTH_FORMAT,
                        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&default());
                let sampler = device.create_sampler(&SamplerDescriptor::default());
                let group = device.create_bind_group(
                    "captured-mesh test scene",
                    &cache.get_bind_group_layout(&renderer.layouts.scene[usize::from(samples > 1)]),
                    &BindGroupEntries::with_indices((
                        (0, &images.get(&white).unwrap().texture_view),
                        (1, &sampler),
                        (4, &depth),
                    )),
                );
                let mut encoder =
                    device.create_command_encoder(&CommandEncoderDescriptor::default());
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("captured-mesh test depth"),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                            view: &depth,
                            depth_ops: Some(Operations {
                                load: LoadOp::Clear(0.),
                                store: StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        ..default()
                    });
                    pass.set_pipeline(cache.get_render_pipeline(depth_pipeline).unwrap());
                    pass.draw(0..3, 0..1);
                }
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("captured-mesh test"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &images.get(&output).unwrap().texture_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::WHITE),
                                store: StoreOp::Store,
                            },
                        })],
                        ..default()
                    });
                    pass.set_bind_group(2, &group, &[]);
                    encode(
                        &mut pass,
                        state,
                        0..1,
                        &renderer.frame_group.as_ref().unwrap().1,
                        cache,
                        false,
                        if masked {
                            Phase::Particles
                        } else {
                            Phase::Captures
                        },
                    );
                }
                let readback = device.create_buffer(&BufferDescriptor {
                    label: None,
                    size: 256 * u64::from(size.height),
                    usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                encoder.copy_texture_to_buffer(
                    images.get(&output).unwrap().texture.as_image_copy(),
                    TexelCopyBufferInfo {
                        buffer: &readback,
                        layout: TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(256),
                            rows_per_image: None,
                        },
                    },
                    size,
                );
                queue.submit([encoder.finish()]);
                readback
                    .slice(..)
                    .map_async(MapMode::Read, |result| result.unwrap());
                device.poll(wgpu::PollType::wait_indefinitely())?;
                Ok(readback.slice(..).get_mapped_range()?.to_vec())
            };
            let pixels = capture(&app)?;
            let culled = reversed && !masked;
            for (x, expected) in [
                (2, if depth_test { 255 } else { 0 }),
                (6, if depth_test && samples > 1 { 128 } else { 0 }),
                (10, 0),
            ] {
                let expected = if culled { 255 } else { expected };
                let color = &pixels[4 * 256 + x * 4..4 * 256 + x * 4 + 4];
                anyhow::ensure!(
                    color[..3]
                        .iter()
                        .all(|channel| channel.abs_diff(expected) <= 1),
                    "{mode:?} masked={masked} depth={depth_test} reversed={reversed}, pixel {x}: {color:?}, expected {expected}"
                );
            }
            let equal = if masked { 0 } else { 255 };
            anyhow::ensure!(
                pixels[10 * 4..10 * 4 + 3]
                    .iter()
                    .all(|channel| *channel == equal),
                "{mode:?} equal depth: field captures require Greater, battle samples GreaterEqual"
            );
            if mode == resonance_content::menu_data::AntiAliasing::Off && masked && !depth_test {
                // Both reusable CPU frames and the GPU stream must stay current.
                // Use real draws and pixel readback through edits and visibility,
                // with the upload counters guarding the steady-frame cost.
                for _ in 0..4 {
                    app.update();
                }
                let stats = app.sub_app(RenderApp).world().resource::<Drawn>().geometry;
                assert_eq!(stats.surface_vertices, 6);
                assert_eq!(stats.surface_copied_vertices, 0);
                assert_eq!(stats.surface_upload_bytes, 0);
                let geometry = app
                    .world()
                    .get::<crate::materials::GeometryData>(entity)
                    .unwrap()
                    .clone();
                let vertex = |position: [f32; 3]| crate::materials::GeometryVertex {
                    position: Vec3::from_array(position).extend(1.).to_array(),
                    normal: [0.; 4],
                    uv: [0.5; 4],
                    color: [1.; 4],
                };
                geometry.replace(std::iter::repeat_n(vertex([2., 2., 0.5]), 6));
                crate::test_support::render_until(
                    &mut app,
                    "edited stream was not uploaded",
                    |app| {
                        app.sub_app(RenderApp)
                            .world()
                            .resource::<Drawn>()
                            .geometry
                            .surface_upload_bytes
                            == 6 * 64
                    },
                )?;
                let changed = capture(&app)?;
                assert_eq!(&changed[4 * 256 + 10 * 4..4 * 256 + 10 * 4 + 4], &[255; 4]);
                app.world_mut()
                    .entity_mut(entity)
                    .insert(Visibility::Hidden);
                crate::test_support::render_until(
                    &mut app,
                    "hidden stream remained visible",
                    |app| {
                        app.sub_app(RenderApp)
                            .world()
                            .resource::<Drawn>()
                            .draws
                            .is_empty()
                    },
                )?;
                assert_eq!(
                    app.sub_app(RenderApp)
                        .world()
                        .resource::<Drawn>()
                        .geometry
                        .surface_vertices,
                    0
                );
                geometry.replace(positions.into_iter().map(vertex));
                app.world_mut()
                    .entity_mut(entity)
                    .insert(Visibility::Inherited);
                crate::test_support::render_until(
                    &mut app,
                    "restored stream was not uploaded",
                    |app| {
                        app.sub_app(RenderApp)
                            .world()
                            .resource::<Drawn>()
                            .geometry
                            .surface_upload_bytes
                            == 3 * 64
                    },
                )?;
                let restored = capture(&app)?;
                assert_eq!(
                    &restored[4 * 256 + 10 * 4..4 * 256 + 10 * 4 + 4],
                    &[0, 0, 0, 255]
                );
                for _ in 0..4 {
                    app.update();
                }
                let stats = app.sub_app(RenderApp).world().resource::<Drawn>().geometry;
                assert_eq!(stats.surface_vertices, 3);
                assert_eq!(stats.surface_copied_vertices, 0);
                assert_eq!(stats.surface_upload_bytes, 0);
            }
        }
    }
    Ok(())
}
