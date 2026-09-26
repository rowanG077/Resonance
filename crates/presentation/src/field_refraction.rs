//! World-space ripples sample the completed scene before dialogue is composited.
use super::{
    field_audit::{Applied, Request},
    field_effects::Artwork,
    field_view::State,
};
use bevy::{
    core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::GpuImage,
        view::{ViewDepthTexture, ViewTarget},
    },
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Resource, Clone, Default)]
pub(super) struct Ready(Arc<AtomicBool>);
impl Ready {
    pub fn get(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
#[derive(Clone, Copy, Default, ShaderType)]
struct Pulse {
    position_size: Vec4,
    opacity: Vec4,
}
#[derive(Component, Clone, Default, ExtractComponent, ShaderType)]
struct Settings {
    world_from_clip: Mat4,
    clip_from_world: Mat4,
    eye: Vec4,
    uv: Vec4,
    parameters: Vec4,
    pulses: [Pulse; 16],
}
#[derive(Component, Clone, ExtractComponent)]
struct Atlas(Handle<Image>);

pub(super) fn install(app: &mut App) {
    let ready = Ready::default();
    bevy::asset::embedded_asset!(app, "field_refraction.wgsl");
    app.insert_resource(ready.clone())
        .add_plugins((
            ExtractComponentPlugin::<Atlas>::default(),
            ExtractComponentPlugin::<Settings>::default(),
            UniformComponentPlugin::<Settings>::default(),
        ))
        .add_systems(
            PostUpdate,
            sync.after(super::field_effects::render)
                .before(super::field_audit::check)
                .run_if(super::battle::field_presenting),
        );
    app.sub_app_mut(RenderApp)
        .insert_resource(ready)
        .add_systems(RenderStartup, prepare)
        .add_systems(Core3d, render.in_set(Core3dSystems::PostProcess));
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    state: State,
    art: Option<Res<Artwork>>,
    images: Res<Assets<Image>>,
    ready: Res<Ready>,
    mut applied: ResMut<Applied>,
    views: Query<(Entity, &Projection), With<super::FieldCamera>>,
) {
    let Some(art) = art else {
        for (entity, _) in &views {
            commands.entity(entity).remove::<(Settings, Atlas)>();
        }
        return;
    };
    let world = &state.get().events.world;
    let (recipe, texture) = art.refraction();
    let mut settings = Settings {
        uv: Vec4::from_array(recipe.sprite.uv),
        parameters: Vec4::new(
            world.refractions.len() as f32,
            // Authored pixel displacement remains the same fraction of the
            // scene at every output resolution.
            recipe.displacement[0] / resonance_content::WIDTH as f32,
            recipe.displacement[1] / resonance_content::SCENE_HEIGHT as f32,
            0.,
        ),
        ..default()
    };
    for (pulse, effect) in settings.pulses.iter_mut().zip(world.refractions.values()) {
        let (size, alpha) = effect.sample(world.tick);
        *pulse = Pulse {
            position_size: Vec3::from_array(effect.position).extend(size),
            opacity: Vec4::splat(alpha / 255.),
        };
    }
    let world_from_view = world
        .field_camera
        .as_ref()
        .map_or(Mat4::IDENTITY, |camera| {
            settings.eye = Vec3::from_array(camera.position).extend(1.);
            Transform::from_translation(Vec3::from_array(camera.position))
                .looking_at(Vec3::from_array(camera.target), Vec3::Z)
                .to_matrix()
        });
    for (entity, projection) in &views {
        settings.world_from_clip = world_from_view * projection.get_clip_from_view().inverse();
        settings.clip_from_world = settings.world_from_clip.inverse();
        commands
            .entity(entity)
            .insert((settings.clone(), Atlas(texture.clone())));
    }
    for &id in world.refractions.keys() {
        if ready.get() && images.contains(texture) && !views.is_empty() {
            applied.ack(Request::Refraction(id));
        } else {
            applied.loading(Request::Refraction(id));
        }
    }
}

#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    id: CachedRenderPipelineId,
}
fn prepare(
    mut commands: Commands,
    device: Res<RenderDevice>,
    server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "resonance/refraction",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                uniform_buffer::<Settings>(true),
                texture_2d(TextureSampleType::Depth),
            ),
        ),
    );
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("resonance/refraction".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: server.load("embedded://resonance_presentation/field_refraction.wgsl"),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Bgra8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(Pipeline {
        layout,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        id,
    });
}

#[derive(Default)]
struct Bindings {
    identity: Option<(TextureViewId, BufferId, TextureViewId)>,
    views: HashMap<TextureViewId, BindGroup>,
}
#[allow(clippy::too_many_arguments)]
fn render(
    view: ViewQuery<(
        &ViewTarget,
        &ViewDepthTexture,
        &Settings,
        &Atlas,
        &DynamicUniformIndex<Settings>,
    )>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<Settings>>,
    images: Res<RenderAssets<GpuImage>>,
    ready: Res<Ready>,
    mut bindings: Local<Bindings>,
    mut context: RenderContext,
) {
    let (target, depth, settings, atlas, index) = view.into_inner();
    let Some(gpu_pipeline) = cache.get_render_pipeline(pipeline.id) else {
        return;
    };
    let Some(atlas) = images.get(&atlas.0) else {
        return;
    };
    let Some(buffer) = uniforms.uniforms().buffer() else {
        return;
    };
    let identity = (atlas.texture_view.id(), buffer.id(), depth.view().id());
    if bindings.identity != Some(identity) {
        bindings.identity = Some(identity);
        bindings.views.clear();
    }
    // Warm both ping-pong bindings once, even when there is no live ripple.
    // A field with no ripple otherwise incurs no extra full-screen draw.
    if settings.parameters.x == 0.
        && bindings
            .views
            .contains_key(&target.main_texture_view().id())
    {
        return;
    }
    let post = target.post_process_write();
    for source in [post.source, post.destination] {
        bindings.views.entry(source.id()).or_insert_with(|| {
            context.render_device().create_bind_group(
                "resonance/refraction",
                &cache.get_bind_group_layout(&pipeline.layout),
                &BindGroupEntries::sequential((
                    source,
                    &pipeline.sampler,
                    &atlas.texture_view,
                    uniforms.uniforms().binding().unwrap(),
                    depth.view(),
                )),
            )
        });
    }
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("resonance/refraction"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(default()),
                    store: StoreOp::Store,
                },
            })],
            ..default()
        });
    pass.set_pipeline(gpu_pipeline);
    pass.set_bind_group(0, &bindings.views[&post.source.id()], &[index.index()]);
    pass.draw(0..3, 0..1);
    ready.0.store(true, Ordering::Release);
}
