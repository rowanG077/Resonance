//! Retained sprite passes in Bevy's render graph. Immutable layers render once;
//! transient textures and storage buffers are pooled between exposures.
use crate::drawing::{self, BlendMode, Content, Shape};
use bevy::{
    core_pipeline::schedule::camera_driver,
    image::ImageSampler,
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{binding_types::*, *},
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderQueue},
        texture::GpuImage,
    },
};
use std::collections::{HashMap, HashSet};

// Keep these shape tags and the parameter layout aligned with yggdrasil.wgsl.
const IMAGE: u32 = 0;
const SOLID: u32 = 1;
const ELLIPSE: u32 = 2;
const RADIAL: u32 = 3;
const CLEAN_IMAGE: u32 = 4;

#[derive(Clone, Copy, Debug, Default, ShaderType)]
struct Parameters {
    color: Vec4,
    region: Vec4,
    image_bounds: Vec4,
    rect: Vec4,
    radial: Vec4,
    positions: Vec4,
    stops: [Vec4; 4],
    kind: UVec4,
    corners: [Vec4; 2],
}
#[derive(Resource, Clone, Default, ExtractResource)]
pub(crate) struct Input {
    pub frame: Option<crate::renderer::Frame>,
    pub output: Handle<Image>,
}
#[derive(Resource)]
struct Pipelines {
    textures: BindGroupLayoutDescriptor,
    parameters: BindGroupLayoutDescriptor,
    ids: [CachedRenderPipelineId; 6],
}
#[derive(Clone)]
struct TextureSlot {
    texture: Texture,
    view: TextureView,
    binding: BindGroup,
    size: (u32, u32),
}
struct Batch {
    source: u64,
    blend: usize,
    start: u32,
    end: u32,
}
struct Layer {
    buffer: StorageBuffer<Vec<Parameters>>,
    binding: Option<(BufferId, BindGroup)>,
    batches: Vec<Batch>,
}
struct Entry {
    texture: TextureSlot,
    layer: Option<Layer>,
    dependencies: Vec<u64>,
    root: bool,
}
#[derive(Resource, Default)]
struct GpuYggdrasil {
    entries: HashMap<u64, Entry>,
    pool: Vec<Entry>,
    fallback: Option<TextureSlot>,
    last: Option<(u64, TextureId)>,
}
const MAX_POOLED_LAYERS: usize = 128;
const BLENDS: [BlendMode; 6] = [
    BlendMode::SrcOver,
    BlendMode::SrcIn,
    BlendMode::SrcATop,
    BlendMode::DstIn,
    BlendMode::DstOut,
    BlendMode::Screen,
];

