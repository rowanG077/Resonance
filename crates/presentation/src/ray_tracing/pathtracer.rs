//! Bevy's reference path integrator, adapted to frozen animation frames and
//! the authored transparent pass. No ReSTIR reservoirs or screen-space GI.
use super::denoise::Accumulation;
use bevy::{
    camera::Hdr,
    core_pipeline::{
        Core3dSystems,
        core_3d::{main_opaque_pass_3d, main_transparent_pass_3d},
        prepass::{DeferredPrepass, DepthPrepass},
        schedule::Core3d,
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_resource::{binding_types::*, *},
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        view::{ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
    solari::scene::RaytracingSceneBindings,
};
use std::sync::atomic::Ordering;

#[derive(Component, Clone, ExtractComponent)]
#[require(Hdr, DepthPrepass, DeferredPrepass)]
pub(crate) struct Pathtraced;

#[derive(Component)]
struct History {
    view: TextureView,
    size: UVec2,
    generation: Option<u32>,
    samples: u32,
}

#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedComputePipelineId,
}

pub(crate) fn install(app: &mut App) {
    app.world_mut().resource_mut::<super::State>().pathtracing = true;
    bevy::asset::embedded_asset!(app, "pathtracer.wgsl");
    app.add_plugins(ExtractComponentPlugin::<Pathtraced>::default());
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, prepare_pipeline)
        .add_systems(
            Render,
            prepare_history.in_set(RenderSystems::PrepareResources),
        )
        .add_systems(
            Core3d,
            pathtrace
                .in_set(Core3dSystems::MainPass)
                .after(main_opaque_pass_3d)
                .before(main_transparent_pass_3d),
        );
}

fn prepare_pipeline(
    mut commands: Commands,
    server: Res<AssetServer>,
    cache: Res<PipelineCache>,
    scene: Option<Res<RaytracingSceneBindings>>,
) {
    let Some(scene) = scene else { return };
    let layout = BindGroupLayoutDescriptor::new(
        "resonance/pathtracer",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_storage_2d(TextureFormat::Rgba32Float, StorageTextureAccess::ReadWrite),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<UVec4>(false),
            ),
        ),
    );
    let id = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("resonance/pathtracer".into()),
        layout: vec![scene.bind_group_layout.clone(), layout.clone()],
        shader: server.load("embedded://resonance_presentation/ray_tracing/pathtracer.wgsl"),
        ..default()
    });
    commands.insert_resource(Pipeline { layout, id });
}

fn prepare_history(
    mut commands: Commands,
    views: Query<(Entity, &ExtractedCamera, Option<&History>), With<Pathtraced>>,
    device: Res<RenderDevice>,
) {
    for (entity, camera, history) in &views {
        let Some(size) = camera.physical_viewport_size else {
            continue;
        };
        if history.is_some_and(|h| h.size == size) {
            continue;
        }
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("resonance/pathtracer-history"),
            size: Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba32Float,
            usage: TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        commands.entity(entity).insert(History {
            view: texture.create_view(&default()),
            size,
            generation: None,
            samples: 0,
        });
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn pathtrace(
    view: ViewQuery<
        (
            &ViewTarget,
            &ViewUniformOffset,
            &mut History,
            Option<&Accumulation>,
        ),
        With<Pathtraced>,
    >,
    pipeline: Option<Res<Pipeline>>,
    cache: Res<PipelineCache>,
    scene: Option<Res<RaytracingSceneBindings>>,
    uniforms: Res<ViewUniforms>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut context: RenderContext,
) {
    let (target, offset, mut history, accumulation) = view.into_inner();
    let (Some(pipeline), Some(scene)) = (pipeline, scene) else {
        return;
    };
    let (Some(gpu_pipeline), Some(scene_binding), Some(view_binding)) = (
        cache.get_compute_pipeline(pipeline.id),
        &scene.bind_group,
        uniforms.uniforms.binding(),
    ) else {
        return;
    };
    let generation = accumulation.map(|a| a.generation);
    if generation.is_none() || history.generation != generation {
        history.samples = 0;
        history.generation = generation;
    }
    // Batch independent camera paths to avoid repeating scene extraction and
    // raster/UI work once for every sample of a frozen video frame.
    let batch = accumulation.map_or(1, |a| a.samples.saturating_sub(history.samples).min(8));
    let mut settings = UniformBuffer::from(UVec4::new(history.samples, batch, 0, 0));
    settings.write_buffer(&device, &queue);
    let binding = device.create_bind_group(
        "resonance/pathtracer",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            &history.view,
            target.get_unsampled_color_attachment().view,
            view_binding,
            settings.binding().unwrap(),
        )),
    );
    let mut pass = context
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("resonance/pathtracer"),
            ..default()
        });
    pass.set_pipeline(gpu_pipeline);
    pass.set_bind_group(0, scene_binding, &[]);
    pass.set_bind_group(1, &binding, &[offset.offset]);
    pass.dispatch_workgroups(history.size.x.div_ceil(8), history.size.y.div_ceil(8), 1);
    history.samples += batch;
    if let Some(accumulation) = accumulation {
        accumulation
            .completed
            .store(history.samples, Ordering::Release);
    }
}
