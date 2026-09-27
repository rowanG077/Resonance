//! Native transition mode 4 fades the last captured field over the live image.
use bevy::{
    core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::ViewTarget,
    },
};

#[derive(Component, Clone, ExtractComponent, ShaderType)]
struct Settings {
    opacity: Vec4,
}

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "field_dissolve.wgsl");
    app.add_plugins((
        ExtractComponentPlugin::<Settings>::default(),
        UniformComponentPlugin::<Settings>::default(),
    ))
    .add_systems(
        PostUpdate,
        sync.run_if(resource_exists::<super::field_view::Art>),
    );
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, prepare)
        .add_systems(
            Core3d,
            render
                .in_set(Core3dSystems::PostProcess)
                .after(super::field_refraction::Pass),
        );
}

fn sync(
    mut commands: Commands,
    state: super::field_view::State,
    views: Query<Entity, With<super::FieldCamera>>,
) {
    let world = &state.get().events.world;
    let alpha = world
        .scene_dissolve
        .as_ref()
        .map_or(0., |f| f.alpha(world.tick));
    for view in &views {
        commands.entity(view).insert(Settings {
            opacity: Vec4::new(alpha / 255., 0., 0., 0.),
        });
    }
}

#[derive(Resource)]
pub(super) struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    id: CachedRenderPipelineId,
}
impl Pipeline {
    pub(super) fn ready(&self, cache: &PipelineCache) -> bool {
        cache.get_render_pipeline(self.id).is_some()
    }
}
fn prepare(
    mut commands: Commands,
    device: Res<RenderDevice>,
    server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "resonance/dissolve",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<Settings>(true),
            ),
        ),
    );
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("resonance/dissolve".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: server.load("embedded://resonance_presentation/field_dissolve.wgsl"),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Bgra8Unorm,
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(Pipeline {
        layout,
        id,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
    });
}

struct History {
    camera: Entity,
    texture: Texture,
    binding: Option<(BufferId, BindGroup)>,
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn render(
    view: ViewQuery<(
        Entity,
        &ViewTarget,
        &Settings,
        &DynamicUniformIndex<Settings>,
    )>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<Settings>>,
    mut history: Local<Option<History>>,
    mut context: RenderContext,
) {
    let (camera, target, settings, index) = view.into_inner();
    let Some(gpu_pipeline) = cache.get_render_pipeline(pipeline.id) else {
        return;
    };
    let Some(buffer) = uniforms.uniforms().buffer() else {
        return;
    };
    let source = target.main_texture();
    let replace = history
        .as_ref()
        .is_none_or(|h| h.camera != camera || h.texture.size() != source.size());
    if replace {
        let texture = context.render_device().create_texture(&TextureDescriptor {
            label: Some("resonance/dissolve-history"),
            size: source.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: source.format(),
            usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        *history = Some(History {
            camera,
            texture,
            binding: None,
        });
    }
    let history = history.as_mut().unwrap();
    if replace || settings.opacity.x == 0. {
        context.command_encoder().copy_texture_to_texture(
            source.as_image_copy(),
            history.texture.as_image_copy(),
            source.size(),
        );
        return;
    }
    if history
        .binding
        .as_ref()
        .is_none_or(|(id, _)| *id != buffer.id())
    {
        let texture_view = history
            .texture
            .create_view(&TextureViewDescriptor::default());
        history.binding = Some((
            buffer.id(),
            context.render_device().create_bind_group(
                "resonance/dissolve",
                &cache.get_bind_group_layout(&pipeline.layout),
                &BindGroupEntries::sequential((
                    &texture_view,
                    &pipeline.sampler,
                    uniforms.uniforms().binding().unwrap(),
                )),
            ),
        ));
    }
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("resonance/dissolve"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target.main_texture_view(),
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            ..default()
        });
    pass.set_pipeline(gpu_pipeline);
    pass.set_bind_group(0, &history.binding.as_ref().unwrap().1, &[index.index()]);
    pass.draw(0..3, 0..1);
}
