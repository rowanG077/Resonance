//! Filter opaque room/character illumination before cutouts, billboards and UI.
//! Offline captures may accumulate a frozen game tick without temporal ghosting.
use super::ModernView;
use bevy::{
    core_pipeline::{
        Core3dSystems,
        core_3d::{main_opaque_pass_3d, main_transparent_pass_3d},
        prepass::ViewPrepassTextures,
        schedule::Core3d,
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_resource::{binding_types::*, *},
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        view::ViewTarget,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

#[derive(Component, Clone, ExtractComponent)]
pub(crate) struct Accumulation {
    pub(super) generation: u32,
    pub(super) samples: u32,
    pub(super) completed: Arc<AtomicU32>,
}
impl Accumulation {
    pub(crate) fn new(generation: u32, samples: u32) -> Self {
        Self {
            generation,
            samples,
            completed: Arc::default(),
        }
    }
    pub(crate) fn completed(&self) -> u32 {
        self.completed.load(Ordering::Acquire)
    }
}

#[derive(Component)]
struct History {
    view: TextureView,
    size: Extent3d,
    generation: Option<u32>,
    samples: u32,
}
#[derive(Resource)]
struct Pipeline {
    accumulate_layout: BindGroupLayoutDescriptor,
    filter_layout: BindGroupLayoutDescriptor,
    accumulate: CachedComputePipelineId,
    filter: CachedComputePipelineId,
}
pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "denoise.wgsl");
    app.add_plugins(ExtractComponentPlugin::<Accumulation>::default());
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, prepare_pipeline)
        .add_systems(
            Render,
            prepare_history.in_set(RenderSystems::PrepareResources),
        )
        .add_systems(
            Core3d,
            denoise
                .in_set(Core3dSystems::MainPass)
                .after(main_opaque_pass_3d)
                .before(main_transparent_pass_3d),
        );
}
fn prepare_pipeline(mut commands: Commands, server: Res<AssetServer>, cache: Res<PipelineCache>) {
    let accumulate_layout = BindGroupLayoutDescriptor::new(
        "resonance/room-accumulate",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_storage_2d(TextureFormat::Rgba32Float, StorageTextureAccess::ReadWrite),
                uniform_buffer::<UVec4>(false),
            ),
        ),
    );
    let filter_layout = BindGroupLayoutDescriptor::new(
        "resonance/room-filter",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Uint),
                texture_depth_2d(),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let shader = server.load("embedded://resonance_presentation/ray_tracing/denoise.wgsl");
    let accumulate = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("resonance/room-accumulate".into()),
        layout: vec![accumulate_layout.clone()],
        shader: shader.clone(),
        shader_defs: vec!["ACCUMULATE".into()],
        entry_point: Some("accumulate".into()),
        ..default()
    });
    let filter = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("resonance/room-filter".into()),
        layout: vec![filter_layout.clone()],
        shader,
        entry_point: Some("filter_room".into()),
        ..default()
    });
    commands.insert_resource(Pipeline {
        accumulate_layout,
        filter_layout,
        accumulate,
        filter,
    });
}
#[allow(clippy::type_complexity)] // Render views with illumination history, excluding path tracing.
fn prepare_history(
    mut commands: Commands,
    views: Query<
        (Entity, &ViewPrepassTextures, Option<&History>),
        (With<ModernView>, Without<super::pathtracer::Pathtraced>),
    >,
    retired: Query<Entity, (With<History>, Without<ModernView>)>,
    device: Res<RenderDevice>,
) {
    for entity in &retired {
        commands.entity(entity).remove::<History>();
    }
    for (entity, prepass, history) in &views {
        if prepass.deferred.is_none() || history.is_some_and(|h| h.size == prepass.size) {
            continue;
        }
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("resonance/room-history"),
            size: prepass.size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba32Float,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        commands.entity(entity).insert(History {
            view: texture.create_view(&default()),
            size: prepass.size,
            generation: None,
            samples: 0,
        });
    }
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // One view's HDR history and compute resources.
fn denoise(
    view: ViewQuery<
        (
            &ViewTarget,
            &ViewPrepassTextures,
            &mut History,
            Option<&Accumulation>,
        ),
        (With<ModernView>, Without<super::pathtracer::Pathtraced>),
    >,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut context: RenderContext,
) {
    let (target, prepass, mut history, accumulation) = view.into_inner();
    // Prepass and history textures can be rebuilt in adjacent frames on resize.
    if history.size != prepass.size {
        return;
    }
    let (Some(accumulate), Some(filter), Some(gbuffer), Some(depth)) = (
        cache.get_compute_pipeline(pipeline.accumulate),
        cache.get_compute_pipeline(pipeline.filter),
        prepass.deferred_view(),
        prepass.depth_view(),
    ) else {
        return;
    };
    let generation = accumulation.map(|a| a.generation);
    if generation.is_none() || history.generation != generation {
        history.samples = 0;
        history.generation = generation;
    }
    let post = target.post_process_write();
    let limit = accumulation.map_or(1, |a| a.samples);
    let (dx, dy) = (
        history.size.width.div_ceil(8),
        history.size.height.div_ceil(8),
    );
    if history.samples < limit {
        history.samples += 1;
        let mut settings = UniformBuffer::from(UVec4::new(history.samples, 0, 0, 0));
        settings.write_buffer(&device, &queue);
        let binding = device.create_bind_group(
            "resonance/room-accumulate",
            &cache.get_bind_group_layout(&pipeline.accumulate_layout),
            &BindGroupEntries::sequential((
                post.source,
                &history.view,
                settings.binding().unwrap(),
            )),
        );
        let mut pass = context
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("resonance/room-accumulate"),
                ..default()
            });
        pass.set_pipeline(accumulate);
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(dx, dy, 1);
    }
    let binding = device.create_bind_group(
        "resonance/room-filter",
        &cache.get_bind_group_layout(&pipeline.filter_layout),
        &BindGroupEntries::sequential((&history.view, gbuffer, depth, post.destination)),
    );
    let mut pass = context
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("resonance/room-filter"),
            ..default()
        });
    pass.set_pipeline(filter);
    pass.set_bind_group(0, &binding, &[]);
    pass.dispatch_workgroups(dx, dy, 1);
    if let Some(accumulation) = accumulation {
        // The screenshot command is submitted after these GPU commands. Sharing
        // this counter prevents asset/shader startup from counting as samples.
        accumulation
            .completed
            .store(history.samples, Ordering::Release);
    }
}
