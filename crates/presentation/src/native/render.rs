//! Render-world half of the native frame renderer: one upload per data table,
//! one pass per target, state changes only where consecutive draws differ.
use super::{Draw, DrawRecord, Frame, Material, Pass, Phase, Source, Target as FrameTarget};
use crate::materials::SURFACE_CHANNELS;
use bevy::{
    mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology, VertexBufferLayout},
    platform::collections::{HashMap, HashSet},
    prelude::*,
    render::{
        Extract, ExtractSchedule, RenderApp, RenderStartup,
        mesh::{RenderMesh, RenderMeshBufferInfo, allocator::MeshAllocator},
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{
                sampler, storage_buffer_read_only_sized, texture_2d, texture_2d_multisampled,
                uniform_buffer_sized,
            },
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue},
        texture::{FallbackImage, GpuImage, TextureCache},
    },
    shader::ShaderDefVal,
};
use std::num::NonZeroU64;

#[cfg(test)]
mod preparation_tests;
#[cfg(test)]
mod scene_sample_tests;
#[cfg(test)]
mod tests;

/// Encoded size of `field_ui::coverage::Coverage`: a count and 64 quads.
pub(super) const COVERAGE_BYTES: usize = 16 + 64 * 48;
/// Uniform slots are padded to the largest offset alignment devices require.
const SLOT: u64 = 256;
const VIEW_SLOTS: u64 = 32;
const COVERAGE_SLOT: u64 = (COVERAGE_BYTES as u64).next_multiple_of(SLOT);
const COVERAGE_SLOTS: u64 = 32;
const REFRACTION_BASE: u64 = VIEW_SLOTS * SLOT;
const COVERAGE_BASE: u64 = REFRACTION_BASE + SLOT;
/// Dissolve opacity for the pass shared with `field_dissolve`.
const DISSOLVE_BASE: u64 = COVERAGE_BASE + COVERAGE_SLOTS * COVERAGE_SLOT;
const UNIFORM_BYTES: u64 = DISSOLVE_BASE + SLOT;
const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;
/// Streamed vertices: position, normal, both UV sets and color as four vec4s.
const STREAM_STRIDE: u64 = 64;

