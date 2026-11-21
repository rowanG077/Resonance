//! One spatial SMAA reconstruction path for all quality presets and S2x.
//! S2x additionally preserves and reconstructs the two native MSAA samples.
//! Edge detection uses encoded color; reconstruction and averaging use linear light.
use super::AntiAliasing;
use bevy::{
    asset::{RenderAssetUsages, embedded_asset, load_embedded_asset},
    core_pipeline::{
        FullscreenShader,
        blit::{BlitPipeline, BlitPipelineKey},
    },
    image::{ImageLoaderSettings, ToExtents},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{sampler, texture_2d, texture_2d_multisampled, uniform_buffer},
            *,
        },
        renderer::{RenderDevice, RenderQueue},
        texture::GpuImage,
    },
};
use std::collections::HashMap;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Smaa(pub AntiAliasing);
impl Smaa {
    pub(super) fn new(mode: AntiAliasing) -> Option<Self> {
        matches!(
            mode,
            AntiAliasing::SmaaLow
                | AntiAliasing::SmaaMedium
                | AntiAliasing::SmaaHigh
                | AntiAliasing::SmaaUltra
                | AntiAliasing::SmaaS2x
        )
        .then_some(Self(mode))
    }
    fn s2x(self) -> bool {
        self.0 == AntiAliasing::SmaaS2x
    }
    fn preset(self) -> &'static str {
        match self.0 {
            AntiAliasing::SmaaLow => "SMAA_PRESET_LOW",
            AntiAliasing::SmaaMedium => "SMAA_PRESET_MEDIUM",
            AntiAliasing::SmaaHigh => "SMAA_PRESET_HIGH",
            _ => "SMAA_PRESET_ULTRA",
        }
    }
}
pub(super) fn install(app: &mut App) {
    embedded_asset!(app, "smaa.wesl");
    embedded_asset!(app, "SMAAAreaLUT.ktx2");
    embedded_asset!(app, "SMAASearchLUT.ktx2");
    embedded_asset!(app, "spatial_io.wesl");
    let server = app.world().resource::<AssetServer>();
    let load_lut = |name: &str| {
        server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.is_srgb = false;
                settings.asset_usage = RenderAssetUsages::RENDER_WORLD;
            })
            .load(format!(
                "embedded://resonance_presentation/anti_aliasing/{name}.ktx2"
            ))
    };
    let luts = Luts {
        area: load_lut("SMAAAreaLUT"),
        search: load_lut("SMAASearchLUT"),
    };
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .insert_resource(luts)
            .add_systems(RenderStartup, initialize);
    }
}
#[derive(Resource)]
struct Luts {
    area: Handle<Image>,
    search: Handle<Image>,
}
#[derive(Clone, Copy, ShaderType)]
struct Info {
    rt_metrics: Vec4,
    sample: u32,
}
#[derive(Clone, Copy)]
struct Pipelines {
    edge: CachedRenderPipelineId,
    weight: CachedRenderPipelineId,
    neighborhood: [CachedRenderPipelineId; 2],
    raster: Option<RasterPipelines>,
}
#[derive(Clone, Copy)]
struct RasterPipelines {
    split: CachedRenderPipelineId,
    calibrate: CachedRenderPipelineId,
    copy: CachedRenderPipelineId,
}
impl Pipelines {
    fn ready(&self, cache: &PipelineCache) -> bool {
        [
            self.edge,
            self.weight,
            self.neighborhood[0],
            self.neighborhood[1],
        ]
        .into_iter()
        .chain(
            self.raster
                .into_iter()
                .flat_map(|p| [p.split, p.calibrate, p.copy]),
        )
        .all(|id| cache.get_render_pipeline(id).is_some())
    }
}
struct Layouts {
    common: [BindGroupLayoutDescriptor; 2],
    edge: BindGroupLayoutDescriptor,
    weight: BindGroupLayoutDescriptor,
    neighborhood: BindGroupLayoutDescriptor,
    split: BindGroupLayoutDescriptor,
}
#[derive(Resource)]
struct State {
    shader: Handle<Shader>,
    io: Handle<Shader>,
    fullscreen: FullscreenShader,
    layouts: Layouts,
    sampler: Sampler,
    pipelines: HashMap<(TextureFormat, AntiAliasing), Pipelines>,
}
struct Surface {
    texture: Texture,
    default_view: TextureView,
}
fn color_view(texture: &Texture, format: TextureFormat) -> TextureView {
    texture.create_view(&TextureViewDescriptor {
        format: Some(format),
        ..default()
    })
}
struct Inputs {
    edges: BindGroup,
    neighborhood: BindGroup,
}
struct SpatialSamples {
    calibrated: std::sync::atomic::AtomicBool,
    msaa: Surface,
    calibration: Surface,
    colors: [Surface; 2],
    split: BindGroup,
}
fn initialize(
    mut commands: Commands,
    server: Res<AssetServer>,
    device: Res<RenderDevice>,
    fullscreen: Res<FullscreenShader>,
) {
    let layouts = Layouts {
        common: [false, true].map(|s2x| {
            let mut entries = vec![
                texture_2d(TextureSampleType::Float { filterable: true })
                    .build(0, ShaderStages::FRAGMENT),
                uniform_buffer::<Info>(false).build(1, ShaderStages::VERTEX_FRAGMENT),
            ];
            if s2x {
                entries.push(
                    texture_2d_multisampled(TextureSampleType::Float { filterable: false })
                        .build(2, ShaderStages::FRAGMENT),
                );
            }
            BindGroupLayoutDescriptor::new("SMAA common", &entries)
        }),
        edge: BindGroupLayoutDescriptor::new(
            "SMAA edge",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (sampler(SamplerBindingType::Filtering),),
            ),
        ),
        weight: BindGroupLayoutDescriptor::new(
            "SMAA weights",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                ),
            ),
        ),
        neighborhood: BindGroupLayoutDescriptor::new(
            "SMAA neighborhood",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        ),
        split: BindGroupLayoutDescriptor::new(
            "S2x split",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (texture_2d_multisampled(TextureSampleType::Float {
                    filterable: false,
                }),),
            ),
        ),
    };
    commands.insert_resource(State {
        shader: load_embedded_asset!(server.as_ref(), "smaa.wesl"),
        io: load_embedded_asset!(server.as_ref(), "spatial_io.wesl"),
        fullscreen: fullscreen.clone(),
        layouts,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        pipelines: HashMap::new(),
    });
}
fn queue_pipelines(
    state: &State,
    cache: &PipelineCache,
    format: TextureFormat,
    mode: Smaa,
    blit: &BlitPipeline,
    specialized: &mut SpecializedRenderPipelines<BlitPipeline>,
) -> Pipelines {
    let layouts = &state.layouts;
    let stages = [
        (
            "SMAA_EDGE_DETECTION",
            "edge_detection_vertex_main",
            "luma_edge_detection_fragment_main",
            TextureFormat::Rg8Unorm,
            &layouts.edge,
        ),
        (
            "SMAA_BLENDING_WEIGHT_CALCULATION",
            "blending_weight_calculation_vertex_main",
            "blending_weight_calculation_fragment_main",
            TextureFormat::Rgba8Unorm,
            &layouts.weight,
        ),
        (
            "SMAA_NEIGHBORHOOD_BLENDING",
            "neighborhood_blending_vertex_main",
            "neighborhood_blending_fragment_main",
            format.add_srgb_suffix(),
            &layouts.neighborhood,
        ),
    ];
    let mut ids = Vec::new();
    for phase in [0, 1, 2, 2]
        .into_iter()
        .take(if mode.s2x() { 4 } else { 3 })
    {
        let (def, vertex, fragment, target_format, layout) = stages[phase];
        let mut defs = vec![def.into(), mode.preset().into()];
        if mode.s2x() {
            defs.push("SMAA_S2X".into());
        }
        if format.is_srgb() {
            defs.push("SMAA_INPUT_SRGB".into());
        }
        let stencil_face = StencilFaceState {
            compare: if phase == 0 {
                CompareFunction::Always
            } else {
                CompareFunction::Equal
            },
            pass_op: if phase == 0 {
                StencilOperation::Replace
            } else {
                StencilOperation::Keep
            },
            ..default()
        };
        // The second spatial result contributes exactly half, independent of
        // material transparency. Ordinary alpha blending is incorrect here.
        let blend = if ids.len() == 3 {
            let component = BlendComponent {
                src_factor: BlendFactor::Constant,
                dst_factor: BlendFactor::OneMinusConstant,
                operation: BlendOperation::Add,
            };
            Some(BlendState {
                color: component,
                alpha: component,
            })
        } else {
            None
        };
        ids.push(cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(format!("SMAA {:?} {def}", mode.0).into()),
            layout: vec![
                layouts.common[usize::from(mode.s2x())].clone(),
                layout.clone(),
            ],
            vertex: VertexState {
                constants: default(),
                shader: state.shader.clone(),
                shader_defs: defs.clone(),
                entry_point: Some(vertex.into()),
                buffers: vec![],
            },
            fragment: Some(FragmentState {
                constants: default(),
                shader: state.shader.clone(),
                shader_defs: defs,
                entry_point: Some(fragment.into()),
                targets: vec![Some(ColorTargetState {
                    format: target_format,
                    blend,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            depth_stencil: (phase < 2).then_some(DepthStencilState {
                format: TextureFormat::Stencil8,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::Always),
                stencil: StencilState {
                    front: stencil_face,
                    back: stencil_face,
                    read_mask: 1,
                    write_mask: if phase == 0 { 1 } else { 0 },
                },
                bias: default(),
            }),
            ..default()
        }));
    }
    let target = Some(ColorTargetState {
        format,
        blend: None,
        write_mask: ColorWrites::ALL,
    });
    Pipelines {
        edge: ids[0],
        weight: ids[1],
        neighborhood: [ids[2], *ids.get(3).unwrap_or(&ids[2])],
        raster: mode.s2x().then(|| RasterPipelines {
            split: {
                cache.queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("S2x separate spatial samples".into()),
                    layout: vec![layouts.split.clone()],
                    vertex: state.fullscreen.to_vertex_state(),
                    fragment: Some(FragmentState {
                        constants: default(),
                        shader: state.io.clone(),
                        shader_defs: vec!["SPLIT_SAMPLES".into()],
                        entry_point: Some("split_samples".into()),
                        targets: vec![target.clone(), target.clone()],
                    }),
                    ..default()
                })
            },
            calibrate: {
                cache.queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("S2x sample position calibration".into()),
                    vertex: VertexState {
                        constants: default(),
                        shader: state.io.clone(),
                        entry_point: Some("calibration_vertex".into()),
                        ..default()
                    },
                    fragment: Some(FragmentState {
                        constants: default(),
                        shader: state.io.clone(),
                        entry_point: Some("calibration_fragment".into()),
                        targets: vec![target],
                        ..default()
                    }),
                    multisample: MultisampleState {
                        count: 2,
                        ..default()
                    },
                    ..default()
                })
            },
            copy: {
                specialized.specialize(
                    cache,
                    blit,
                    BlitPipelineKey {
                        target_format: format,
                        blend_state: None,
                        samples: 2,
                        source_space: None,
                    },
                )
            },
        }),
    }
}
fn attachment(view: &TextureView, load: LoadOp<LinearRgba>) -> RenderPassColorAttachment<'_> {
    RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: match load {
                LoadOp::Clear(c) => LoadOp::Clear(c.into()),
                LoadOp::Load => LoadOp::Load,
                LoadOp::DontCare(token) => LoadOp::DontCare(token),
            },
            store: StoreOp::Store,
        },
    }
}
struct Passes<'a> {
    edges: &'a TextureView,
    weights: &'a TextureView,
    stencil: &'a TextureView,
    edge: &'a BindGroup,
    weight: &'a BindGroup,
    neighborhood: &'a BindGroup,
}