pub(crate) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "yggdrasil.wgsl");
    app.init_resource::<Input>()
        .add_plugins(ExtractResourcePlugin::<Input>::default());
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, initialize)
        .add_systems(RenderGraph, draw.before(camera_driver));
    #[cfg(test)]
    app.sub_app_mut(RenderApp).init_resource::<CaptureStatus>();
}
pub(crate) fn target(w: u32, h: u32) -> Image {
    let mut im = Image::new_target_texture(w, h, TextureFormat::Rgba8UnormSrgb, None);
    im.sampler = ImageSampler::nearest();
    im.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    // The artwork is composited in encoded sRGB, then sampled by the host's
    // ordinary sRGB image path. No extra gamma conversion occurs in our passes.
    im.texture_descriptor.view_formats = &[TextureFormat::Rgba8Unorm];
    im
}
fn initialize(mut commands: Commands, cache: Res<PipelineCache>, server: Res<AssetServer>) {
    let textures = BindGroupLayoutDescriptor::new(
        "yggdrasil textures",
        &BindGroupLayoutEntries::single(
            ShaderStages::FRAGMENT,
            texture_2d(TextureSampleType::Float { filterable: false }),
        ),
    );
    let parameters = BindGroupLayoutDescriptor::new(
        "yggdrasil parameters",
        &BindGroupLayoutEntries::single(
            ShaderStages::VERTEX_FRAGMENT,
            storage_buffer_read_only::<Vec<Parameters>>(false),
        ),
    );
    let shader: Handle<Shader> = server.load("embedded://resonance_yggdrasil/yggdrasil.wgsl");
    let ids = BLENDS.map(|blend| {
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("yggdrasil sprite pass".into()),
            layout: vec![textures.clone(), parameters.clone()],
            vertex: VertexState {
                shader: shader.clone(),
                entry_point: Some("vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: shader.clone(),
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Rgba8Unorm,
                    blend: Some(blending(blend)),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    });
    commands.insert_resource(Pipelines {
        textures,
        parameters,
        ids,
    });
    commands.insert_resource(GpuYggdrasil::default());
}
fn blending(mode: BlendMode) -> BlendState {
    let component = |src_factor, dst_factor| BlendComponent {
        src_factor,
        dst_factor,
        operation: BlendOperation::Add,
    };
    let over = component(BlendFactor::One, BlendFactor::OneMinusSrcAlpha);
    let color = match mode {
        BlendMode::SrcOver => over,
        BlendMode::SrcIn => component(BlendFactor::DstAlpha, BlendFactor::Zero),
        BlendMode::SrcATop => component(BlendFactor::DstAlpha, BlendFactor::OneMinusSrcAlpha),
        BlendMode::DstIn => component(BlendFactor::Zero, BlendFactor::SrcAlpha),
        BlendMode::DstOut => component(BlendFactor::Zero, BlendFactor::OneMinusSrcAlpha),
        BlendMode::Screen => component(BlendFactor::One, BlendFactor::OneMinusSrc),
    };
    BlendState {
        color,
        alpha: if mode == BlendMode::Screen {
            over
        } else {
            color
        },
    }
}

fn slot(
    texture: Texture,
    size: (u32, u32),
    device: &RenderDevice,
    pipelines: &Pipelines,
    cache: &PipelineCache,
) -> TextureSlot {
    let view = texture.create_view(&TextureViewDescriptor {
        format: Some(TextureFormat::Rgba8Unorm),
        ..default()
    });
    let binding = device.create_bind_group(
        "yggdrasil sampled image",
        &cache.get_bind_group_layout(&pipelines.textures),
        &BindGroupEntries::single(&view),
    );
    TextureSlot {
        texture,
        view,
        binding,
        size,
    }
}
fn texture(
    size: (u32, u32),
    device: &RenderDevice,
    pipelines: &Pipelines,
    cache: &PipelineCache,
) -> TextureSlot {
    slot(
        device.create_texture(&TextureDescriptor {
            label: Some("yggdrasil retained layer"),
            size: Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::COPY_DST,
            view_formats: &[],
        }),
        size,
        device,
        pipelines,
        cache,
    )
}
fn upload(slot: &TextureSlot, pixels: &[u8], queue: &RenderQueue) {
    let mut bytes = pixels.to_vec();
    for p in bytes.chunks_exact_mut(4) {
        for k in 0..3 {
            p[k] = ((p[k] as u32 * p[3] as u32 + 127) / 255) as u8;
        }
    }
    queue.write_texture(
        slot.texture.as_image_copy(),
        &bytes,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(slot.size.0 * 4),
            rows_per_image: Some(slot.size.1),
        },
        Extent3d {
            width: slot.size.0,
            height: slot.size.1,
            depth_or_array_layers: 1,
        },
    );
}
fn collect(
    image: &drawing::Image,
    entries: &HashMap<u64, Entry>,
    seen: &mut HashSet<u64>,
    nodes: &mut Vec<drawing::Image>,
) {
    if let Content::Region { atlas, .. } = &image.0.content {
        collect(atlas, entries, seen, nodes);
        return;
    }
    if !seen.insert(image.0.id) {
        return;
    }
    if let Some(entry) = entries.get(&image.0.id) {
        fn mark(id: u64, entries: &HashMap<u64, Entry>, seen: &mut HashSet<u64>) {
            if seen.insert(id) {
                for id in &entries[&id].dependencies {
                    mark(*id, entries, seen);
                }
            }
        }
        for id in &entry.dependencies {
            mark(*id, entries, seen);
        }
    } else if let Content::Layer(draws) = &image.0.content {
        for d in draws {
            if let Shape::Image { image, .. } = &d.shape {
                collect(image, entries, seen, nodes);
            }
        }
    }
    nodes.push(image.clone());
}
#[cfg(test)]
#[derive(Resource, Default)]
pub(crate) struct CaptureStatus {
    pub completed_frame: Option<u64>,
}
#[cfg(test)]
pub(crate) fn resource_usage(world: &World) -> (usize, u64) {
    let gpu = world.resource::<GpuYggdrasil>();
    let mut textures = HashMap::new();
    let mut buffers = HashMap::new();
    for entry in gpu.entries.values().chain(&gpu.pool) {
        textures.insert(
            entry.texture.texture.id(),
            entry.texture.size.0 as u64 * entry.texture.size.1 as u64 * 4,
        );
        if let Some(buffer) = entry.layer.as_ref().and_then(|layer| layer.buffer.buffer()) {
            buffers.insert(buffer.id(), buffer.size());
        }
    }
    if let Some(fallback) = &gpu.fallback {
        textures.insert(fallback.texture.id(), 4);
    }
    (
        textures.len() + buffers.len(),
        textures.values().chain(buffers.values()).sum(),
    )
}
#[allow(clippy::too_many_arguments)]
fn draw(
    mut context: RenderContext,
    #[cfg(test)] mut capture: ResMut<CaptureStatus>,
    input: Res<Input>,
    mut gpu: ResMut<GpuYggdrasil>,
    images: Res<RenderAssets<GpuImage>>,
    pipelines: Res<Pipelines>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let Some(frame) = &input.frame else {
        return;
    };
    let Some(output) = images.get(&input.output) else {
        return;
    };
    let key = (frame.image.0.id, output.texture.id());
    if gpu.last == Some(key)
        || pipelines
            .ids
            .iter()
            .any(|id| cache.get_render_pipeline(*id).is_none())
    {
        return;
    }
    if gpu.fallback.is_none() {
        gpu.fallback = Some(texture((1, 1), &device, &pipelines, &cache));
    }
    let mut seen = HashSet::new();
    let mut nodes = Vec::new();
    collect(&frame.image, &gpu.entries, &mut seen, &mut nodes);
    let obsolete: Vec<_> = gpu
        .entries
        .keys()
        // The root must render into the host output even if cached as a child.
        // Previous roots must also be rebuilt when reused as child layers.
        .filter(|id| !seen.contains(id) || gpu.entries[*id].root || **id == frame.image.0.id)
        .copied()
        .collect();
    for id in obsolete {
        let mut entry = gpu.entries.remove(&id).unwrap();
        entry.dependencies.clear();
        if let Some(layer) = &mut entry.layer {
            layer.batches.clear();
        }
        gpu.pool.push(entry);
    }
    for image in nodes {
        let id = image.0.id;
        if gpu.entries.contains_key(&id) {
            continue;
        }
        let size = (image.width() as u32, image.height() as u32);
        let is_layer = matches!(image.0.content, Content::Layer(_));
        let root = id == frame.image.0.id;
        let pool = gpu.pool.iter().position(|e| {
            e.root == root && e.texture.size == size && e.layer.is_some() == is_layer
        });
        let mut entry = if let Some(i) = pool {
            gpu.pool.swap_remove(i)
        } else {
            let tex = if root {
                slot(output.texture.clone(), size, &device, &pipelines, &cache)
            } else {
                texture(size, &device, &pipelines, &cache)
            };
            Entry {
                texture: tex,
                layer: is_layer.then(|| Layer {
                    buffer: StorageBuffer::default(),
                    binding: None,
                    batches: Vec::new(),
                }),
                dependencies: Vec::new(),
                root,
            }
        };
        // The root is always the host's stable image, including after a resize.
        if root && entry.texture.texture.id() != output.texture.id() {
            entry.texture = slot(output.texture.clone(), size, &device, &pipelines, &cache);
        }
        if let Content::Layer(draws) = &image.0.content {
            let mut parameters = Vec::with_capacity(draws.len());
            let mut dependencies = HashSet::new();
            let layer = entry.layer.as_mut().unwrap();
            for d in draws {
                if d.rect[2] == 0. || d.rect[3] == 0. {
                    continue;
                }
                let mut p = Parameters {
                    color: Vec4::from_array(d.color),
                    region: Vec4::new(0., 0., 1., 1.),
                    rect: Vec4::from_array(d.rect),
                    ..default()
                };
                // Procedural shapes do not sample a texture. Keep the preceding
                // binding so stems between leaves do not split atlas batches.
                let unsampled_source = layer.batches.last().map_or(0, |batch| batch.source);
                let source = match &d.shape {
                    Shape::Image {
                        image,
                        source,
                        clean,
                    } => {
                        let (texture, offset) = match &image.0.content {
                            Content::Region { atlas, rect } => {
                                (atlas, [rect[0] as f32, rect[1] as f32])
                            }
                            _ => (image, [0., 0.]),
                        };
                        p.image_bounds = Vec4::new(
                            offset[0],
                            offset[1],
                            image.width() as f32,
                            image.height() as f32,
                        );
                        p.region = Vec4::new(
                            source[0] / image.width() as f32,
                            source[1] / image.height() as f32,
                            source[2] / image.width() as f32,
                            source[3] / image.height() as f32,
                        );
                        p.kind.x = if *clean { CLEAN_IMAGE } else { IMAGE };
                        dependencies.insert(texture.0.id);
                        texture.0.id
                    }
                    Shape::Solid => {
                        p.kind.x = SOLID;
                        unsampled_source
                    }
                    Shape::Ellipse => {
                        p.kind.x = ELLIPSE;
                        unsampled_source
                    }
                    Shape::Radial {
                        center,
                        radii,
                        stops,
                    } => {
                        p.kind.x = RADIAL;
                        p.kind.y = stops.len() as u32;
                        p.radial = Vec4::new(center[0], center[1], radii[0], radii[1]);
                        for (i, (t, c)) in stops.iter().enumerate() {
                            p.positions[i] = *t;
                            p.stops[i] = Vec4::from_array(c.to_array());
                        }
                        unsampled_source
                    }
                };
                let [x, y, w, h] = d.rect;
                let [a, b, tx, c, e, ty] = d.transform;
                let corners = [[x, y + h], [x + w, y + h], [x + w, y], [x, y]].map(|[px, py]| {
                    [
                        2. * (a * px + b * py + tx) / size.0 as f32 - 1.,
                        1. - 2. * (c * px + e * py + ty) / size.1 as f32,
                    ]
                });
                p.corners = [
                    Vec4::new(corners[0][0], corners[0][1], corners[1][0], corners[1][1]),
                    Vec4::new(corners[2][0], corners[2][1], corners[3][0], corners[3][1]),
                ];
                let index = parameters.len() as u32 * 6;
                let blend = BLENDS.iter().position(|b| *b == d.blend).unwrap();
                if let Some(batch) = layer
                    .batches
                    .last_mut()
                    .filter(|b| b.source == source && b.blend == blend)
                {
                    batch.end = index + 6;
                } else {
                    layer.batches.push(Batch {
                        source,
                        blend,
                        start: index,
                        end: index + 6,
                    });
                }
                parameters.push(p);
            }
            entry.dependencies = dependencies.into_iter().collect();
            if !parameters.is_empty() {
                layer.buffer.set(parameters);
                layer.buffer.write_buffer(&device, &queue);
                let buffer = layer.buffer.buffer().unwrap();
                // Bind the allocation, not the previous exposure's shorter
                // slice: the same buffer can grow within its existing capacity.
                if layer
                    .binding
                    .as_ref()
                    .is_none_or(|(id, _)| *id != buffer.id())
                {
                    layer.binding = Some((
                        buffer.id(),
                        device.create_bind_group(
                            "yggdrasil layer parameters",
                            &cache.get_bind_group_layout(&pipelines.parameters),
                            &BindGroupEntries::single(buffer.as_entire_binding()),
                        ),
                    ));
                }
            }
        } else {
            upload(&entry.texture, &image.source_pixels(), &queue);
        }
        gpu.entries.insert(id, entry);
        if is_layer {
            let entry = &gpu.entries[&id];
            let layer = entry.layer.as_ref().unwrap();
            let mut pass = context
                .command_encoder()
                .begin_render_pass(&RenderPassDescriptor {
                    label: Some("yggdrasil layer"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: &entry.texture.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(Default::default()),
                            store: StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            if let Some((_, binding)) = &layer.binding {
                pass.set_bind_group(1, binding, &[]);
                for batch in &layer.batches {
                    pass.set_pipeline(
                        cache
                            .get_render_pipeline(pipelines.ids[batch.blend])
                            .unwrap(),
                    );
                    pass.set_bind_group(
                        0,
                        if batch.source == 0 {
                            &gpu.fallback.as_ref().unwrap().binding
                        } else {
                            &gpu.entries[&batch.source].texture.binding
                        },
                        &[],
                    );
                    pass.draw(batch.start..batch.end, 0..1);
                }
            }
            drop(pass);
        }
    }
    // Pools retain at most the high-water mark of a scene, not the journey.
    gpu.pool.truncate(MAX_POOLED_LAYERS);
    gpu.last = Some(key);
    #[cfg(test)]
    {
        capture.completed_frame = Some(key.0);
    }
}