#[derive(Clone, PartialEq, Eq, Hash)]
struct PipelineKey {
    mesh: Option<MeshKey>,
    target: TargetKey,
    material: MaterialKey,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct MeshKey {
    layout: MeshVertexBufferLayoutRef,
    topology: PrimitiveTopology,
    strip_index_format: Option<IndexFormat>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TargetKey {
    samples: u32,
    format: TextureFormat,
    depth: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum MaterialKey {
    Surface {
        key: crate::materials::SurfaceKey,
        skinned: bool,
        streamed: bool,
        fog: bool,
    },
    Ui(crate::field_ui::SurfaceKey),
    Output,
    /// The model preview's image: premultiplied, as `Composite`'s pipeline.
    Composite,
    /// The title glow: added over the scene with its depth test, as
    /// `GlowMaterial`'s pipeline. It only draws in scene passes.
    Glow,
    /// Ymir's water: alpha-blended over the scene, writing depth, as the
    /// water material's pipeline. It only draws in scene passes.
    Water,
    /// The menu backdrop: alpha-blended.
    Backdrop,
    /// Title text: premultiplied alpha. It draws in scene passes, which have
    /// depth, and in output passes.
    Text,
    /// Copies the kept scene image under the overlay pass.
    Blit,
    /// The performance panel, alpha-blended into a window's output pass.
    Panel,
    Refraction {
        ripple: bool,
    },
    SceneSample {
        masked: bool,
        depth_test: bool,
    },
}

#[derive(Resource)]
struct Shaders {
    surface_vertex: Handle<Shader>,
    surface_fragment: Handle<Shader>,
    ui: Handle<Shader>,
    flat: Handle<Shader>,
    scene_sample: Handle<Shader>,
    refraction: Handle<Shader>,
}

struct Layouts {
    frame: BindGroupLayoutDescriptor,
    surface: BindGroupLayoutDescriptor,
    ui: BindGroupLayoutDescriptor,
    flat: BindGroupLayoutDescriptor,
    output: BindGroupLayoutDescriptor,
    scene: [BindGroupLayoutDescriptor; 2],
    refraction: BindGroupLayoutDescriptor,
}
impl Default for Layouts {
    fn default() -> Self {
        let texture = || texture_2d(TextureSampleType::Float { filterable: true });
        let filtering = || sampler(SamplerBindingType::Filtering);
        Self {
            refraction: BindGroupLayoutDescriptor::new(
                "resonance/native-refraction",
                &BindGroupLayoutEntries::sequential(
                    ShaderStages::VERTEX_FRAGMENT,
                    (
                        texture(),
                        texture(),
                        texture(),
                        filtering(),
                        uniform_buffer_sized(
                            false,
                            NonZeroU64::new(size_of::<crate::field_refraction::Settings>() as u64),
                        ),
                        storage_buffer_read_only_sized(
                            false,
                            NonZeroU64::new(size_of::<crate::field_refraction::Pulse>() as u64),
                        ),
                    ),
                ),
            ),
            scene: [false, true].map(|multisampled| {
                BindGroupLayoutDescriptor::new(
                    "resonance/scene-sample",
                    &BindGroupLayoutEntries::with_indices(
                        ShaderStages::FRAGMENT,
                        (
                            (0, texture()),
                            (1, filtering()),
                            (
                                4,
                                if multisampled {
                                    texture_2d_multisampled(TextureSampleType::Float {
                                        filterable: false,
                                    })
                                } else {
                                    texture_2d(TextureSampleType::Float { filterable: false })
                                },
                            ),
                        ),
                    ),
                )
            }),
            frame: BindGroupLayoutDescriptor::new(
                "resonance/native-frame",
                &BindGroupLayoutEntries::sequential(
                    ShaderStages::VERTEX_FRAGMENT,
                    (
                        uniform_buffer_sized(true, NonZeroU64::new(128)),
                        uniform_buffer_sized(true, NonZeroU64::new(32)),
                        storage_buffer_read_only_sized(false, None),
                        storage_buffer_read_only_sized(false, None),
                        storage_buffer_read_only_sized(false, None),
                        storage_buffer_read_only_sized(false, None),
                    ),
                ),
            ),
            surface: BindGroupLayoutDescriptor::new(
                "resonance/native-surface",
                &BindGroupLayoutEntries::with_indices(
                    ShaderStages::FRAGMENT,
                    (
                        (0, texture()),
                        (1, filtering()),
                        (2, texture()),
                        (3, filtering()),
                        (6, texture()),
                        (7, filtering()),
                    ),
                ),
            ),
            ui: BindGroupLayoutDescriptor::new(
                "resonance/native-ui",
                &BindGroupLayoutEntries::sequential(
                    ShaderStages::FRAGMENT,
                    (
                        texture(),
                        filtering(),
                        texture(),
                        filtering(),
                        texture(),
                        filtering(),
                        uniform_buffer_sized(true, NonZeroU64::new(COVERAGE_BYTES as u64)),
                    ),
                ),
            ),
            flat: BindGroupLayoutDescriptor::new(
                "resonance/native-flat",
                &BindGroupLayoutEntries::sequential(
                    ShaderStages::FRAGMENT,
                    (texture(), filtering()),
                ),
            ),
            output: BindGroupLayoutDescriptor::new(
                "resonance/native-output",
                &BindGroupLayoutEntries::sequential(
                    ShaderStages::FRAGMENT,
                    (texture(), filtering(), texture(), filtering()),
                ),
            ),
        }
    }
}

struct Growing {
    buffer: Buffer,
    capacity: u64,
    sources: Vec<(u64, Range<u32>)>,
}
impl Growing {
    fn new(device: &RenderDevice, label: &'static str, usage: BufferUsages, capacity: u64) -> Self {
        Self {
            buffer: device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: capacity,
                usage: usage | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            capacity,
            sources: Vec::new(),
        }
    }
    /// Upload this frame's bytes, replacing the buffer when they outgrow it.
    fn write(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        label: &'static str,
        usage: BufferUsages,
        bytes: &[u8],
    ) {
        if bytes.len() as u64 > self.capacity {
            *self = Self::new(
                device,
                label,
                usage,
                (bytes.len() as u64).next_power_of_two(),
            );
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
    }
    fn write_stream<T: bytemuck::Pod>(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        label: &'static str,
        frame: &crate::materials::GeometryFrame<T>,
    ) -> usize {
        if self.sources == frame.sources {
            return 0;
        }
        let bytes = bytemuck::cast_slice(&frame.vertices);
        let uploaded = if bytes.len() as u64 > self.capacity {
            // A replacement buffer has no retained prefix or suffix.
            self.write(device, queue, label, BufferUsages::VERTEX, bytes);
            bytes.len()
        } else {
            // One copy spanning changed sources keeps small animated layers
            // from reuploading the unchanged menu around them.
            let mut changed = frame
                .sources
                .iter()
                .enumerate()
                .filter_map(|(index, source)| {
                    (self.sources.get(index) != Some(source)).then_some(source.1.clone())
                });
            if let Some(first) = changed.next() {
                let end = changed.next_back().map_or(first.end, |last| last.end);
                let stride = std::mem::size_of::<T>();
                let start = first.start as usize * stride;
                let end = end as usize * stride;
                if start < end {
                    queue.write_buffer(&self.buffer, start as u64, &bytes[start..end]);
                }
                end - start
            } else {
                0 // Only an unused tail disappeared.
            }
        };
        self.sources.clone_from(&frame.sources);
        uploaded
    }
}

struct Buffers {
    uniforms: Buffer,
    draws: Growing,
    joints: Growing,
    vertices: Growing,
    ui_vertices: Growing,
    pulses: Growing,
    /// Bound when a record table does not exist on this device.
    empty: Buffer,
}

struct Target {
    color: Option<TextureView>,
    depth: Option<TextureView>,
}

struct Prepared {
    group: BindGroup,
    resources: [u64; 8],
}

fn cached_material(
    materials: &mut HashMap<Material, Prepared>,
    material: &Material,
    resources: [u64; 8],
    build: impl FnOnce() -> BindGroup,
) -> BindGroup {
    if let Some(prepared) = materials.get(material)
        && prepared.resources == resources
    {
        return prepared.group.clone();
    }
    let group = build();
    materials.insert(
        material.clone(),
        Prepared {
            group: group.clone(),
            resources,
        },
    );
    group
}

impl Material {
    fn references_any(&self, images: &HashSet<AssetId<Image>>) -> bool {
        match self {
            Self::Surface { textures, .. } => {
                textures.iter().flatten().any(|id| images.contains(id))
            }
            Self::Ui { textures, .. } => textures.iter().any(|id| images.contains(id)),
            Self::Output { source, overlay } => {
                images.contains(source) || overlay.is_some_and(|id| images.contains(&id))
            }
            Self::Text { source }
            | Self::Backdrop { source }
            | Self::Composite { source }
            | Self::Glow { source } => images.contains(source),
            Self::Water { reflection } => images.contains(reflection),
            Self::SceneSample { mask, .. } => mask.is_some_and(|id| images.contains(&id)),
            Self::Refraction { atlas } => atlas.iter().any(|id| images.contains(id)),
        }
    }
}

struct OffscreenImages {
    size: Extent3d,
    format: TextureFormat,
    scene: Option<crate::field_dissolve::NativeScene>,
    scratch: Option<crate::field_dissolve::NativeScene>,
}

#[derive(Resource, Default)]
struct FrameState {
    frame: Frame,
    permutation: Vec<u32>,
    sorted_draws: Vec<Draw>,
    sorted_records: Vec<DrawRecord>,
    resolved: Vec<Option<Resolved>>,
    groups: Vec<Option<BindGroup>>,
}

/// Persistent GPU resources, independent of the frame being prepared and drawn.
#[derive(Resource, Default)]
struct Renderer {
    layouts: Layouts,
    buffers: Option<Buffers>,
    frame_group: Option<([BufferId; 6], BindGroup)>,
    materials: HashMap<Material, Prepared>,
    pipelines: HashMap<PipelineKey, CachedRenderPipelineId>,
    targets: HashMap<(u32, u32, u32, TextureFormat, bool), Target>,
    /// Single-sample scene image, exchanged with the dissolve history.
    scene: Option<crate::field_dissolve::NativeScene>,
    /// The other image of the pair, drawn into by a pass that reads `scene`.
    scratch: Option<crate::field_dissolve::NativeScene>,
    /// The same pair for each offscreen image size and format; their images
    /// (model preview, Ymir's reflection) need not match the scene's.
    offscreen_images: Vec<OffscreenImages>,
    smaa: crate::anti_aliasing::NativeSmaa,
    scene_groups: Vec<((TextureViewId, TextureViewId), BindGroup, BindGroup)>,
    linear: Option<Sampler>,
    repeating: Option<Sampler>,
    blits: Vec<(TextureViewId, BindGroup)>,
    /// Panel bindings, for their current textures.
    panels: Vec<(TextureViewId, BindGroup)>,
    nearest: Option<Sampler>,
    uniforms: Vec<u8>,
    /// Whether surface draws pass their material record row as an immediate;
    /// decided on the first frame, see [`immediate_slot`].
    immediate_slot: Option<bool>,
}

/// Surface draws pass their material record row to the fragment shader as an
/// immediate, when the device has them, instead of as a flat varying. The
/// values and the arithmetic are the same; a slot that is uniform across the
/// draw lets the GPU compiler load the material record and evaluate its
/// uniform math once per draw. Devices without immediates keep the varying.
fn immediate_slot(device: &RenderDevice) -> bool {
    device.features().contains(wgpu::Features::IMMEDIATES)
        && device.limits().max_immediate_size >= IMMEDIATE_BYTES
}
const IMMEDIATE_BYTES: u32 = 4;

/// What the frame drew, for scene admission: GPU preparation reads which draws
/// and pipelines were ready.
#[derive(Resource, Default)]
pub(crate) struct Drawn {
    /// Each draw encoded this frame with the pipeline it uses.
    pub draws: Vec<(bevy::render::sync_world::MainEntity, CachedRenderPipelineId)>,
    /// Draws of the compiled frame whose pipeline is still being created.
    pub pending: usize,
    pub geometry: GeometryStats,
}

/// Actual snapshot copies and GPU uploads, including unchanged frames.
#[derive(Clone, Copy, Default, serde::Serialize)]
pub(crate) struct GeometryStats {
    pub surface_vertices: usize,
    pub ui_vertices: usize,
    pub surface_copied_vertices: usize,
    pub ui_copied_vertices: usize,
    pub surface_upload_bytes: usize,
    pub ui_upload_bytes: usize,
}

/// A draw with every GPU object it needs.
struct Resolved {
    topology: PrimitiveTopology,
    pipeline: CachedRenderPipelineId,
    vertices: BufferId,
    vertex_buffer: Buffer,
    indices: Option<(Buffer, IndexFormat, Range<u32>)>,
    vertex_range: Range<u32>,
    base_vertex: i32,
}
use std::ops::Range;

pub(super) fn install(app: &mut App) {
    let server = app.world().resource::<AssetServer>();
    let shaders = Shaders {
        surface_vertex: server.load("embedded://resonance_presentation/native_surface_vertex.wesl"),
        surface_fragment: server.load("embedded://resonance_presentation/title_surface.wesl"),
        ui: server.load("embedded://resonance_presentation/field_ui.wesl"),
        flat: server.load("embedded://resonance_presentation/native_flat.wesl"),
        scene_sample: server.load("embedded://resonance_presentation/native_scene_sample.wesl"),
        refraction: server.load("embedded://resonance_presentation/field_refraction.wesl"),
    };
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .insert_resource(shaders)
        .init_resource::<Renderer>()
        .init_resource::<FrameState>()
        .init_resource::<Drawn>()
        .add_systems(RenderStartup, prepare_samples)
        .add_systems(ExtractSchedule, extract)
        .add_systems(RenderGraph, draw.in_set(RenderGraphSystems::Render));
}

/// Both battle depth modes are warmed before battle admission, even when the
/// first frame has no scene-sampling particles.
#[derive(Resource)]
pub(crate) struct SamplePipelines {
    battle: Vec<CachedRenderPipelineId>,
    refraction: [[CachedRenderPipelineId; 2]; 2],
}
impl SamplePipelines {
    pub fn ready(&self, cache: &PipelineCache) -> anyhow::Result<bool> {
        for &id in &self.battle {
            match cache.get_render_pipeline_state(id) {
                CachedPipelineState::Ok(_) => {}
                CachedPipelineState::Err(error) if !crate::gpu_prepare::shader_pending(error) => {
                    anyhow::bail!("battle scene-sampling pipeline: {error}")
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
    }
}
fn prepare_samples(
    mut commands: Commands,
    mut renderer: ResMut<Renderer>,
    shaders: Res<Shaders>,
    cache: Res<PipelineCache>,
) {
    let mut ids = Vec::new();
    for samples in [1, 2] {
        for depth_test in [false, true] {
            ids.push(
                renderer
                    .pipeline(
                        PipelineKey {
                            mesh: None,
                            target: TargetKey {
                                samples,
                                format: crate::renderer::SCENE_COLOR_FORMAT,
                                depth: false,
                            },
                            material: MaterialKey::SceneSample {
                                masked: true,
                                depth_test,
                            },
                        },
                        &shaders,
                        &cache,
                    )
                    .unwrap(),
            );
        }
    }
    ids.push(
        renderer
            .pipeline(
                PipelineKey {
                    mesh: None,
                    target: TargetKey {
                        samples: 1,
                        format: crate::renderer::SCENE_COLOR_FORMAT,
                        depth: false,
                    },
                    material: MaterialKey::Blit,
                },
                &shaders,
                &cache,
            )
            .unwrap(),
    );
    let refraction = [1, 2].map(|samples| {
        [false, true].map(|ripple| {
            renderer
                .pipeline(
                    PipelineKey {
                        mesh: None,
                        target: TargetKey {
                            samples,
                            format: crate::renderer::SCENE_COLOR_FORMAT,
                            depth: false,
                        },
                        material: MaterialKey::Refraction { ripple },
                    },
                    &shaders,
                    &cache,
                )
                .unwrap()
        })
    });
    commands.insert_resource(SamplePipelines {
        battle: ids,
        refraction,
    });
}

/// Whether the native frame's SMAA is set up to draw `mode` at `size`.
#[cfg(test)]
pub(crate) fn smaa_ready(
    app: &App,
    mode: resonance_content::menu_data::AntiAliasing,
    size: UVec2,
) -> bool {
    let render = app.sub_app(RenderApp).world();
    smaa(app).is_some_and(|smaa| smaa.ready(mode, size, render.resource::<PipelineCache>()))
}

#[cfg(test)]
pub(crate) fn smaa(app: &App) -> Option<&crate::anti_aliasing::NativeSmaa> {
    app.sub_app(RenderApp)
        .world()
        .get_resource::<Renderer>()
        .map(|renderer| &renderer.smaa)
}

/// Take the compiled frame; the main world reuses the previous one's storage.
fn extract(
    source: Extract<Res<Source>>,
    mut image_events: Extract<MessageReader<AssetEvent<Image>>>,
    mut invalidated: Local<HashSet<AssetId<Image>>>,
    mut renderer: ResMut<Renderer>,
    mut frame: ResMut<FrameState>,
    mut windows: Query<(
        bevy::render::sync_world::MainEntity,
        &mut bevy::render::view::ExtractedWindow,
    )>,
) {
    // Bind groups own GPU textures independently of their image assets. Release
    // both cached and per-frame references, including on frames with no draws.
    frame.groups.clear();
    frame.resolved.clear();
    invalidated.clear();
    invalidated.extend(image_events.read().filter_map(|event| match event {
        AssetEvent::Unused { id } | AssetEvent::Modified { id } => Some(*id),
        // Removed alone can mean a render-only image left the CPU world.
        // Like RenderAssets, keep its GPU resources until the last handle drops.
        _ => None,
    }));
    if !invalidated.is_empty() {
        renderer
            .materials
            .retain(|material, _| !material.references_any(&invalidated));
    }
    std::mem::swap(&mut *source.0.lock().unwrap(), &mut frame.frame);
    // No camera of Bevy's targets the windows this frame draws; asking for a
    // present has Bevy acquire their swap chain textures and present them
    // after the frame's commands are submitted.
    for pass in &frame.frame.passes {
        if let FrameTarget::Window(entity) = pass.target {
            for (window, mut extracted) in &mut windows {
                if window == entity {
                    extracted.needs_initial_present = true;
                }
            }
        }
    }
}

/// The color view, size and format a pass draws into, when it is available.
fn pass_target(
    target: FrameTarget,
    images: &RenderAssets<GpuImage>,
    windows: &Query<(
        bevy::render::sync_world::MainEntity,
        &bevy::render::view::ExtractedWindow,
    )>,
) -> Option<(TextureView, Extent3d, TextureFormat)> {
    match target {
        FrameTarget::Image(image) => {
            let image = images.get(image)?;
            Some((
                image.texture_view.clone(),
                image.texture_descriptor.size,
                image.texture_descriptor.format,
            ))
        }
        FrameTarget::Window(entity) => {
            let (_, window) = windows.iter().find(|(window, _)| *window == entity)?;
            Some((
                window.swap_chain_texture_view.clone()?,
                Extent3d {
                    width: window.physical_width,
                    height: window.physical_height,
                    depth_or_array_layers: 1,
                },
                window.swap_chain_texture_view_format?,
            ))
        }
    }
}

/// A flat quad's vertex attributes: position, uv and, when the mesh has
/// them, colors, whose alpha the menu backdrop applies.
fn flat_attributes(
    layout: &MeshVertexBufferLayoutRef,
) -> Vec<bevy::mesh::VertexAttributeDescriptor> {
    let mut attributes = vec![
        Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
        Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
    ];
    if layout.0.contains(Mesh::ATTRIBUTE_COLOR) {
        attributes.push(Mesh::ATTRIBUTE_COLOR.at_shader_location(5));
    }
    attributes
}

fn flat_defs(layout: &MeshVertexBufferLayoutRef) -> Vec<ShaderDefVal> {
    defs(if layout.0.contains(Mesh::ATTRIBUTE_COLOR) {
        &["VERTEX_COLORS"]
    } else {
        &[]
    })
}

fn defs(names: &[&'static str]) -> Vec<ShaderDefVal> {
    let mut defs: Vec<ShaderDefVal> = names.iter().map(|name| (*name).into()).collect();
    defs.push(ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 1));
    defs
}

fn streamed_layout(layout: Option<&MeshVertexBufferLayoutRef>) -> VertexBufferLayout {
    let mut attributes = vec![VertexAttribute {
        format: VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    }];
    for (attribute, format, offset, shader_location) in [
        (Mesh::ATTRIBUTE_NORMAL, VertexFormat::Float32x3, 16, 1),
        (Mesh::ATTRIBUTE_UV_0, VertexFormat::Float32x2, 32, 2),
        (Mesh::ATTRIBUTE_UV_1, VertexFormat::Float32x2, 40, 3),
        (Mesh::ATTRIBUTE_COLOR, VertexFormat::Float32x4, 48, 5),
    ] {
        if layout.is_none_or(|layout| layout.0.contains(attribute)) {
            attributes.push(VertexAttribute {
                format,
                offset,
                shader_location,
            });
        }
    }
    VertexBufferLayout {
        array_stride: STREAM_STRIDE,
        step_mode: VertexStepMode::Vertex,
        attributes,
    }
}

impl Renderer {
    fn pipeline(
        &mut self,
        key: PipelineKey,
        shaders: &Shaders,
        cache: &PipelineCache,
    ) -> Option<CachedRenderPipelineId> {
        if let Some(id) = self.pipelines.get(&key) {
            return Some(*id);
        }
        let TargetKey {
            samples,
            format,
            depth,
        } = key.target;
        let target = |blend| {
            vec![Some(ColorTargetState {
                format,
                blend,
                write_mask: ColorWrites::ALL,
            })]
        };
        let primitive = key
            .mesh
            .as_ref()
            .map_or_else(PrimitiveState::default, |mesh| PrimitiveState {
                topology: mesh.topology,
                strip_index_format: mesh.strip_index_format,
                ..default()
            });
        let depth_state = |write, compare| DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(write),
            depth_compare: Some(compare),
            stencil: default(),
            bias: default(),
        };
        let mut descriptor = RenderPipelineDescriptor {
            layout: vec![self.layouts.frame.clone(), self.layouts.flat.clone()],
            vertex: VertexState {
                shader: shaders.flat.clone(),
                shader_defs: defs(&[]),
                entry_point: Some("vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: shaders.flat.clone(),
                entry_point: Some("fragment".into()),
                ..default()
            }),
            multisample: MultisampleState {
                count: samples,
                ..default()
            },
            ..default()
        };
        let fragment = descriptor.fragment.as_mut().unwrap();
        let blend;
        match &key.material {
            MaterialKey::Refraction { ripple } => {
                let mut names = vec!["NATIVE_SCENE"];
                if samples > 1 {
                    names.push("MULTISAMPLED_DEPTH");
                }
                descriptor.label = Some("resonance/native-refraction".into());
                descriptor.layout[1] = self.layouts.refraction.clone();
                descriptor
                    .layout
                    .push(self.layouts.scene[usize::from(samples > 1)].clone());
                descriptor.vertex.shader_defs = defs(&names);
                descriptor.vertex.entry_point =
                    Some(if *ripple { "quad_vertex" } else { "fullscreen" }.into());
                if *ripple {
                    descriptor.vertex.shader = shaders.refraction.clone();
                }
                fragment.shader = shaders.refraction.clone();
                fragment.entry_point =
                    Some(if *ripple { "quad_fragment" } else { "fragment" }.into());
                descriptor.multisample.count = 1;
                blend = ripple.then_some(BlendState::ALPHA_BLENDING);
            }
            MaterialKey::SceneSample { masked, depth_test } => {
                let mut names = vec!["NATIVE_SCENE"];
                if *masked {
                    names.push("MASKED_SCENE");
                }
                if *depth_test {
                    names.push("DEPTH_TEST");
                }
                if samples > 1 {
                    names.push("MULTISAMPLED_DEPTH");
                }
                let buffer = if let Some(mesh) = &key.mesh {
                    let mut attributes = vec![
                        Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
                        Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
                    ];
                    if *masked {
                        attributes.push(Mesh::ATTRIBUTE_NORMAL.at_shader_location(1));
                    }
                    if mesh.layout.0.contains(Mesh::ATTRIBUTE_COLOR) {
                        attributes.push(Mesh::ATTRIBUTE_COLOR.at_shader_location(5));
                        names.push("VERTEX_COLORS");
                    }
                    mesh.layout.0.get_layout(&attributes).ok()?
                } else {
                    names.push("VERTEX_COLORS");
                    streamed_layout(None)
                };
                descriptor.label = Some("resonance/native-scene-sample".into());
                descriptor
                    .layout
                    .push(self.layouts.scene[usize::from(samples > 1)].clone());
                descriptor.vertex.shader = shaders.scene_sample.clone();
                descriptor.vertex.shader_defs = defs(&names);
                descriptor.vertex.buffers = vec![buffer];
                fragment.shader = shaders.scene_sample.clone();
                descriptor.primitive = PrimitiveState {
                    cull_mode: (!masked).then_some(Face::Back),
                    ..primitive
                };
                descriptor.multisample.count = 1;
                blend = Some(BlendState::ALPHA_BLENDING);
            }
            MaterialKey::Surface {
                key: material,
                skinned,
                streamed,
                fog,
            } => {
                let layout = &key.mesh.as_ref()?.layout;
                let state = material.native_state();
                let mut names = state.defs;
                let mut attributes = vec![Mesh::ATTRIBUTE_POSITION.at_shader_location(0)];
                for (attribute, location, def) in [
                    (Mesh::ATTRIBUTE_NORMAL, 1, "VERTEX_NORMALS"),
                    (Mesh::ATTRIBUTE_UV_0, 2, "VERTEX_UVS_A"),
                    (Mesh::ATTRIBUTE_UV_1, 3, "VERTEX_UVS_B"),
                    (Mesh::ATTRIBUTE_COLOR, 5, "VERTEX_COLORS"),
                ] {
                    if layout.0.contains(attribute) {
                        attributes.push(attribute.at_shader_location(location));
                        names.push(def);
                    }
                }
                if *skinned {
                    attributes.push(Mesh::ATTRIBUTE_JOINT_INDEX.at_shader_location(6));
                    attributes.push(Mesh::ATTRIBUTE_JOINT_WEIGHT.at_shader_location(7));
                    names.push("SKINNED");
                }
                if !*streamed && layout.0.contains(SURFACE_CHANNELS) {
                    attributes.push(SURFACE_CHANNELS.at_shader_location(8));
                    names.push("SURFACE_LAYOUT");
                }
                if *fog {
                    names.push("DISTANCE_FOG");
                }
                let immediate = self.immediate_slot == Some(true);
                if immediate {
                    names.push("NATIVE_IMMEDIATE_SLOT");
                }
                let buffer = if *streamed {
                    streamed_layout(Some(layout))
                } else {
                    layout.0.get_layout(&attributes).ok()?
                };
                descriptor.label = Some("resonance/native-surface".into());
                descriptor.layout[1] = self.layouts.surface.clone();
                descriptor.immediate_size = if immediate { IMMEDIATE_BYTES } else { 0 };
                descriptor.vertex.shader = shaders.surface_vertex.clone();
                descriptor.vertex.shader_defs = defs(&names);
                descriptor.vertex.buffers = vec![buffer];
                fragment.shader = shaders.surface_fragment.clone();
                blend = state.blend;
                descriptor.primitive = PrimitiveState {
                    cull_mode: state.cull,
                    ..primitive
                };
                descriptor.depth_stencil =
                    Some(depth_state(state.depth_write, state.depth_compare));
            }
            MaterialKey::Ui(material) => {
                let state = material.native_state(samples);
                descriptor.label = Some("resonance/native-ui".into());
                descriptor.layout[1] = self.layouts.ui.clone();
                descriptor.vertex.shader = shaders.ui.clone();
                descriptor.vertex.shader_defs = defs(&state.defs);
                descriptor.vertex.buffers = vec![state.layout];
                fragment.shader = shaders.ui.clone();
                blend = Some(state.blend);
                descriptor.primitive = primitive;
                // Ordered overlay art must not be clipped by the scene's depth.
                descriptor.depth_stencil = Some(depth_state(false, CompareFunction::Always));
            }
            MaterialKey::Blit | MaterialKey::Panel => {
                let panel = key.material == MaterialKey::Panel;
                descriptor.label = Some(
                    if panel {
                        "resonance/native-panel"
                    } else {
                        "resonance/native-blit"
                    }
                    .into(),
                );
                descriptor.vertex.entry_point = Some("fullscreen".into());
                fragment.entry_point = Some("blit".into());
                blend = panel.then_some(BlendState::ALPHA_BLENDING);
                descriptor.depth_stencil =
                    depth.then(|| depth_state(false, CompareFunction::Always));
            }
            MaterialKey::Output
            | MaterialKey::Text
            | MaterialKey::Backdrop
            | MaterialKey::Composite
            | MaterialKey::Glow
            | MaterialKey::Water => {
                let layout = &key.mesh.as_ref()?.layout;
                let (entry, blending) = match key.material {
                    MaterialKey::Text => ("text", Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING)),
                    MaterialKey::Backdrop => ("backdrop", Some(BlendState::ALPHA_BLENDING)),
                    MaterialKey::Composite => {
                        ("composite", Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING))
                    }
                    MaterialKey::Glow => (
                        "glow",
                        Some(BlendState {
                            color: BlendComponent {
                                src_factor: BlendFactor::SrcAlpha,
                                dst_factor: BlendFactor::One,
                                operation: BlendOperation::Add,
                            },
                            alpha: BlendComponent::OVER,
                        }),
                    ),
                    MaterialKey::Water => ("water", Some(BlendState::ALPHA_BLENDING)),
                    _ => ("output", None),
                };
                descriptor.label = Some(format!("resonance/native-{entry}").into());
                if key.material == MaterialKey::Output {
                    descriptor.layout[1] = self.layouts.output.clone();
                }
                descriptor.vertex.shader_defs = flat_defs(layout);
                descriptor.vertex.buffers =
                    vec![layout.0.get_layout(&flat_attributes(layout)).ok()?];
                fragment.entry_point = Some(entry.into());
                blend = blending;
                descriptor.primitive = primitive;
                descriptor.depth_stencil = match key.material {
                    MaterialKey::Glow | MaterialKey::Water => Some(depth_state(
                        key.material == MaterialKey::Water,
                        CompareFunction::Greater,
                    )),
                    // Ordered by the display list, like the overlay art.
                    MaterialKey::Text | MaterialKey::Backdrop | MaterialKey::Composite if depth => {
                        Some(depth_state(false, CompareFunction::Always))
                    }
                    _ => None,
                };
            }
        }
        fragment.shader_defs = if key.material == MaterialKey::Output {
            defs(&[])
        } else {
            descriptor.vertex.shader_defs.clone()
        };
        fragment.targets = target(blend);
        let id = cache.queue_render_pipeline(descriptor);
        self.pipelines.insert(key, id);
        Some(id)
    }

    /// The material's textures as a bind group, rebuilt when a view changes.
    fn material(
        &mut self,
        material: &Material,
        images: &RenderAssets<GpuImage>,
        fallback: &FallbackImage,
        device: &RenderDevice,
        cache: &PipelineCache,
    ) -> Option<BindGroup> {
        let image = |id: Option<AssetId<Image>>| match id {
            Some(id) => images.get(id),
            None => Some(&fallback.d2),
        };
        let ids = |list: &[&GpuImage]| {
            let mut resources = [0u64; 8];
            for (slot, image) in list.iter().enumerate() {
                resources[slot * 2] = id_bits(image.texture_view.id());
                resources[slot * 2 + 1] = id_bits(image.sampler.id());
            }
            resources
        };
        let uniforms = &self.buffers.as_ref()?.uniforms;
        Some(match material {
            Material::Refraction { atlas } => {
                let [Some(atlas), Some(air), Some(heat)] = atlas.map(|id| images.get(id)) else {
                    return None;
                };
                let repeating = self.repeating.get_or_insert_with(|| {
                    device.create_sampler(&SamplerDescriptor {
                        address_mode_u: AddressMode::Repeat,
                        address_mode_v: AddressMode::Repeat,
                        mag_filter: FilterMode::Linear,
                        min_filter: FilterMode::Linear,
                        ..default()
                    })
                });
                let pulses = &self.buffers.as_ref()?.pulses.buffer;
                let mut resources = ids(&[atlas, air, heat]);
                resources[6] = id_bits(uniforms.id());
                resources[7] = id_bits(pulses.id());
                cached_material(&mut self.materials, material, resources, || {
                    device.create_bind_group(
                        "resonance/native-refraction",
                        &cache.get_bind_group_layout(&self.layouts.refraction),
                        &BindGroupEntries::sequential((
                            &atlas.texture_view,
                            &air.texture_view,
                            &heat.texture_view,
                            &*repeating,
                            BufferBinding {
                                buffer: uniforms,
                                offset: REFRACTION_BASE,
                                size: NonZeroU64::new(
                                    size_of::<crate::field_refraction::Settings>() as u64,
                                ),
                            },
                            pulses.as_entire_buffer_binding(),
                        )),
                    )
                })
            }
            Material::Surface { textures, .. } => {
                let [color, sampling, multiply, toon] = textures.map(image);
                let (color, sampling, multiply, toon) = (color?, sampling?, multiply?, toon?);
                let layout = &self.layouts.surface;
                cached_material(
                    &mut self.materials,
                    material,
                    ids(&[color, sampling, multiply, toon]),
                    || {
                        device.create_bind_group(
                            "resonance/native-surface",
                            &cache.get_bind_group_layout(layout),
                            &BindGroupEntries::with_indices((
                                (0, &color.texture_view),
                                (1, &sampling.sampler),
                                (2, &multiply.texture_view),
                                (3, &multiply.sampler),
                                (6, &toon.texture_view),
                                (7, &toon.sampler),
                            )),
                        )
                    },
                )
            }
            Material::Ui { textures, .. } => {
                let [source, sampling, frame, mask] = textures.map(|id| images.get(id));
                let (source, sampling, frame, mask) = (source?, sampling?, frame?, mask?);
                let layout = &self.layouts.ui;
                let mut resources = ids(&[source, sampling, frame, mask]);
                resources[7] ^= id_bits(uniforms.id());
                cached_material(&mut self.materials, material, resources, || {
                    device.create_bind_group(
                        "resonance/native-ui",
                        &cache.get_bind_group_layout(layout),
                        &BindGroupEntries::sequential((
                            &source.texture_view,
                            &sampling.sampler,
                            &frame.texture_view,
                            &frame.sampler,
                            &mask.texture_view,
                            &mask.sampler,
                            BufferBinding {
                                buffer: uniforms,
                                offset: COVERAGE_BASE,
                                size: NonZeroU64::new(COVERAGE_BYTES as u64),
                            },
                        )),
                    )
                })
            }
            Material::Output { source, overlay } => {
                let source = images.get(*source)?;
                // Without an overlay the shader ignores the second texture.
                let overlay = match overlay {
                    Some(overlay) => images.get(*overlay)?,
                    None => source,
                };
                let layout = &self.layouts.output;
                cached_material(
                    &mut self.materials,
                    material,
                    ids(&[source, overlay]),
                    || {
                        device.create_bind_group(
                            "resonance/native-output",
                            &cache.get_bind_group_layout(layout),
                            &BindGroupEntries::sequential((
                                &source.texture_view,
                                &source.sampler,
                                &overlay.texture_view,
                                &overlay.sampler,
                            )),
                        )
                    },
                )
            }
            Material::SceneSample { mask, .. } => {
                let source = image(*mask)?;
                cached_material(&mut self.materials, material, ids(&[source]), || {
                    device.create_bind_group(
                        "resonance/native-scene-mask",
                        &cache.get_bind_group_layout(&self.layouts.flat),
                        &BindGroupEntries::sequential((&source.texture_view, &source.sampler)),
                    )
                })
            }
            Material::Text { source }
            | Material::Backdrop { source }
            | Material::Composite { source }
            | Material::Glow { source }
            | Material::Water { reflection: source } => {
                let source = images.get(*source)?;
                let layout = &self.layouts.flat;
                cached_material(&mut self.materials, material, ids(&[source]), || {
                    device.create_bind_group(
                        "resonance/native-flat",
                        &cache.get_bind_group_layout(layout),
                        &BindGroupEntries::sequential((&source.texture_view, &source.sampler)),
                    )
                })
            }
        })
    }

    fn target(
        &mut self,
        device: &RenderDevice,
        size: Extent3d,
        format: TextureFormat,
        samples: u32,
        depth: bool,
    ) -> &Target {
        self.targets
            .entry((size.width, size.height, samples, format, depth))
            .or_insert_with(|| {
                let texture = |label, format| {
                    device
                        .create_texture(&TextureDescriptor {
                            label: Some(label),
                            size,
                            mip_level_count: 1,
                            sample_count: samples,
                            dimension: TextureDimension::D2,
                            format,
                            // Scene-sampling particles read the depth image.
                            usage: TextureUsages::RENDER_ATTACHMENT
                                | TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&TextureViewDescriptor::default())
                };
                Target {
                    color: (samples > 1).then(|| texture("resonance/native-color", format)),
                    depth: depth.then(|| texture("resonance/native-depth", DEPTH_FORMAT)),
                }
            })
    }
}

fn id_bits<T: Hash>(id: T) -> u64 {
    use std::hash::BuildHasher;
    bevy::platform::hash::FixedHasher.hash_one(id)
}
use std::hash::Hash;

#[derive(bevy::ecs::system::SystemParam)]
struct Resources<'w, 's> {
    shaders: Res<'w, Shaders>,
    samples: Res<'w, SamplePipelines>,
    device: Res<'w, RenderDevice>,
    queue: Res<'w, RenderQueue>,
    cache: Res<'w, PipelineCache>,
    images: Res<'w, RenderAssets<GpuImage>>,
    meshes: Res<'w, RenderAssets<RenderMesh>>,
    allocator: Res<'w, MeshAllocator>,
    fallback: Res<'w, FallbackImage>,
    attachments: Res<'w, bevy::render::view::ViewTargetAttachments>,
    windows: Query<
        'w,
        's,
        (
            bevy::render::sync_world::MainEntity,
            &'static bevy::render::view::ExtractedWindow,
        ),
    >,
}

#[derive(bevy::ecs::system::SystemParam)]
struct Effects<'w> {
    history: Option<ResMut<'w, crate::field_dissolve::SceneHistory>>,
    dissolve: Option<Res<'w, crate::field_dissolve::Pipeline>>,
    refraction_ready: Option<Res<'w, crate::field_refraction::Ready>>,
    smaa: crate::anti_aliasing::NativeSmaaContext<'w>,
}

fn draw(
    mut renderer: ResMut<Renderer>,
    mut state: ResMut<FrameState>,
    resources: Resources,
    mut effects: Effects,
    mut drawn: ResMut<Drawn>,
    mut textures: ResMut<TextureCache>,
    mut context: RenderContext,
) {
    renderer.immediate_slot.get_or_insert_with(|| {
        let enabled = immediate_slot(&resources.device);
        if enabled {
            info!("Native surface draws pass their material row as an immediate");
        }
        enabled
    });
    drawn.draws.clear();
    drawn.pending = 0;
    drawn.geometry = default();
    retire_targets(&mut renderer, &state.frame, &resources);
    if state.frame.passes.is_empty() {
        return;
    }
    let state = &mut *state;
    sort(state, &resources.meshes);
    let [surface_upload_bytes, ui_upload_bytes] =
        upload_frame(&mut renderer, &state.frame, &resources);
    drawn.geometry = GeometryStats {
        surface_vertices: state.frame.vertices.vertices.len(),
        ui_vertices: state.frame.ui_vertices.vertices.len(),
        surface_copied_vertices: state.frame.vertices.copied_vertices,
        ui_copied_vertices: state.frame.ui_vertices.copied_vertices,
        surface_upload_bytes,
        ui_upload_bytes,
    };
    prepare_draws(&mut renderer, state, &resources, &mut drawn);
    let encoder = context.command_encoder();
    for pass in &state.frame.passes {
        if let Some(target) = pass_output(&mut renderer, pass, &resources, &mut textures, encoder) {
            draw_pass(
                &mut renderer,
                state,
                pass,
                &target,
                &resources,
                &mut effects,
                encoder,
            );
        }
    }
}

fn retire_targets(renderer: &mut Renderer, frame: &Frame, resources: &Resources) {
    let Resources {
        images, windows, ..
    } = resources;
    if !frame.passes.iter().any(|pass| pass.smaa.is_some()) {
        renderer.smaa = default();
    }
    // Keep all configurations used by this frame (including offscreen cameras),
    // but release obsolete resolution/MSAA attachments before allocating more.
    renderer
        .targets
        .retain(|&(width, height, samples, format, depth), _| {
            frame.passes.iter().any(|pass| {
                pass.samples == samples
                    && pass.depth == depth
                    && pass_target(pass.target, images, windows).is_some_and(
                        |(_, size, target_format)| {
                            size.width == width && size.height == height && target_format == format
                        },
                    )
            })
        });
    renderer.scene_groups.retain(|((_, depth), _, _)| {
        renderer.targets.values().any(|target| {
            target
                .depth
                .as_ref()
                .is_some_and(|view| view.id() == *depth)
        })
    });
}

fn upload_frame(renderer: &mut Renderer, frame: &Frame, resources: &Resources) -> [usize; 2] {
    let Resources {
        device,
        queue,
        cache,
        ..
    } = resources;
    let storage = BufferUsages::STORAGE;
    let buffers = renderer.buffers.get_or_insert_with(|| Buffers {
        uniforms: device.create_buffer(&BufferDescriptor {
            label: Some("resonance/native-uniforms"),
            size: UNIFORM_BYTES,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        draws: Growing::new(device, "resonance/native-draws", storage, 80 * 4096),
        joints: Growing::new(device, "resonance/native-joints", storage, 64 * 4096),
        pulses: Growing::new(
            device,
            "resonance/native-refraction-pulses",
            storage,
            size_of::<crate::field_refraction::Pulse>() as u64,
        ),
        vertices: Growing::new(
            device,
            "resonance/native-vertices",
            BufferUsages::VERTEX,
            STREAM_STRIDE * 16384,
        ),
        ui_vertices: Growing::new(
            device,
            "resonance/native-ui-vertices",
            BufferUsages::VERTEX,
            60 * 16384,
        ),
        empty: device.create_buffer(&BufferDescriptor {
            label: Some("resonance/native-empty"),
            size: 256,
            usage: BufferUsages::STORAGE,
            mapped_at_creation: false,
        }),
    });

    // One upload per table.
    renderer.uniforms.clear();
    renderer.uniforms.resize(
        frame.views.len().min(VIEW_SLOTS as usize / 2) * 2 * SLOT as usize,
        0,
    );
    for (index, view) in frame.views.iter().enumerate().take(VIEW_SLOTS as usize / 2) {
        let base = index * 2 * SLOT as usize;
        let bytes = &mut renderer.uniforms[base..base + 2 * SLOT as usize];
        bytes[..64].copy_from_slice(bytemuck::cast_slice(&view.clip_from_world.to_cols_array()));
        bytes[64..128].copy_from_slice(bytemuck::cast_slice(&view.view_from_world.to_cols_array()));
        if let Some((color, range)) = view.fog {
            let fog = &mut bytes[SLOT as usize..];
            fog[..16].copy_from_slice(bytemuck::cast_slice(&color));
            fog[16..24].copy_from_slice(bytemuck::cast_slice(&range));
        }
    }
    if let Some(effect) = frame
        .passes
        .iter()
        .find_map(|pass| pass.refraction.as_ref())
    {
        let settings = bytemuck::bytes_of(&effect.settings);
        renderer
            .uniforms
            .resize(REFRACTION_BASE as usize + settings.len(), 0);
        renderer.uniforms[REFRACTION_BASE as usize..].copy_from_slice(settings);
        buffers.pulses.write(
            device,
            queue,
            "resonance/native-refraction-pulses",
            storage,
            bytemuck::cast_slice(&effect.pulses),
        );
    }
    // Views and coverage tables go up as one copy: every separate write
    // is a copy command of its own, and on Honeykrisp each one consumes
    // command memory that a busy frame then reallocates in the kernel.
    let tables = frame
        .coverage
        .as_chunks::<COVERAGE_BYTES>()
        .0
        .iter()
        .take(COVERAGE_SLOTS as usize - 1);
    let used = tables.len();
    if used > 0 {
        let end = (COVERAGE_BASE + (used as u64 + 1) * COVERAGE_SLOT) as usize;
        renderer.uniforms.resize(end, 0);
        for (slot, table) in tables.enumerate() {
            let start = (COVERAGE_BASE + (slot as u64 + 1) * COVERAGE_SLOT) as usize;
            renderer.uniforms[start..start + COVERAGE_BYTES].copy_from_slice(table);
        }
    }
    queue.write_buffer(&buffers.uniforms, 0, &renderer.uniforms);
    buffers.draws.write(
        device,
        queue,
        "resonance/native-draws",
        storage,
        bytemuck::cast_slice(&frame.records),
    );
    buffers.joints.write(
        device,
        queue,
        "resonance/native-joints",
        storage,
        bytemuck::cast_slice(&frame.joints),
    );
    let uploaded = [
        buffers
            .vertices
            .write_stream(device, queue, "resonance/native-vertices", &frame.vertices),
        buffers.ui_vertices.write_stream(
            device,
            queue,
            "resonance/native-ui-vertices",
            &frame.ui_vertices,
        ),
    ];

    let title = frame.title_states.as_ref().unwrap_or(&buffers.empty);
    let world = frame.world_states.as_ref().unwrap_or(&buffers.empty);
    let identity = [
        buffers.uniforms.id(),
        buffers.uniforms.id(),
        buffers.draws.buffer.id(),
        buffers.joints.buffer.id(),
        title.id(),
        world.id(),
    ];
    if renderer
        .frame_group
        .as_ref()
        .is_none_or(|(current, _)| *current != identity)
    {
        let group = device.create_bind_group(
            "resonance/native-frame",
            &cache.get_bind_group_layout(&renderer.layouts.frame),
            &BindGroupEntries::sequential((
                BufferBinding {
                    buffer: &buffers.uniforms,
                    offset: 0,
                    size: NonZeroU64::new(128),
                },
                BufferBinding {
                    buffer: &buffers.uniforms,
                    offset: 0,
                    size: NonZeroU64::new(32),
                },
                buffers.draws.buffer.as_entire_buffer_binding(),
                buffers.joints.buffer.as_entire_buffer_binding(),
                title.as_entire_buffer_binding(),
                world.as_entire_buffer_binding(),
            )),
        );
        renderer.frame_group = Some((identity, group));
    }
    uploaded
}

fn prepare_draws(
    renderer: &mut Renderer,
    state: &mut FrameState,
    resources: &Resources,
    drawn: &mut Drawn,
) {
    let Resources {
        device,
        cache,
        images,
        fallback,
        windows,
        ..
    } = resources;
    let FrameState {
        frame,
        groups,
        resolved,
        ..
    } = state;
    let buffers = renderer.buffers.as_ref().unwrap();
    let streams = [
        buffers.vertices.buffer.clone(),
        buffers.ui_vertices.buffer.clone(),
    ];
    // Materials once per frame, then every draw's pipeline and geometry.
    groups.clear();
    for material in &frame.materials {
        groups.push(renderer.material(material, images, fallback, device, cache));
    }
    resolved.clear();
    resolved.resize_with(frame.draws.len(), || None);
    for pass in &frame.passes {
        let Some((_, _, format)) = pass_target(pass.target, images, windows) else {
            continue;
        };
        let target = TargetKey {
            samples: pass.samples,
            depth: pass.depth,
            format,
        };
        for view in &frame.views[pass.views.start as usize..pass.views.end as usize] {
            for index in view.draws.clone() {
                let draw = &frame.draws[index as usize];
                resolved[index as usize] = resolve(
                    renderer,
                    draw,
                    &frame.materials[draw.material as usize],
                    view.fog.is_some(),
                    target,
                    &streams,
                    resources,
                );
            }
        }
    }

    for (index, draw) in frame.draws.iter().enumerate() {
        let (Some(item), Some(_)) = (&resolved[index], &groups[draw.material as usize]) else {
            continue;
        };
        match cache.get_render_pipeline_state(item.pipeline) {
            CachedPipelineState::Ok(_) => {}
            CachedPipelineState::Err(error) if !crate::gpu_prepare::shader_pending(error) => {}
            _ => drawn.pending += 1,
        }
        drawn.draws.push((draw.entity.into(), item.pipeline));
    }
}

struct PassTarget {
    output: TextureView,
    color: Option<TextureView>,
    depth: Option<TextureView>,
    size: Extent3d,
    format: TextureFormat,
}

fn pass_output(
    renderer: &mut Renderer,
    pass: &Pass,
    resources: &Resources,
    textures: &mut TextureCache,
    encoder: &mut CommandEncoder,
) -> Option<PassTarget> {
    let Resources {
        device,
        images,
        windows,
        attachments,
        ..
    } = resources;
    let (view, size, format) = pass_target(pass.target, images, windows)?;
    // A pending screenshot redirects its target into a capture texture,
    // which Bevy afterwards copies to the image or swap chain and reads
    // back.
    let capture = attachments
        .iter()
        .find_map(|(target, attachment)| match target {
            bevy::camera::NormalizedRenderTarget::Image(target)
                if FrameTarget::Image(target.handle.id()) == pass.target =>
            {
                Some(attachment.view.clone())
            }
            bevy::camera::NormalizedRenderTarget::Window(window)
                if FrameTarget::Window(window.entity()) == pass.target
                    && attachment.view.id() != view.id() =>
            {
                Some(attachment.view.clone())
            }
            _ => None,
        });
    let output = if pass.retain_output {
        // Screenshots copy their capture back to the scene image after this pass.
        // Give them the retained frame while the replacement renders offscreen.
        if let Some(capture) = capture.filter(|capture| capture.id() != view.id()) {
            let device = device.wgpu_device();
            wgpu::util::TextureBlitter::new(device, capture.texture().format())
                .copy(device, encoder, &view, &capture);
        }
        textures
            .get(
                device,
                TextureDescriptor {
                    label: Some("resonance/field-warmup"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage: TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                },
            )
            .default_view
    } else {
        capture.unwrap_or(view)
    };
    let target = renderer.target(device, size, format, pass.samples, pass.depth);
    let (color, depth) = (target.color.clone(), target.depth.clone());
    Some(PassTarget {
        output,
        color,
        depth,
        size,
        format,
    })
}

fn draw_pass(
    renderer: &mut Renderer,
    state: &FrameState,
    pass: &Pass,
    target: &PassTarget,
    resources: &Resources,
    effects: &mut Effects,
    encoder: &mut CommandEncoder,
) {
    let Resources {
        device,
        queue,
        cache,
        images,
        ..
    } = resources;
    let Effects {
        history,
        dissolve,
        refraction_ready,
        smaa,
    } = effects;
    let PassTarget {
        output,
        color,
        depth,
        size,
        format,
    } = target;
    let frame = &state.frame;
    let frame_group = renderer.frame_group.as_ref().unwrap().1.clone();
    let uniforms = renderer.buffers.as_ref().unwrap().uniforms.clone();
    let immediate = renderer.immediate_slot == Some(true);
    let [r, g, b, a] = pass.clear;
    let clear = wgpu::Color { r, g, b, a };
    let dissolving = match (&pass.dissolve, history.as_mut()) {
        // Nothing reads the kept scene before the next update, so the
        // overlay is drawn in the scene pass instead of a second one.
        (Some(effect), Some(history)) if !effect.keep && effect.opacity.x == 0. => {
            history.invalidate();
            None
        }
        (Some(effect), Some(history)) => Some((effect, history)),
        _ => None,
    };
    let sampling = pass.particles || pass.captures;
    let refraction = resources.samples.refraction[usize::from(pass.samples > 1)]
        .map(|id| cache.get_render_pipeline(id));
    let ready = dissolve
        .as_ref()
        .is_some_and(|pipeline| pipeline.ready(cache))
        && refraction.iter().all(Option::is_some);
    if ready
        && pass.depth
        && !pass.offscreen
        && let Some(signal) = refraction_ready.as_deref()
    {
        signal.mark();
    }
    let refracting = pass
        .refraction
        .as_ref()
        .filter(|effect| ready && state.groups[effect.material as usize].is_some());
    let split = pass.scene_end;
    // Both captures read the scene before any overlay, including the menu itself.
    let capturing = frame
        .scene_capture
        .as_ref()
        .filter(|_| pass.depth && !pass.offscreen);
    let menu_capture = frame
        .capture
        .as_ref()
        .filter(|_| pass.depth && !pass.offscreen);
    if dissolving.is_none()
        && menu_capture.is_none()
        && !sampling
        && refracting.is_none()
        && pass.smaa.is_none()
        && capturing.is_none()
    {
        let (color_attachment, depth_attachment) =
            pass_attachments(color, depth, output, clear, false);
        let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("resonance/native"),
            color_attachments: &[Some(color_attachment)],
            depth_stencil_attachment: depth_attachment,
            ..default()
        });
        encode(
            &mut render_pass,
            state,
            pass.views.clone(),
            &frame_group,
            cache,
            immediate,
            Phase::Scene,
        );
        if matches!(pass.target, FrameTarget::Window(_)) {
            draw_panels(
                renderer,
                frame,
                pass,
                target,
                resources,
                &frame_group,
                &mut render_pass,
            );
        }
        return;
    }
    let copy = renderer.pipeline(
        PipelineKey {
            mesh: None,
            target: TargetKey {
                samples: 1,
                format: *format,
                depth: false,
            },
            material: MaterialKey::Blit,
        },
        &resources.shaders,
        cache,
    );
    // Scene first, into an image of its own that an effect reads or keeps.
    let (scene_image, scratch_image) = if pass.offscreen {
        let index = renderer
            .offscreen_images
            .iter()
            .position(|images| images.size == *size && images.format == *format)
            .unwrap_or_else(|| {
                // Keep a few sizes; an older one is rebuilt if it returns.
                if renderer.offscreen_images.len() >= 4 {
                    renderer.offscreen_images.remove(0);
                }
                renderer.offscreen_images.push(OffscreenImages {
                    size: *size,
                    format: *format,
                    scene: None,
                    scratch: None,
                });
                renderer.offscreen_images.len() - 1
            });
        let images = &mut renderer.offscreen_images[index];
        (&mut images.scene, &mut images.scratch)
    } else {
        (&mut renderer.scene, &mut renderer.scratch)
    };
    for image in [&mut *scene_image, &mut *scratch_image] {
        if image
            .as_ref()
            .is_none_or(|image| image.texture.size() != *size || image.texture.format() != *format)
        {
            *image = Some(crate::field_dissolve::NativeScene::new(
                device, *size, *format,
            ));
        }
    }
    let scene = scene_image.as_mut().unwrap();
    // SMAA S2x draws the scene part into a readable two-sample image and
    // reconstructs both samples into the scene image, before the
    // post-processing below. Until S2x is ready the frame draws plain MSAA.
    let s2x = (pass.smaa == Some(resonance_content::menu_data::AntiAliasing::SmaaS2x)
        && pass.samples == 2)
        .then(|| {
            renderer.smaa.s2x_target(
                smaa,
                images,
                cache,
                device,
                queue,
                UVec2::new(size.width, size.height),
                *format,
            )
        })
        .flatten();
    {
        // Particles and refraction read the scene's depth after its pass.
        let keep_depth = sampling || refracting.is_some();
        let (color_attachment, depth_attachment) = match &s2x {
            Some(target) => pass_attachments(&None, depth, target, clear, keep_depth),
            None => pass_attachments(color, depth, &scene.view, clear, keep_depth),
        };
        let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("resonance/native-scene"),
            color_attachments: &[Some(color_attachment)],
            depth_stencil_attachment: depth_attachment,
            ..default()
        });
        encode(
            &mut render_pass,
            state,
            pass.views.start..split,
            &frame_group,
            cache,
            immediate,
            Phase::Scene,
        );
    }
    if s2x.is_some() {
        renderer
            .smaa
            .s2x_reconstruct(cache, encoder, &scene.texture);
    }
    // Each effect reads one completed scene. Battle samples precede field
    // refraction; field captured meshes then read the same pre-refraction image.
    for (phase, present) in [
        (Phase::Particles, pass.particles),
        (Phase::Captures, pass.captures || refracting.is_some()),
    ] {
        let Some(depth) = depth.as_ref().filter(|_| present) else {
            continue;
        };
        let effect = refracting.filter(|_| phase == Phase::Captures);
        let copy = copy.and_then(|id| cache.get_render_pipeline(id));
        if effect.is_none() && copy.is_none() {
            continue;
        }
        let identity = (scene.view.id(), depth.id());
        let index = renderer
            .scene_groups
            .iter()
            .position(|(key, _, _)| *key == identity)
            .unwrap_or_else(|| {
                let sampler = renderer.linear.get_or_insert_with(|| {
                    device.create_sampler(&SamplerDescriptor {
                        mag_filter: FilterMode::Linear,
                        min_filter: FilterMode::Linear,
                        ..default()
                    })
                });
                let scene_group = device.create_bind_group(
                    "resonance/native-scene-sample",
                    &cache.get_bind_group_layout(
                        &renderer.layouts.scene[usize::from(pass.samples > 1)],
                    ),
                    &BindGroupEntries::with_indices(((0, &scene.view), (1, &*sampler), (4, depth))),
                );
                let flat = device.create_bind_group(
                    "resonance/native-scene-copy",
                    &cache.get_bind_group_layout(&renderer.layouts.flat),
                    &BindGroupEntries::sequential((&scene.view, &*sampler)),
                );
                if renderer.scene_groups.len() >= 4 {
                    renderer.scene_groups.remove(0);
                }
                renderer.scene_groups.push((identity, scene_group, flat));
                renderer.scene_groups.len() - 1
            });
        let (_, group, flat) = &renderer.scene_groups[index];
        let scratch = scratch_image.as_mut().unwrap();
        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("resonance/native-scene-effects"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &scratch.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(default()),
                        store: StoreOp::Store,
                    },
                })],
                ..default()
            });
            render_pass.set_bind_group(0, &frame_group, &[0, SLOT as u32]);
            if let Some(effect) = effect {
                render_pass.set_bind_group(
                    1,
                    state.groups[effect.material as usize].as_ref().unwrap(),
                    &[],
                );
                render_pass.set_bind_group(2, group, &[]);
                render_pass.set_pipeline(refraction[0].unwrap());
                render_pass.draw(0..3, 0..1);
                render_pass.set_pipeline(refraction[1].unwrap());
                render_pass.draw(0..6, 0..effect.pulses.len() as u32);
            } else {
                render_pass.set_pipeline(copy.unwrap());
                render_pass.set_bind_group(1, flat, &[]);
                render_pass.draw(0..3, 0..1);
                render_pass.set_bind_group(2, group, &[]);
            }
            encode(
                &mut render_pass,
                state,
                pass.views.start..split,
                &frame_group,
                cache,
                immediate,
                phase,
            );
        }
        std::mem::swap(scene, scratch);
    }
    // Scripts may retire actors under a fade before requesting battle;
    // the shatter keeps the last visible scene.
    if let (Some(capture), Some((effect, history))) = (capturing, &dissolving)
        && effect.fading_out()
        && let Some(kept) = history.valid_texture()
    {
        capture.0.copy_native(kept, images, encoder);
    }
    let current = match (dissolve.as_deref(), dissolving) {
        (Some(pipeline), Some((effect, history))) => {
            if effect.opacity.x != 0. {
                queue.write_buffer(
                    &uniforms,
                    DISSOLVE_BASE,
                    bytemuck::cast_slice(&effect.opacity.to_array()),
                );
            }
            let view = history.native(
                effect,
                scene,
                device,
                cache,
                pipeline,
                encoder,
                &uniforms,
                DISSOLVE_BASE as u32,
            );
            let texture = if view.id() == scene.view.id() {
                scene.texture.clone()
            } else {
                history.kept_texture().unwrap().clone()
            };
            (view, texture)
        }
        _ => (scene.view.clone(), scene.texture.clone()),
    };
    // Then SMAA, after the dissolve.
    let current = match pass.smaa {
        Some(mode) => {
            let scratch = scratch_image.as_mut().unwrap();
            if renderer.smaa.encode(
                smaa,
                mode,
                images,
                cache,
                device,
                queue,
                encoder,
                &current.1,
                &scratch.texture,
            ) {
                (scratch.view.clone(), scratch.texture.clone())
            } else {
                current
            }
        }
        None => current,
    };
    // Bevy copies the field camera's image once its passes are done.
    for capture in [menu_capture, capturing.map(|c| &c.0)]
        .into_iter()
        .flatten()
    {
        capture.copy_native(&current.1, images, encoder);
    }
    let current = current.0;
    draw_overlay(renderer, state, pass, target, resources, &current, encoder);
}

