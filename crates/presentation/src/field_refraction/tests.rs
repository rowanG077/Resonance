//! Synthetic per-sample depth exercises the actual captured-mesh render pass.
use super::*;
use crate::anti_aliasing::{AntiAliasingCapabilities, AntiAliasingPlugin, AntiAliasingSettings};
use bevy::{
    camera::{CameraOutputMode, RenderTarget},
    core_pipeline::tonemapping::Tonemapping,
    render::{
        RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        renderer::RenderAdapter,
        settings::{Backends, RenderCreation, WgpuSettings, WgpuSettingsPriority},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
};

const DEPTH_SHADER: &str = r#"
@fragment
fn fragment(@builtin(position) position:vec4<f32>,@builtin(sample_index) sample:u32)->@builtin(frag_depth) f32 {
    // Left: all samples occlude the mesh. Middle: alternating samples occlude
    // it. Right: no samples occlude it. Captured mesh depth is exactly 0.5.
    if position.y < 2. {return 0.5;}
    if position.x < 4. {return 0.75;}
    if position.x < 8. {return select(0.25,0.75,sample % 2u == 1u);}
    return 0.25;
}
"#;

#[derive(Resource)]
struct DepthShader(Handle<Shader>);
#[derive(Resource)]
struct DepthPipelines(Vec<(u32, CachedRenderPipelineId)>);
#[derive(Resource, Default)]
struct Captures(Vec<Image>);

fn prepare_depth(
    mut commands: Commands,
    device: Res<RenderDevice>,
    adapter: Res<RenderAdapter>,
    cache: Res<PipelineCache>,
    fullscreen: Res<FullscreenShader>,
    shader: Res<DepthShader>,
) {
    let features = if device
        .features()
        .contains(WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        adapter.get_texture_format_features(TextureFormat::Depth32Float)
    } else {
        TextureFormat::Depth32Float.guaranteed_format_features(device.features())
    };
    let pipelines = [1, 2, 4, 8]
        .into_iter()
        .filter(|samples| features.flags.sample_count_supported(*samples))
        .map(|samples| {
            let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("captured-mesh test depth".into()),
                vertex: fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: shader.0.clone(),
                    entry_point: Some("fragment".into()),
                    targets: vec![],
                    ..default()
                }),
                depth_stencil: Some(DepthStencilState {
                    format: TextureFormat::Depth32Float,
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
            });
            (samples, id)
        })
        .collect();
    commands.insert_resource(DepthPipelines(pipelines));
}
fn fill_depth(
    view: ViewQuery<(&ViewDepthTexture, &Msaa)>,
    pipelines: Res<DepthPipelines>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let (depth, msaa) = view.into_inner();
    let Some((_, id)) = pipelines
        .0
        .iter()
        .find(|(samples, _)| *samples == msaa.samples())
    else {
        return;
    };
    let Some(pipeline) = cache.get_render_pipeline(*id) else {
        return;
    };
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("captured-mesh test depth"),
        color_attachments: &[],
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: depth.view(),
            depth_ops: Some(Operations {
                load: LoadOp::Clear(0.),
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..default()
    });
    pass.set_render_pipeline(pipeline);
    pass.draw(0..3, 0..1);
}

#[test]
#[ignore = "requires a GPU; tests captured-scene mesh depth coverage with every supported AA mode"]
fn captured_meshes_obey_per_sample_depth_in_every_aa_mode() -> anyhow::Result<()> {
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
    .init_resource::<crate::field_audit::Applied>()
    .init_resource::<Captures>();
    bevy::shader::load_shader_library!(&mut app, "../effect_color.wgsl");
    install(&mut app);
    let shader = app
        .world_mut()
        .resource_mut::<Assets<Shader>>()
        .add(Shader::from_wgsl(
            DEPTH_SHADER,
            "captured-mesh synthetic depth",
        ));
    app.sub_app_mut(RenderApp)
        .insert_resource(DepthShader(shader))
        .add_systems(RenderStartup, prepare_depth)
        .add_systems(Core3d, fill_depth.in_set(Core3dSystems::EarlyPostProcess));
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            12,
            8,
            TextureFormat::Bgra8Unorm,
            None,
        ));
    let atlas = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_fill(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[128, 128, 128, 128],
            TextureFormat::Rgba8Unorm,
            bevy::asset::RenderAssetUsages::MAIN_WORLD
                | bevy::asset::RenderAssetUsages::RENDER_WORLD,
        ));
    // A black screen-copy triangle over a white source makes visibility
    // directly observable: white=occluded, black=visible, gray=partial coverage.
    let mut vertices = Vec::new();
    for [x, y] in [[-1., -1.], [3., -1.], [-1., 3.]] {
        vertices.extend([x, y, 0.5, 0.5, 0.5, 0., 0., 0., 1.]);
    }
    app.world_mut().spawn((
        Camera3d {
            depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING)
                .into(),
            ..default()
        },
        Msaa::Off,
        Tonemapping::None,
        RenderTarget::Image(target.clone().into()),
        Camera {
            clear_color: Color::WHITE.into(),
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::REPLACE),
                clear_color: ClearColorConfig::None,
            },
            ..default()
        },
        Settings {
            clip_from_world: Mat4::IDENTITY,
            ..default()
        },
        Atlas([atlas.clone(), atlas.clone(), atlas]),
        Pulses(vec![]),
        crate::field_capture::Meshes(vertices),
    ));
    let began = std::time::Instant::now();
    while app.plugins_state() == bevy::app::PluginsState::Adding {
        anyhow::ensure!(began.elapsed().as_secs() < 120, "GPU setup timed out");
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    app.update();
    crate::playthrough::check_exit(&app)?;
    let choices = app
        .world()
        .resource::<AntiAliasingCapabilities>()
        .available_modes()
        .to_vec();
    for choice in choices {
        app.world_mut().resource_mut::<AntiAliasingSettings>().0 = choice;
        for _ in 0..40 {
            app.update();
            crate::playthrough::check_exit(&app)?;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let count = app.world().resource::<Captures>().0.len();
        app.world_mut()
            .spawn(Screenshot::image(target.clone()))
            .observe(
                |shot: On<ScreenshotCaptured>, mut captures: ResMut<Captures>| {
                    captures.0.push(shot.image.clone())
                },
            );
        while app.world().resource::<Captures>().0.len() == count {
            anyhow::ensure!(began.elapsed().as_secs() < 120, "GPU readback timed out");
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
        let expected_middle = if choice.samples() == 1 { 0 } else { 128 };
        for (x, expected) in [(2, 255), (6, expected_middle), (10, 0)] {
            let color = image.get_pixel(x, 4).0;
            anyhow::ensure!(
                color[..3]
                    .iter()
                    .all(|channel| channel.abs_diff(expected) <= 1),
                "{choice:?} depth coverage at {x}: {color:?}, expected {expected}"
            );
        }
        anyhow::ensure!(
            image.get_pixel(10, 0).0[..3]
                .iter()
                .all(|channel| *channel == 255),
            "{choice:?} must retain strict Greater depth comparison at equal depth"
        );
        println!("{choice:?}: captured-mesh occlusion and fractional coverage passed");
    }
    Ok(())
}
