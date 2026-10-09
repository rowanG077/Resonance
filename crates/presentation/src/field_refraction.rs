//! World-space ripples sample the completed scene before dialogue is composited.
use super::{
    field_audit::{Applied, Request},
    field_effects::{Artwork, effect_rotation},
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
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        texture::GpuImage,
        view::{ViewDepthTexture, ViewTarget},
    },
};
use resonance_content::effect::REFRACTION_LIMIT;
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
    center: Vec4,
    right: Vec4,
    up: Vec4,
    opacity: Vec4,
    tint: Vec4,
}
#[derive(Component, Clone, Default, ExtractComponent, ShaderType)]
struct Settings {
    clip_from_world: Mat4,
    uv: [Vec4; 2],
    parameters: Vec4,
    screen_copy: Vec4,
    pulses: [Pulse; REFRACTION_LIMIT],
}
#[derive(Component, Clone, ExtractComponent)]
struct Atlas([Handle<Image>; 2]);

pub(super) fn install(app: &mut App) {
    super::field_capture::install(app);
    super::field_dissolve::install(app);
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
                .before(super::field_audit::check),
        );
    app.sub_app_mut(RenderApp)
        .insert_resource(ready)
        .add_systems(RenderStartup, prepare)
        .add_systems(
            Core3d,
            render.in_set(Core3dSystems::PostProcess).in_set(Pass),
        );
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Pass;

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
    let (recipe, air, textures) = art.refraction();
    let mut settings = Settings {
        uv: [recipe.sprite.uv, air.uv].map(Vec4::from_array),
        parameters: Vec4::new(
            world.refractions.len() as f32,
            // Normalize authored displacement across output resolutions.
            recipe.displacement[0] / resonance_content::WIDTH as f32,
            recipe.displacement[1] / resonance_content::SCENE_HEIGHT as f32,
            1. / super::camera::RASTER_SUBDIVISIONS,
        ),
        screen_copy: Vec4::new(
            world.screen_copy_depth[0],
            world.screen_copy_depth[1],
            super::camera::FIELD_NEAR / super::camera::FIELD_FAR,
            0.,
        ),
        ..default()
    };
    let camera = world
        .field_camera
        .as_ref()
        .map_or(Transform::IDENTITY, super::field_view::camera_transform);
    let view_from_world = camera.to_matrix().inverse();
    let mut effects: Vec<_> = world.refractions.values().collect();
    effects.sort_unstable_by_key(|effect| effect.draw_order);
    for (entity, projection) in &views {
        settings.clip_from_world = projection.get_clip_from_view() * view_from_world;
        for (pulse, effect) in settings.pulses.iter_mut().zip(&effects) {
            *pulse = Pulse::default();
            let size = (effect.size / 2.).trunc() * 2.;
            if size <= 0. {
                continue;
            }
            let rotation = effect_rotation(effect.orientation, effect.rotation, camera.rotation);
            *pulse = Pulse {
                center: settings.clip_from_world * Vec3::from_array(effect.position).extend(1.),
                right: settings.clip_from_world * (rotation * Vec3::X * size).extend(0.),
                up: settings.clip_from_world * (rotation * Vec3::Y * size).extend(0.),
                opacity: Vec4::new(
                    effect.alpha(world.tick) / 255.,
                    effect.image as u8 as f32,
                    0.,
                    0.,
                ),
                tint: Vec4::from_array(
                    art.palette(effect.palette)
                        .map(|v| f32::from(v) * 4. / 255.),
                ),
            };
        }
        commands
            .entity(entity)
            .insert((settings.clone(), Atlas(textures.clone())));
    }
    let loaded =
        ready.get() && textures.iter().all(|texture| images.contains(texture)) && !views.is_empty();
    for &id in world.refractions.keys() {
        if loaded {
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
    ripple: CachedRenderPipelineId,
    mesh: CachedRenderPipelineId,
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
            ShaderStages::VERTEX_FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                uniform_buffer::<Settings>(true),
                texture_2d(TextureSampleType::Depth),
                texture_2d(TextureSampleType::Float { filterable: true }),
            ),
        ),
    );
    let shader = server.load("embedded://resonance_presentation/field_refraction.wgsl");
    let definitions = vec![bevy::shader::ShaderDefVal::UInt(
        "REFRACTION_LIMIT".into(),
        REFRACTION_LIMIT as u32,
    )];
    let [id, ripple] = [
        (fullscreen.to_vertex_state(), "fragment", None),
        (
            VertexState {
                shader: shader.clone(),
                shader_defs: definitions.clone(),
                entry_point: Some("quad_vertex".into()),
                ..default()
            },
            "quad_fragment",
            Some(BlendState::ALPHA_BLENDING),
        ),
    ]
    .map(|(vertex, entry_point, blend)| {
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(format!("resonance/refraction/{entry_point}").into()),
            layout: vec![layout.clone()],
            vertex,
            fragment: Some(FragmentState {
                shader: shader.clone(),
                shader_defs: definitions.clone(),
                entry_point: Some(entry_point.into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Bgra8Unorm,
                    blend,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            ..default()
        })
    });
    let mesh = cache.queue_render_pipeline(super::field_capture::pipeline(
        layout.clone(),
        server.load("embedded://resonance_presentation/field_capture.wgsl"),
    ));
    commands.insert_resource(Pipeline {
        layout,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        id,
        ripple,
        mesh,
    });
}