/// Edge detection, blending weights and neighborhood blending of each input
/// into `destination`, an sRGB view; a second (S2x) input adds half.
fn passes(
    encoder: &mut CommandEncoder,
    resources: &Passes,
    pipelines: &Pipelines,
    cache: &PipelineCache,
    destination: &TextureView,
    inputs: &[&Inputs],
) {
    for (sample, input) in inputs.iter().enumerate() {
        for phase in 0..3 {
            let (destination, pipeline, group) = match phase {
                0 => (resources.edges, pipelines.edge, resources.edge),
                1 => (resources.weights, pipelines.weight, resources.weight),
                _ => (
                    destination,
                    pipelines.neighborhood[sample],
                    resources.neighborhood,
                ),
            };
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("SMAA reconstruction"),
                color_attachments: &[Some(attachment(
                    destination,
                    if phase == 2 && sample == 1 {
                        LoadOp::Load
                    } else {
                        LoadOp::Clear(LinearRgba::NONE)
                    },
                ))],
                depth_stencil_attachment: (phase < 2).then_some(RenderPassDepthStencilAttachment {
                    view: resources.stencil,
                    depth_ops: None,
                    stencil_ops: Some(Operations {
                        load: if phase == 0 {
                            LoadOp::Clear(0)
                        } else {
                            LoadOp::Load
                        },
                        store: StoreOp::Store,
                    }),
                }),
                ..default()
            });
            pass.set_pipeline(cache.get_render_pipeline(pipeline).unwrap());
            let input = if phase == 2 {
                &input.neighborhood
            } else {
                &input.edges
            };
            pass.set_bind_group(0, input, &[]);
            pass.set_bind_group(1, group, &[]);
            if phase < 2 {
                pass.set_stencil_reference(1);
            }
            if phase == 2 && sample == 1 {
                pass.set_blend_constant(wgpu::Color {
                    r: 0.5,
                    g: 0.5,
                    b: 0.5,
                    a: 0.5,
                });
            }
            pass.draw(0..3, 0..1);
        }
    }
}