fn draw_panels(
    renderer: &mut Renderer,
    frame: &Frame,
    pass: &Pass,
    target: &PassTarget,
    resources: &Resources,
    frame_group: &BindGroup,
    render_pass: &mut wgpu::RenderPass<'_>,
) {
    let Resources {
        device,
        cache,
        images,
        shaders,
        ..
    } = resources;
    let PassTarget { size, format, .. } = target;
    for panel in &frame.panels {
        let pipeline = renderer.pipeline(
            PipelineKey {
                mesh: None,
                target: TargetKey {
                    samples: pass.samples,
                    format: *format,
                    depth: pass.depth,
                },
                material: MaterialKey::Panel,
            },
            shaders,
            cache,
        );
        let view = images
            .get(panel.image)
            .map(|image| image.texture_view.clone());
        let (Some(ready), Some(view)) =
            (pipeline.and_then(|id| cache.get_render_pipeline(id)), view)
        else {
            continue;
        };
        let index = match renderer.panels.iter().position(|(id, _)| *id == view.id()) {
            Some(index) => index,
            None => {
                let nearest = renderer
                    .nearest
                    .get_or_insert_with(|| device.create_sampler(&SamplerDescriptor::default()));
                let group = device.create_bind_group(
                    "resonance/native-panel",
                    &cache.get_bind_group_layout(&renderer.layouts.flat),
                    &BindGroupEntries::sequential((&view, &*nearest)),
                );
                if renderer.panels.len() >= 4 {
                    renderer.panels.remove(0);
                }
                renderer.panels.push((view.id(), group));
                renderer.panels.len() - 1
            }
        };
        let [x, y, width, height] = panel.rect;
        let (right, bottom) = (size.width as f32, size.height as f32);
        let (x, y) = (x.min(right), y.min(bottom));
        let (width, height) = (width.min(right - x), height.min(bottom - y));
        if width > 0. && height > 0. {
            render_pass.set_pipeline(ready);
            render_pass.set_bind_group(0, frame_group, &[0, SLOT as u32]);
            render_pass.set_bind_group(1, &renderer.panels[index].1, &[]);
            render_pass.set_viewport(x, y, width, height, 0., 1.);
            render_pass.draw(0..3, 0..1);
        }
    }
}

