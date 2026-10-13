//! Masked particles sample one completed battle image before UI composition.
use anyhow::{Context, Result, ensure};
use bevy::{
    core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d},
    mesh::{VertexAttributeValues, VertexBufferLayout},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        texture::GpuImage,
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};
use std::ops::Range;

#[derive(Clone)]
struct Batch {
    mask: Handle<Image>,
    depth_test: bool,
    vertices: Range<u32>,
}

#[derive(Component, Clone, Default, ExtractComponent)]
pub(super) struct Draws {
    vertices: Vec<f32>,
    batches: Vec<Batch>,
}

const VERTEX_FLOATS: usize = 11;
impl Draws {
    pub fn push(
        &mut self,
        mesh: &Mesh,
        mask: Handle<Image>,
        offset: [i8; 2],
        depth_test: bool,
    ) -> Result<()> {
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(VertexAttributeValues::as_float3)
            .context("scene particle positions")?;
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            anyhow::bail!("scene particle UVs are missing");
        };
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            anyhow::bail!("scene particle colors are missing");
        };
        ensure!(
            mesh.indices().is_none()
                && positions.len().is_multiple_of(3)
                && positions.len() == uvs.len()
                && positions.len() == colors.len(),
            "invalid scene particle triangles"
        );
        let start = (self.vertices.len() / VERTEX_FLOATS) as u32;
        for ((position, uv), color) in positions.iter().zip(uvs).zip(colors) {
            self.vertices.extend(position);
            self.vertices.extend(uv);
            self.vertices.extend(color);
            self.vertices.extend(offset.map(f32::from));
        }
        let end = (self.vertices.len() / VERTEX_FLOATS) as u32;
        if let Some(last) = self.batches.last_mut()
            && last.mask == mask
            && last.depth_test == depth_test
        {
            last.vertices.end = end;
        } else {
            self.batches.push(Batch {
                mask,
                depth_test,
                vertices: start..end,
            });
        }
        Ok(())
    }
}

#[derive(Default, ShaderType)]
struct Settings {
    clip_from_world: Mat4,
    dimensions: Vec4,
}

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "scene_sample.wgsl");
    app.add_plugins(ExtractComponentPlugin::<Draws>::default());
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, prepare)
        .add_systems(
            Core3d,
            render
                .in_set(Core3dSystems::PostProcess)
                .before(crate::field_refraction::Pass),
        );
}

#[derive(Resource)]
pub(super) struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    copy: CachedRenderPipelineId,
    particles: [CachedRenderPipelineId; 2],
}
impl Pipeline {
    pub fn ready(&self, cache: &PipelineCache) -> Result<bool> {
        for id in [self.copy, self.particles[0], self.particles[1]] {
            match cache.get_render_pipeline_state(id) {
                CachedPipelineState::Ok(_) => {}
                CachedPipelineState::Err(error)
                    if !crate::model_preview::gpu::shader_pending(error) =>
                {
                    anyhow::bail!("battle scene-sampling pipeline: {error}");
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
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
        "battle/scene-sample",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                uniform_buffer::<Settings>(false),
            ),
        ),
    );
    let shader = server.load("embedded://resonance_presentation/battle_view/scene_sample.wgsl");
    let vertex = VertexState {
        shader: shader.clone(),
        entry_point: Some("vertex".into()),
        buffers: vec![VertexBufferLayout {
            array_stride: (VERTEX_FLOATS * 4) as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: vec![
                VertexAttribute {
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x2,
                    offset: 12,
                    shader_location: 1,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 20,
                    shader_location: 2,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x2,
                    offset: 36,
                    shader_location: 3,
                },
            ],
        }],
        ..default()
    };
    let [copy, particle, depth_particle] = [
        (fullscreen.to_vertex_state(), "copy", None),
        (vertex.clone(), "fragment", Some(CompareFunction::Always)),
        (vertex, "fragment", Some(CompareFunction::GreaterEqual)),
    ]
    .map(|(vertex, entry, depth_compare)| {
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(format!("battle/scene-sample/{entry}").into()),
            layout: vec![layout.clone()],
            vertex,
            fragment: Some(FragmentState {
                shader: shader.clone(),
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Bgra8Unorm,
                    blend: depth_compare.map(|_| BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            depth_stencil: depth_compare.map(|compare| DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(compare),
                stencil: default(),
                bias: default(),
            }),
            ..default()
        })
    });
    commands.insert_resource(Pipeline {
        layout,
        copy,
        particles: [particle, depth_particle],
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
    });
}

#[allow(clippy::too_many_arguments)]
fn render(
    view: ViewQuery<(&ViewTarget, &ViewDepthTexture, &ExtractedView, &Draws)>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    queue: Res<RenderQueue>,
    mut vertices: Local<Option<RawBufferVec<f32>>>,
    mut settings: Local<UniformBuffer<Settings>>,
    mut context: RenderContext,
) {
    let (target, depth, view, draws) = view.into_inner();
    if draws.batches.is_empty() {
        return;
    }
    let (Some(copy), Some(particle), Some(depth_particle)) = (
        cache.get_render_pipeline(pipeline.copy),
        cache.get_render_pipeline(pipeline.particles[0]),
        cache.get_render_pipeline(pipeline.particles[1]),
    ) else {
        return;
    };
    let Some(masks) = draws
        .batches
        .iter()
        .map(|b| images.get(&b.mask))
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    settings.set(Settings {
        clip_from_world: view
            .clip_from_world
            .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse()),
        dimensions: Vec4::new(
            resonance_content::WIDTH as f32,
            resonance_content::SCENE_HEIGHT as f32,
            resonance_content::WIDTH as f32 / 2.,
            resonance_content::SCENE_HEIGHT as f32 / 2.,
        ),
    });
    settings.write_buffer(context.render_device(), &queue);
    let vertices = vertices.get_or_insert_with(|| RawBufferVec::new(BufferUsages::VERTEX));
    vertices.clear();
    vertices.extend(draws.vertices.iter().copied());
    vertices.write_buffer(context.render_device(), &queue);
    let post = target.post_process_write();
    let groups: Vec<_> = masks
        .iter()
        .map(|mask| {
            context.render_device().create_bind_group(
                "battle/scene-sample",
                &cache.get_bind_group_layout(&pipeline.layout),
                &BindGroupEntries::sequential((
                    post.source,
                    &pipeline.sampler,
                    &mask.texture_view,
                    settings.binding().unwrap(),
                )),
            )
        })
        .collect();
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("battle/scene-sample"),
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
    pass.set_pipeline(copy);
    pass.set_bind_group(0, &groups[0], &[]);
    pass.draw(0..3, 0..1);
    drop(pass);
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("battle/scene-sample/particles"),
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
    pass.set_vertex_buffer(0, *vertices.buffer().unwrap().slice(..));
    for (batch, group) in draws.batches.iter().zip(&groups) {
        pass.set_pipeline(if batch.depth_test {
            depth_particle
        } else {
            particle
        });
        pass.set_bind_group(0, group, &[]);
        pass.draw(batch.vertices.clone(), 0..1);
    }
}