/// A single-mip 2D texture SMAA renders to or samples. An image of the scene
/// format can also be viewed as sRGB, where reconstruction blends.
fn surface(
    device: &RenderDevice,
    label: &'static str,
    size: UVec2,
    format: TextureFormat,
    samples: u32,
    srgb_view: bool,
) -> Surface {
    let alternate_format = format.add_srgb_suffix();
    let texture = device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: size.to_extents(),
        mip_level_count: 1,
        sample_count: samples,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT
            | if format == TextureFormat::Stencil8 {
                TextureUsages::empty()
            } else {
                TextureUsages::TEXTURE_BINDING
            },
        view_formats: if srgb_view && alternate_format != format {
            std::slice::from_ref(&alternate_format)
        } else {
            &[]
        },
    });
    let default_view = texture.create_view(&TextureViewDescriptor::default());
    Surface {
        texture,
        default_view,
    }
}

/// An input image's bind groups. Native views feed luma thresholds (the
/// shader re-encodes sRGB reads); sRGB views provide linear interpolation and
/// averaging.
fn inputs(
    device: &RenderDevice,
    cache: &PipelineCache,
    layout: &BindGroupLayoutDescriptor,
    source: &Texture,
    uniform: &UniformBuffer<Info>,
    calibration: Option<&TextureView>,
) -> Inputs {
    let format = source.format();
    let bind = |view_format| {
        let color = color_view(source, view_format);
        let mut entries = vec![
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&color),
            },
            BindGroupEntry {
                binding: 1,
                resource: uniform.binding().unwrap(),
            },
        ];
        if let Some(calibration) = calibration {
            entries.push(BindGroupEntry {
                binding: 2,
                resource: BindingResource::TextureView(calibration),
            });
        }
        device.create_bind_group("SMAA input", &cache.get_bind_group_layout(layout), &entries)
    };
    Inputs {
        edges: bind(format),
        neighborhood: bind(format.add_srgb_suffix()),
    }
}