fn draw_overlay(
    renderer: &mut Renderer,
    state: &FrameState,
    pass: &Pass,
    target: &PassTarget,
    resources: &Resources,
    current: &TextureView,
    encoder: &mut CommandEncoder,
) {
    let Resources {
        device,
        cache,
        shaders,
        ..
    } = resources;
    let PassTarget {
        color,
        depth,
        output,
        format,
        ..
    } = target;
    let frame_group = renderer.frame_group.as_ref().unwrap().1.clone();
    let immediate = renderer.immediate_slot == Some(true);
    // Then the kept image and the overlay views into the shared target.
    let blit = renderer.pipeline(
        PipelineKey {
            mesh: None,
            target: TargetKey {
                samples: pass.samples,
                format: *format,
                depth: pass.depth,
            },
            material: MaterialKey::Blit,
        },
        shaders,
        cache,
    );
    let group = match renderer.blits.iter().find(|(id, _)| *id == current.id()) {
        Some((_, group)) => group.clone(),
        None => {
            let nearest = renderer
                .nearest
                .get_or_insert_with(|| device.create_sampler(&SamplerDescriptor::default()));
            let group = device.create_bind_group(
                "resonance/native-blit",
                &cache.get_bind_group_layout(&renderer.layouts.flat),
                &BindGroupEntries::sequential((current, &*nearest)),
            );
            if renderer.blits.len() >= 4 {
                renderer.blits.remove(0);
            }
            renderer.blits.push((current.id(), group.clone()));
            group
        }
    };
    let (color_attachment, depth_attachment) =
        pass_attachments(color, depth, output, wgpu::Color::BLACK, false);
    let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("resonance/native-overlay"),
        color_attachments: &[Some(color_attachment)],
        depth_stencil_attachment: depth_attachment,
        ..default()
    });
    if let Some(blit) = blit.and_then(|id| cache.get_render_pipeline(id)) {
        render_pass.set_pipeline(blit);
        render_pass.set_bind_group(0, &frame_group, &[0, SLOT as u32]);
        render_pass.set_bind_group(1, &group, &[]);
        render_pass.draw(0..3, 0..1);
    }
    encode(
        &mut render_pass,
        state,
        pass.scene_end..pass.views.end,
        &frame_group,
        cache,
        immediate,
        Phase::Scene,
    );
}