#[derive(Default)]
struct Bindings {
    identity: Option<([TextureViewId; 2], BufferId, TextureViewId)>,
    views: HashMap<TextureViewId, BindGroup>,
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn render(
    view: ViewQuery<(
        &ViewTarget,
        &ViewDepthTexture,
        &Settings,
        &Atlas,
        &DynamicUniformIndex<Settings>,
        &super::field_capture::Meshes,
    )>,
    pipeline: Res<Pipeline>,
    dissolve: Res<super::field_dissolve::Pipeline>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<Settings>>,
    images: Res<RenderAssets<GpuImage>>,
    ready: Res<Ready>,
    mut bindings: Local<Bindings>,
    queue: Res<RenderQueue>,
    mut mesh_buffer: Local<Option<RawBufferVec<f32>>>,
    mut context: RenderContext,
) {
    let (target, depth, settings, atlas, index, meshes) = view.into_inner();
    if !dissolve.ready(&cache) {
        return;
    }
    let Some(gpu_pipeline) = cache.get_render_pipeline(pipeline.id) else {
        return;
    };
    let Some(mesh_pipeline) = cache.get_render_pipeline(pipeline.mesh) else {
        return;
    };
    let Some(ripple_pipeline) = cache.get_render_pipeline(pipeline.ripple) else {
        return;
    };
    let [Some(atlas), Some(air)] = atlas.0.each_ref().map(|handle| images.get(handle)) else {
        return;
    };
    let Some(buffer) = uniforms.uniforms().buffer() else {
        return;
    };
    let identity = (
        [atlas.texture_view.id(), air.texture_view.id()],
        buffer.id(),
        depth.view().id(),
    );
    if bindings.identity != Some(identity) {
        bindings.identity = Some(identity);
        bindings.views.clear();
    }
    // Warm both ping-pong bindings before skipping inactive frames.
    if settings.parameters.x == 0.
        && meshes.0.is_empty()
        && settings.screen_copy.x == 0.
        && settings.screen_copy.y == 0.
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
                    &air.texture_view,
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
    pass.set_pipeline(ripple_pipeline);
    pass.draw(0..6, 0..settings.parameters.x as u32);
    drop(pass);
    if !meshes.0.is_empty() {
        let buffer = mesh_buffer.get_or_insert_with(|| RawBufferVec::new(BufferUsages::VERTEX));
        buffer.clear();
        buffer.extend(meshes.0.iter().copied());
        buffer.write_buffer(context.render_device(), &queue);
        let mut pass = context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("resonance/captured-mesh"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: post.destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: depth.view(),
                    depth_ops: None,
                    stencil_ops: None,
                }),
                ..default()
            });
        pass.set_pipeline(mesh_pipeline);
        pass.set_bind_group(0, &bindings.views[&post.source.id()], &[index.index()]);
        pass.set_vertex_buffer(0, *buffer.buffer().unwrap().slice(..));
        pass.draw(0..super::field_capture::vertex_count(meshes), 0..1);
    }
    ready.0.store(true, Ordering::Release);
}