/// The edge, weight and neighborhood passes' own bind groups.
fn pass_groups(
    device: &RenderDevice,
    cache: &PipelineCache,
    state: &State,
    area: &GpuImage,
    search: &GpuImage,
    edges: &Surface,
    weights: &Surface,
) -> [BindGroup; 3] {
    let layouts = &state.layouts;
    [
        device.create_bind_group(
            "SMAA edge",
            &cache.get_bind_group_layout(&layouts.edge),
            &BindGroupEntries::sequential((&state.sampler,)),
        ),
        device.create_bind_group(
            "SMAA weights",
            &cache.get_bind_group_layout(&layouts.weight),
            &BindGroupEntries::sequential((
                &edges.default_view,
                &state.sampler,
                &search.texture_view,
                &area.texture_view,
            )),
        ),
        device.create_bind_group(
            "SMAA neighborhood",
            &cache.get_bind_group_layout(&layouts.neighborhood),
            &BindGroupEntries::sequential((&weights.default_view, &state.sampler)),
        ),
    ]
}

/// What the native frame needs to run SMAA: its pipelines, layouts and lookup
/// tables.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct NativeContext<'w> {
    state: Option<ResMut<'w, State>>,
    luts: Option<Res<'w, Luts>>,
    blit: Option<Res<'w, BlitPipeline>>,
    specialized: Option<ResMut<'w, SpecializedRenderPipelines<BlitPipeline>>>,
}