/// Finish the transparent sort of 3D views: draws without an authored order
/// sort at the view depth of their mesh's AABB center, as in Bevy's phase.
fn sort(state: &mut FrameState, meshes: &RenderAssets<RenderMesh>) {
    let FrameState {
        frame,
        permutation,
        sorted_draws,
        sorted_records,
        ..
    } = state;
    for view in &frame.views {
        if !view.sort {
            continue;
        }
        let range = view.draws.start as usize..view.draws.end as usize;
        let row = view.view_from_world.row(2);
        for (draw, record) in frame.draws[range.clone()]
            .iter_mut()
            .zip(&frame.records[range.clone()])
        {
            if draw.depth.is_nan() {
                let center = draw
                    .mesh
                    .and_then(|id| meshes.get(id))
                    .map_or(Vec3::ZERO, |mesh| mesh.aabb_center)
                    .extend(1.);
                let [x, y, z] = record
                    .world_from_local
                    .map(|r| Vec4::from_array(r).dot(center));
                draw.depth = row.dot(Vec4::new(x, y, z, 1.));
            }
        }
        let draws = &frame.draws[range.clone()];
        permutation.clear();
        permutation.extend(0..draws.len() as u32);
        permutation.sort_by(|a, b| {
            draws[*a as usize]
                .depth
                .total_cmp(&draws[*b as usize].depth)
        });
        if permutation.iter().enumerate().all(|(i, p)| i as u32 == *p) {
            continue;
        }
        let start = range.start;
        sorted_draws.clear();
        sorted_records.clear();
        sorted_draws.extend(
            permutation
                .iter()
                .map(|p| frame.draws[start + *p as usize].clone()),
        );
        sorted_records.extend(
            permutation
                .iter()
                .map(|p| frame.records[start + *p as usize]),
        );
        frame.draws[range.clone()].clone_from_slice(sorted_draws);
        frame.records[range].copy_from_slice(sorted_records);
    }
}