/// SMAA for the native frame renderer, on textures it owns: one set per view
/// size, since the model preview and reflection images need not match the scene's.
#[derive(Default)]
pub(crate) struct Native(Vec<NativeView>);
struct NativeView {
    size: UVec2,
    format: TextureFormat,
    mode: Smaa,
    pipelines: Pipelines,
    edges: Surface,
    stencil: Surface,
    weights: Surface,
    uniform: UniformBuffer<Info>,
    groups: [BindGroup; 3],
    /// S2x: the readable two-sample scene image, its separated samples and
    /// their input bind groups.
    spatial: Option<(SpatialSamples, [Inputs; 2])>,
    /// Input bind groups and sRGB destination views of the few textures the
    /// native frame alternates between.
    inputs: Vec<(TextureId, Inputs)>,
    outputs: Vec<(TextureId, TextureView)>,
}

impl NativeView {
    fn output(&mut self, destination: &Texture) -> &TextureView {
        if !self.outputs.iter().any(|(id, _)| *id == destination.id()) {
            if self.outputs.len() >= 6 {
                self.outputs.remove(0);
            }
            self.outputs.push((
                destination.id(),
                color_view(destination, self.format.add_srgb_suffix()),
            ));
        }
        &self
            .outputs
            .iter()
            .find(|(id, _)| *id == destination.id())
            .unwrap()
            .1
    }
}

fn info(size: UVec2, sample: u32) -> Info {
    Info {
        rt_metrics: Vec4::new(
            1. / size.x as f32,
            1. / size.y as f32,
            size.x as f32,
            size.y as f32,
        ),
        sample,
    }
}

impl Native {
    /// Whether SMAA is set up to draw `mode` at `size`, S2x calibrated; modes
    /// without SMAA always are.
    #[cfg(test)]
    pub(crate) fn ready(&self, mode: AntiAliasing, size: UVec2, cache: &PipelineCache) -> bool {
        let Some(mode) = Smaa::new(mode) else {
            return true;
        };
        self.0.iter().any(|view| {
            view.size == size
                && view.mode == mode
                && view.pipelines.ready(cache)
                && view.spatial.as_ref().is_none_or(|(spatial, _)| {
                    spatial
                        .calibrated
                        .load(std::sync::atomic::Ordering::Relaxed)
                })
        })
    }