/// Multisampled targets resolve into `resolve` and keep nothing else; depth
/// outlives its pass only for an effect that reads it.
fn pass_attachments<'a>(
    color: &'a Option<TextureView>,
    depth: &'a Option<TextureView>,
    resolve: &'a TextureView,
    clear: wgpu::Color,
    keep_depth: bool,
) -> (
    RenderPassColorAttachment<'a>,
    Option<RenderPassDepthStencilAttachment<'a>>,
) {
    (
        RenderPassColorAttachment {
            view: color.as_ref().unwrap_or(resolve),
            depth_slice: None,
            resolve_target: color.as_ref().map(|_| &**resolve),
            ops: Operations {
                load: LoadOp::Clear(clear),
                store: if color.is_some() {
                    StoreOp::Discard
                } else {
                    StoreOp::Store
                },
            },
        },
        depth.as_ref().map(|view| RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(0.),
                store: if keep_depth {
                    StoreOp::Store
                } else {
                    StoreOp::Discard
                },
            }),
            stencil_ops: None,
        }),
    )
}

/// Record the draws of consecutive views, changing state only where it differs.
fn encode(
    render_pass: &mut wgpu::RenderPass<'_>,
    state: &FrameState,
    views: Range<u32>,
    frame_group: &BindGroup,
    cache: &PipelineCache,
    immediate: bool,
    phase: Phase,
) {
    let FrameState {
        frame,
        resolved,
        groups,
        ..
    } = state;
    let mut pipeline = None;
    let mut material = u32::MAX;
    let mut coverage = u32::MAX;
    let mut vertices = None;
    let mut indices = None;
    let mut slot = None;
    for view_index in views {
        let view = &frame.views[view_index as usize];
        let offset = (view_index as u64 * 2 * SLOT) as u32;
        render_pass.set_bind_group(0, frame_group, &[offset, offset + SLOT as u32]);
        let mut draw_indices = view
            .draws
            .clone()
            .filter(|&index| {
                frame.materials[frame.draws[index as usize].material as usize].phase() == phase
            })
            .peekable();
        while let Some(index) = draw_indices.next() {
            let draw = &frame.draws[index as usize];
            let (Some(item), Some(group)) =
                (&resolved[index as usize], &groups[draw.material as usize])
            else {
                continue;
            };
            let Some(ready) = cache.get_render_pipeline(item.pipeline) else {
                continue;
            };
            if pipeline != Some(item.pipeline) {
                render_pass.set_pipeline(ready);
                pipeline = Some(item.pipeline);
                // Immediates need not survive a layout without them.
                slot = None;
            }
            // The same record row the vertex shader reads for this instance.
            if immediate
                && matches!(
                    frame.materials[draw.material as usize],
                    Material::Surface { .. }
                )
            {
                let row = frame.records[index as usize].ids[0];
                if slot != Some(row) {
                    render_pass.set_immediates(0, bytemuck::bytes_of(&row));
                    slot = Some(row);
                }
            }
            let ui = matches!(frame.materials[draw.material as usize], Material::Ui { .. });
            if material != draw.material || (ui && coverage != draw.coverage) {
                if ui {
                    let offset = (u64::from(draw.coverage) * COVERAGE_SLOT) as u32;
                    render_pass.set_bind_group(1, group, &[offset]);
                } else {
                    render_pass.set_bind_group(1, group, &[]);
                }
                material = draw.material;
                coverage = draw.coverage;
            }
            if vertices != Some(item.vertices) {
                render_pass.set_vertex_buffer(0, *item.vertex_buffer.slice(..));
                vertices = Some(item.vertices);
            }
            let instances = index..index + 1;
            match &item.indices {
                Some((buffer, format, range)) => {
                    if indices != Some(buffer.id()) {
                        render_pass.set_index_buffer(*buffer.slice(..), *format);
                        indices = Some(buffer.id());
                    }
                    render_pass.draw_indexed(range.clone(), item.base_vertex, instances);
                }
                None => {
                    let mut end = item.vertex_range.end;
                    // Streamed triangles with the same effective draw record can
                    // share an invocation, including differently displaced scene
                    // particles whose offsets live in their vertices.
                    if draw.geometry.is_some()
                        && item.topology == PrimitiveTopology::TriangleList
                        && item.vertex_range.len().is_multiple_of(3)
                    {
                        while let Some(&next) = draw_indices.peek() {
                            let next_draw = &frame.draws[next as usize];
                            let Some(next_item) = &resolved[next as usize] else {
                                break;
                            };
                            if next_draw.geometry.is_none()
                                || next_draw.material != draw.material
                                || next_draw.coverage != draw.coverage
                                || next_item.pipeline != item.pipeline
                                || next_item.topology != PrimitiveTopology::TriangleList
                                || next_item.vertices != item.vertices
                                || next_item.indices.is_some()
                                || !next_item.vertex_range.len().is_multiple_of(3)
                                || next_item.vertex_range.start != end
                                || frame.records[next as usize] != frame.records[index as usize]
                            {
                                break;
                            }
                            end = next_item.vertex_range.end;
                            draw_indices.next();
                        }
                    }
                    render_pass.draw(item.vertex_range.start..end, instances);
                }
            }
        }
    }
}

/// The pipeline a draw of `prepare` with this mesh uses in a pass.
fn pipeline_key(
    prepare: super::Prepare,
    mesh: Option<&RenderMesh>,
    skinned: bool,
    streamed: bool,
    fog: bool,
    target: TargetKey,
) -> PipelineKey {
    use super::Prepare;

    let material = match prepare {
        Prepare::Surface(key) => MaterialKey::Surface {
            key,
            skinned: skinned
                && !streamed
                && mesh.is_some_and(|mesh| {
                    mesh.layout.0.contains(Mesh::ATTRIBUTE_JOINT_INDEX)
                        && mesh.layout.0.contains(Mesh::ATTRIBUTE_JOINT_WEIGHT)
                }),
            streamed,
            fog,
        },
        Prepare::Ui(key) => MaterialKey::Ui(key),
        Prepare::Output => MaterialKey::Output,
        Prepare::Text => MaterialKey::Text,
        Prepare::Backdrop => MaterialKey::Backdrop,
        Prepare::Composite => MaterialKey::Composite,
        Prepare::Glow => MaterialKey::Glow,
        Prepare::Water => MaterialKey::Water,
        Prepare::Refraction => MaterialKey::Refraction { ripple: false },
        Prepare::SceneSample { masked, depth_test } => {
            MaterialKey::SceneSample { masked, depth_test }
        }
    };
    let sampled = matches!(material, MaterialKey::SceneSample { .. });
    PipelineKey {
        mesh: if matches!(material, MaterialKey::Ui(_)) || (sampled && streamed) {
            None
        } else {
            mesh.map(|mesh| MeshKey {
                layout: mesh.layout.clone(),
                topology: mesh.primitive_topology(),
                strip_index_format: match mesh.primitive_topology() {
                    PrimitiveTopology::LineStrip | PrimitiveTopology::TriangleStrip => {
                        mesh.index_format()
                    }
                    _ => None,
                },
            })
        },
        target: TargetKey {
            samples: if sampled {
                if target.samples > 1 { 2 } else { 1 }
            } else {
                target.samples
            },
            depth: target.depth && !sampled,
            ..target
        },
        material,
    }
}