    /// This size's resources for `mode`, once its pipelines and lookup tables
    /// are ready. Until then the caller draws without SMAA (S2x as plain
    /// MSAA).
    #[allow(clippy::too_many_arguments)]
    fn view(
        &mut self,
        context: &mut NativeContext,
        mode: Smaa,
        images: &RenderAssets<GpuImage>,
        cache: &PipelineCache,
        device: &RenderDevice,
        queue: &RenderQueue,
        size: UVec2,
        format: TextureFormat,
    ) -> Option<usize> {
        let NativeContext {
            state: Some(state),
            luts: Some(luts),
            blit: Some(blit),
            specialized: Some(specialized),
        } = context
        else {
            return None;
        };
        let (area, search) = (images.get(&luts.area)?, images.get(&luts.search)?);
        let pipelines = match state.pipelines.get(&(format, mode.0)) {
            Some(pipelines) => *pipelines,
            None => {
                let pipelines = queue_pipelines(state, cache, format, mode, blit, specialized);
                state.pipelines.insert((format, mode.0), pipelines);
                pipelines
            }
        };
        if !pipelines.ready(cache) {
            return None;
        }
        if let Some(index) = self
            .0
            .iter()
            .position(|view| view.size == size && view.format == format && view.mode == mode)
        {
            return Some(index);
        }
        // A resized scene, the preview's startup size and the field reflection
        // can all draw in one frame. Retain all three to avoid cache thrashing.
        if self.0.len() >= 3 {
            self.0.remove(0);
        }
        let edges = surface(
            device,
            "SMAA edges",
            size,
            TextureFormat::Rg8Unorm,
            1,
            false,
        );
        let stencil = surface(
            device,
            "SMAA stencil",
            size,
            TextureFormat::Stencil8,
            1,
            false,
        );
        let weights = surface(
            device,
            "SMAA weights",
            size,
            TextureFormat::Rgba8Unorm,
            1,
            false,
        );
        let mut uniform = UniformBuffer::from(info(size, 0));
        uniform.write_buffer(device, queue);
        let groups = pass_groups(device, cache, state, area, search, &edges, &weights);
        let spatial = mode.s2x().then(|| {
            let msaa = surface(device, "S2x readable MSAA", size, format, 2, true);
            let calibration = surface(
                device,
                "S2x measured positions",
                UVec2::ONE,
                format,
                2,
                true,
            );
            let colors = [
                surface(device, "S2x sample 0", size, format, 1, true),
                surface(device, "S2x sample 1", size, format, 1, true),
            ];
            let split = device.create_bind_group(
                "S2x split",
                &cache.get_bind_group_layout(&state.layouts.split),
                &BindGroupEntries::sequential((&msaa.default_view,)),
            );
            let inputs = std::array::from_fn(|sample| {
                let mut uniform = UniformBuffer::from(info(size, sample as u32));
                uniform.write_buffer(device, queue);
                inputs(
                    device,
                    cache,
                    &state.layouts.common[1],
                    &colors[sample].texture,
                    &uniform,
                    Some(&calibration.default_view),
                )
            });
            (
                SpatialSamples {
                    calibrated: std::sync::atomic::AtomicBool::new(false),
                    msaa,
                    calibration,
                    colors,
                    split,
                },
                inputs,
            )
        });
        self.0.push(NativeView {
            size,
            format,
            mode,
            pipelines,
            edges,
            stencil,
            weights,
            uniform,
            groups,
            spatial,
            inputs: Vec::new(),
            outputs: Vec::new(),
        });
        Some(self.0.len() - 1)
    }

    /// Reconstruct `input` into `destination` with a regular preset, as
    /// Bevy's SMAA pass does at the end of a camera. False, with nothing
    /// recorded, while the pipelines or lookup tables are not ready.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode(
        &mut self,
        context: &mut NativeContext,
        mode: AntiAliasing,
        images: &RenderAssets<GpuImage>,
        cache: &PipelineCache,
        device: &RenderDevice,
        queue: &RenderQueue,
        encoder: &mut CommandEncoder,
        input: &Texture,
        destination: &Texture,
    ) -> bool {
        let Some(mode) = Smaa::new(mode).filter(|mode| !mode.s2x()) else {
            return false;
        };
        let size = UVec2::new(input.width(), input.height());
        let Some(index) = self.view(
            context,
            mode,
            images,
            cache,
            device,
            queue,
            size,
            input.format(),
        ) else {
            return false;
        };
        let layout = &context.state.as_ref().unwrap().layouts.common[0];
        let view = &mut self.0[index];
        if !view.inputs.iter().any(|(id, _)| *id == input.id()) {
            if view.inputs.len() >= 6 {
                view.inputs.remove(0);
            }
            let group = inputs(device, cache, layout, input, &view.uniform, None);
            view.inputs.push((input.id(), group));
        }
        view.output(destination);
        let view = &self.0[index];
        let input = &view
            .inputs
            .iter()
            .find(|(id, _)| *id == input.id())
            .unwrap()
            .1;
        let destination = &view
            .outputs
            .iter()
            .find(|(id, _)| *id == destination.id())
            .unwrap()
            .1;
        passes(
            encoder,
            &view.passes(),
            &view.pipelines,
            cache,
            destination,
            &[input],
        );
        true
    }

    /// S2x: the readable two-sample image the scene part draws into instead
    /// of a resolving one, or nothing while S2x is not ready; Bevy then draws
    /// plain MSAA as well.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn s2x_target(
        &mut self,
        context: &mut NativeContext,
        images: &RenderAssets<GpuImage>,
        cache: &PipelineCache,
        device: &RenderDevice,
        queue: &RenderQueue,
        size: UVec2,
        format: TextureFormat,
    ) -> Option<TextureView> {
        let mode = Smaa::new(AntiAliasing::SmaaS2x)?;
        let index = self.view(context, mode, images, cache, device, queue, size, format)?;
        let (spatial, _) = self.0[index].spatial.as_ref()?;
        Some(spatial.msaa.default_view.clone())
    }

    /// S2x, after the scene part drew into [`Self::s2x_target`]: measure the
    /// sample order once, separate both samples and reconstruct each into
    /// `destination`, the second blended at half, as `render_s2x` does.
    pub(crate) fn s2x_reconstruct(
        &mut self,
        cache: &PipelineCache,
        encoder: &mut CommandEncoder,
        destination: &Texture,
    ) {
        let size = UVec2::new(destination.width(), destination.height());
        let Some(view) = self.0.iter_mut().find(|view| {
            view.size == size && view.format == destination.format() && view.mode.s2x()
        }) else {
            return;
        };
        view.output(destination);
        let view = &*view;
        let (spatial, inputs) = view.spatial.as_ref().unwrap();
        let raster = view.pipelines.raster.unwrap();
        if !spatial
            .calibrated
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("S2x calibrate sample order"),
                color_attachments: &[Some(attachment(
                    &spatial.calibration.default_view,
                    LoadOp::Clear(LinearRgba::BLACK),
                ))],
                ..default()
            });
            pass.set_pipeline(cache.get_render_pipeline(raster.calibrate).unwrap());
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("S2x separate samples"),
                color_attachments: &spatial.colors.each_ref().map(|t| {
                    Some(attachment(
                        &t.default_view,
                        LoadOp::Clear(LinearRgba::BLACK),
                    ))
                }),
                ..default()
            });
            pass.set_pipeline(cache.get_render_pipeline(raster.split).unwrap());
            pass.set_bind_group(0, &spatial.split, &[]);
            pass.draw(0..3, 0..1);
        }
        let destination = &view
            .outputs
            .iter()
            .find(|(id, _)| *id == destination.id())
            .unwrap()
            .1;
        passes(
            encoder,
            &view.passes(),
            &view.pipelines,
            cache,
            destination,
            &[&inputs[0], &inputs[1]],
        );
    }
}

impl NativeView {
    fn passes(&self) -> Passes<'_> {
        Passes {
            edges: &self.edges.default_view,
            weights: &self.weights.default_view,
            stencil: &self.stencil.default_view,
            edge: &self.groups[0],
            weight: &self.groups[1],
            neighborhood: &self.groups[2],
        }
    }
}

#[cfg(test)]
mod tests;