/// Build, ahead of time, the pipeline a preparation's draw will use in the
/// scene pass; `None` until its mesh is uploaded.
/// Skinned layouts are prepared skinned, as their draws are; without an
/// explicit sample count the current scene pass's is used.
pub(crate) fn prepare_pipeline(
    world: &mut World,
    request: &crate::gpu_prepare::Request,
) -> Option<CachedRenderPipelineId> {
    let format = crate::renderer::SCENE_COLOR_FORMAT;
    world.resource_scope(|world, mut renderer: Mut<Renderer>| {
        let meshes = world.resource::<RenderAssets<RenderMesh>>();
        let mesh = match request.mesh {
            Some(id) => Some(meshes.get(id)?),
            None => None,
        };
        let frame = &world.resource::<FrameState>().frame;
        let scene = frame
            .passes
            .iter()
            .find(|pass| pass.depth && !pass.offscreen);
        let samples = request
            .samples
            .or_else(|| scene.map(|pass| pass.samples))
            .unwrap_or(1);
        let fog = request.view == crate::gpu_prepare::View::Field;
        let key = pipeline_key(
            request.material,
            mesh,
            true,
            request.streamed,
            fog,
            TargetKey {
                samples,
                format,
                depth: true,
            },
        );
        renderer.pipeline(
            key,
            world.resource::<Shaders>(),
            world.resource::<PipelineCache>(),
        )
    })
}

fn resolve(
    renderer: &mut Renderer,
    draw: &Draw,
    material: &Material,
    fog: bool,
    target: TargetKey,
    streams: &[Buffer; 2],
    resources: &Resources,
) -> Option<Resolved> {
    let Resources {
        meshes,
        allocator,
        shaders,
        cache,
        ..
    } = resources;
    let mesh = match draw.mesh {
        Some(id) => Some(meshes.get(id)?),
        None => None,
    };
    let streamed = draw.geometry.is_some();
    let key = pipeline_key(
        material.prepare(),
        mesh,
        draw.skinned,
        streamed,
        fog,
        target,
    );
    let pipeline = renderer.pipeline(key, shaders, cache)?;
    if let Some(range) = &draw.geometry {
        let stream = &streams[usize::from(draw.mesh.is_none())];
        return Some(Resolved {
            topology: PrimitiveTopology::TriangleList,
            pipeline,
            vertices: stream.id(),
            vertex_buffer: stream.clone(),
            indices: None,
            vertex_range: range.clone(),
            base_vertex: 0,
        });
    }
    let mesh = mesh?;
    let mesh_id = draw.mesh?;
    let slice = allocator.mesh_vertex_slice(&mesh_id)?;
    let vertex_buffer = slice.buffer.clone();
    let first_vertex = slice.range.start;
    let indices = match mesh.buffer_info {
        RenderMeshBufferInfo::Indexed {
            count,
            index_format,
        } => {
            let slice = allocator.mesh_index_slice(&mesh_id)?;
            Some((
                slice.buffer.clone(),
                index_format,
                slice.range.start..slice.range.start + count,
            ))
        }
        RenderMeshBufferInfo::NonIndexed => None,
    };
    Some(Resolved {
        topology: mesh.primitive_topology(),
        pipeline,
        vertices: vertex_buffer.id(),
        vertex_buffer,
        indices,
        vertex_range: first_vertex..first_vertex + mesh.vertex_count,
        base_vertex: first_vertex as i32,
    })
}
