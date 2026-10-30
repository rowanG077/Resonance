//! One spatial SMAA reconstruction path for all quality presets and S2x.
//! S2x additionally preserves and reconstructs the two native MSAA samples.
//! Edge detection uses encoded color; reconstruction and averaging use linear light.
use super::AntiAliasing;
use bevy::{
    asset::{RenderAssetUsages, embedded_asset, load_embedded_asset},
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::{
        FullscreenShader,
        blit::{BlitPipeline, BlitPipelineKey},
        core_3d::{AlphaMask3d, Opaque3d, Transparent3d},
        schedule::{Core3d, Core3dSystems},
    },
    image::{ImageLoaderSettings, ToExtents},
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        render_phase::{ViewBinnedRenderPhases, ViewSortedRenderPhases},
        render_resource::{
            binding_types::{sampler, texture_2d, texture_2d_multisampled, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        texture::GpuImage,
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};
use std::collections::HashMap;

#[derive(Component, Clone, Copy, PartialEq, Eq, ExtractComponent)]
#[extract_component_sync_target((Self, ViewResources))]
pub(super) struct Smaa(pub AntiAliasing);
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
    embedded_asset!(app, "smaa.wgsl");
    embedded_asset!(app, "SMAAAreaLUT.ktx2");
    embedded_asset!(app, "SMAASearchLUT.ktx2");
    embedded_asset!(app, "spatial_io.wgsl");
    app.add_plugins(ExtractComponentPlugin::<Smaa>::default());
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
            .add_systems(RenderStartup, initialize)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups))
            // S2x needs a readable MSAA attachment. Bevy's default attachment
            // has RENDER_ATTACHMENT usage only. Reuse its queued draw phases
            // and depth, replacing just the main color attachment for this mode.
            .configure_sets(Core3d, Core3dSystems::MainPass.run_if(regular_main_pass))
            .add_systems(
                Core3d,
                render_regular
                    .in_set(Core3dSystems::PostProcess)
                    .after(bevy::core_pipeline::tonemapping::tonemapping)
                    .after(crate::field_dissolve::Pass),
            )
            .add_systems(
                Core3d,
                render_s2x
                    .after(Core3dSystems::MainPass)
                    .before(Core3dSystems::EarlyPostProcess),
            );
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
#[derive(Component)]
pub(super) struct ViewResources {
    size: UVec2,
    format: TextureFormat,
    mode: Smaa,
    spatial: Option<SpatialSamples>,
    edges: Surface,
    stencil: Surface,
    weights: Surface,
    common: [Inputs; 2],
    srgb_outputs: [(TextureId, TextureView); 2],
    edge: BindGroup,
    weight: BindGroup,
    neighborhood: BindGroup,
    pipelines: Pipelines,
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
        shader: load_embedded_asset!(server.as_ref(), "smaa.wgsl"),
        io: load_embedded_asset!(server.as_ref(), "spatial_io.wgsl"),
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
                shader: state.shader.clone(),
                shader_defs: defs.clone(),
                entry_point: Some(vertex.into()),
                buffers: vec![],
            },
            fragment: Some(FragmentState {
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
                        shader: state.io.clone(),
                        entry_point: Some("calibration_vertex".into()),
                        ..default()
                    },
                    fragment: Some(FragmentState {
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
#[allow(clippy::too_many_arguments)]
fn prepare(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    mut state: ResMut<State>,
    luts: Res<Luts>,
    images: Res<RenderAssets<GpuImage>>,
    blit: Res<BlitPipeline>,
    mut specialized: ResMut<SpecializedRenderPipelines<BlitPipeline>>,
    views: Query<
        (
            Entity,
            &ExtractedCamera,
            &ViewTarget,
            &Msaa,
            &Smaa,
            Option<&ViewResources>,
        ),
        With<Smaa>,
    >,
) {
    let (Some(area), Some(search)) = (images.get(&luts.area), images.get(&luts.search)) else {
        return;
    };
    for (entity, camera, target, msaa, mode, existing) in &views {
        let Some(size) = camera.physical_target_size else {
            continue;
        };
        if msaa.samples() != mode.0.samples() {
            continue;
        }
        let format = target.main_texture_format();
        let output_textures = [target.main_texture(), target.main_texture_other()];
        let output_ids = output_textures.map(Texture::id);
        if existing.is_some_and(|existing| {
            existing.size == size
                && existing.format == format
                && existing.mode == *mode
                && existing
                    .srgb_outputs
                    .iter()
                    .all(|(id, _)| output_ids.contains(id))
        }) {
            continue;
        }
        let pipelines = if let Some(pipelines) = state.pipelines.get(&(format, mode.0)) {
            *pipelines
        } else {
            let pipelines = queue_pipelines(&state, &cache, format, *mode, &blit, &mut specialized);
            state.pipelines.insert((format, mode.0), pipelines);
            pipelines
        };
        // Never suppress Bevy's main pass while shader/LUT compilation is pending.
        if !pipelines.ready(&cache) {
            commands.entity(entity).remove::<ViewResources>();
            continue;
        }
        // These attachments persist with the view. TextureCache is for
        // frame-local borrows and could hand a retained texture to another view.
        let texture = |label, dimensions: UVec2, format: TextureFormat, samples| {
            let alternate_format = format.add_srgb_suffix();
            let texture = device.create_texture(&TextureDescriptor {
                label: Some(label),
                size: dimensions.to_extents(),
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
                view_formats: if alternate_format != format {
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
        };
        let spatial = mode.s2x().then(|| {
            let msaa = texture("S2x readable MSAA", size, format, 2);
            let calibration = texture("S2x measured positions", UVec2::ONE, format, 2);
            let colors = [
                texture("S2x sample 0", size, format, 1),
                texture("S2x sample 1", size, format, 1),
            ];
            let split = device.create_bind_group(
                "S2x split",
                &cache.get_bind_group_layout(&state.layouts.split),
                &BindGroupEntries::sequential((&msaa.default_view,)),
            );
            SpatialSamples {
                calibrated: std::sync::atomic::AtomicBool::new(false),
                msaa,
                calibration,
                colors,
                split,
            }
        });
        let edges = texture("SMAA edges", size, TextureFormat::Rg8Unorm, 1);
        let stencil = texture("SMAA stencil", size, TextureFormat::Stencil8, 1);
        let weights = texture("SMAA weights", size, TextureFormat::Rgba8Unorm, 1);
        let layouts = &state.layouts;
        let common = std::array::from_fn(|sample| {
            let mut uniform = UniformBuffer::from(Info {
                rt_metrics: Vec4::new(
                    1. / size.x as f32,
                    1. / size.y as f32,
                    size.x as f32,
                    size.y as f32,
                ),
                sample: sample as u32,
            });
            uniform.write_buffer(&device, &queue);
            let bind = |view_format| {
                let source = spatial.as_ref().map_or(output_textures[sample], |spatial| {
                    &spatial.colors[sample].texture
                });
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
                if let Some(spatial) = &spatial {
                    entries.push(BindGroupEntry {
                        binding: 2,
                        resource: BindingResource::TextureView(&spatial.calibration.default_view),
                    });
                }
                device.create_bind_group(
                    "SMAA input",
                    &cache.get_bind_group_layout(&layouts.common[usize::from(mode.s2x())]),
                    &entries,
                )
            };
            // Native views feed luma thresholds (the shader re-encodes sRGB
            // reads). sRGB views provide linear interpolation and averaging.
            Inputs {
                edges: bind(format),
                neighborhood: bind(format.add_srgb_suffix()),
            }
        });
        let edge = device.create_bind_group(
            "SMAA edge",
            &cache.get_bind_group_layout(&layouts.edge),
            &BindGroupEntries::sequential((&state.sampler,)),
        );
        let weight = device.create_bind_group(
            "SMAA weights",
            &cache.get_bind_group_layout(&layouts.weight),
            &BindGroupEntries::sequential((
                &edges.default_view,
                &state.sampler,
                &search.texture_view,
                &area.texture_view,
            )),
        );
        let neighborhood = device.create_bind_group(
            "SMAA neighborhood",
            &cache.get_bind_group_layout(&layouts.neighborhood),
            &BindGroupEntries::sequential((&weights.default_view, &state.sampler)),
        );
        commands.entity(entity).insert(ViewResources {
            size,
            format,
            mode: *mode,
            spatial,
            edges,
            stencil,
            weights,
            common,
            srgb_outputs: output_textures.map(|texture| {
                (
                    texture.id(),
                    texture.create_view(&TextureViewDescriptor {
                        format: Some(format.add_srgb_suffix()),
                        ..default()
                    }),
                )
            }),
            edge,
            weight,
            neighborhood,
            pipelines,
        });
    }
}
fn regular_main_pass(view: ViewQuery<Option<&ViewResources>>, cache: Res<PipelineCache>) -> bool {
    view.into_inner()
        .is_none_or(|resources| resources.spatial.is_none() || !resources.pipelines.ready(&cache))
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
#[allow(clippy::too_many_arguments)]
fn render_s2x(
    world: &World,
    view: ViewQuery<(
        &ExtractedCamera,
        &ExtractedView,
        &ViewTarget,
        &ViewDepthTexture,
        Option<&ViewResources>,
        Option<&MainPassResolutionOverride>,
    )>,
    opaque: Res<ViewBinnedRenderPhases<Opaque3d>>,
    mask: Res<ViewBinnedRenderPhases<AlphaMask3d>>,
    transparent: Res<ViewSortedRenderPhases<Transparent3d>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    blit: Res<BlitPipeline>,
    mut ctx: RenderContext,
) {
    let entity = view.entity();
    let (camera, extracted, target, depth, Some(resources), resolution_override) =
        view.into_inner()
    else {
        return;
    };
    let Some(spatial) = &resources.spatial else {
        return;
    };
    let pipelines = &resources.pipelines;
    let raster = pipelines.raster.unwrap();
    if !pipelines.ready(&cache) {
        return;
    }
    let msaa = &spatial.msaa;
    let calibration = &spatial.calibration;
    let original = target.get_color_attachment();
    let load = original.ops.load;
    // A camera which loads an existing scene (previews, warmup views) must
    // preserve it in both spatial samples before adding its own geometry.
    if matches!(load, LoadOp::Load) {
        let bind = blit.create_bind_group(&device, target.main_texture_view(), &cache);
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("S2x load scene"),
            color_attachments: &[Some(attachment(
                &msaa.default_view,
                LoadOp::Clear(LinearRgba::BLACK),
            ))],
            ..default()
        });
        pass.set_render_pipeline(cache.get_render_pipeline(raster.copy).unwrap());
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
    {
        // Reconstruction reads both samples directly. Resolving here would
        // create an unused intermediate scene and spend extra bandwidth.
        let mut color = attachment(&msaa.default_view, LoadOp::Load);
        color.ops.load = load;
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("S2x main scene"),
            color_attachments: &[Some(color)],
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            ..default()
        });
        if let Some(viewport) =
            Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution_override)
        {
            pass.set_camera_viewport(&viewport);
        }
        let key = &extracted.retained_view_entity;
        if let Some(phase) = opaque.get(key)
            && let Err(error) = phase.render(&mut pass, world, entity)
        {
            error!("S2x opaque draw failed: {error:?}");
        }
        if let Some(phase) = mask.get(key)
            && let Err(error) = phase.render(&mut pass, world, entity)
        {
            error!("S2x alpha mask draw failed: {error:?}");
        }
        if let Some(phase) = transparent.get(key)
            && let Err(error) = phase.render(&mut pass, world, entity)
        {
            error!("S2x transparent draw failed: {error:?}");
        }
    }
    if !spatial
        .calibrated
        .swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("S2x calibrate sample order"),
            color_attachments: &[Some(attachment(
                &calibration.default_view,
                LoadOp::Clear(LinearRgba::BLACK),
            ))],
            ..default()
        });
        pass.set_render_pipeline(cache.get_render_pipeline(raster.calibrate).unwrap());
        pass.draw(0..3, 0..1);
    }
    {
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("S2x separate samples"),
            color_attachments: &spatial.colors.each_ref().map(|t| {
                Some(attachment(
                    &t.default_view,
                    LoadOp::Clear(LinearRgba::BLACK),
                ))
            }),
            ..default()
        });
        pass.set_render_pipeline(cache.get_render_pipeline(raster.split).unwrap());
        pass.set_bind_group(0, &spatial.split, &[]);
        pass.draw(0..3, 0..1);
    }
    let output = target.post_process_write();
    reconstruct(
        &mut ctx,
        resources,
        pipelines,
        &cache,
        output.destination_texture.id(),
        &[0, 1],
    );
}

fn render_regular(
    view: ViewQuery<(&ViewTarget, &ViewResources)>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (target, resources) = view.into_inner();
    if resources.spatial.is_some() || !resources.pipelines.ready(&cache) {
        return;
    }
    let output = target.post_process_write();
    let source = resources
        .srgb_outputs
        .iter()
        .position(|(id, _)| *id == output.source_texture.id())
        .unwrap();
    reconstruct(
        &mut ctx,
        resources,
        &resources.pipelines,
        &cache,
        output.destination_texture.id(),
        &[source],
    );
}

fn reconstruct(
    ctx: &mut RenderContext,
    resources: &ViewResources,
    pipelines: &Pipelines,
    cache: &PipelineCache,
    destination_id: TextureId,
    samples: &[usize],
) {
    // sRGB attachment views store encoded scene bytes. The second S2x
    // pass also decodes the first result before averaging in linear light.
    let destination = &resources
        .srgb_outputs
        .iter()
        .find(|(id, _)| *id == destination_id)
        .unwrap()
        .1;
    for (sample, &input_index) in samples.iter().enumerate() {
        for phase in 0..3 {
            let (destination, pipeline, group) = match phase {
                0 => (
                    &resources.edges.default_view,
                    pipelines.edge,
                    &resources.edge,
                ),
                1 => (
                    &resources.weights.default_view,
                    pipelines.weight,
                    &resources.weight,
                ),
                _ => (
                    destination,
                    pipelines.neighborhood[sample],
                    &resources.neighborhood,
                ),
            };
            let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
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
                    view: &resources.stencil.default_view,
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
            pass.set_render_pipeline(cache.get_render_pipeline(pipeline).unwrap());
            let input = if phase == 2 {
                &resources.common[input_index].neighborhood
            } else {
                &resources.common[input_index].edges
            };
            pass.set_bind_group(0, input, &[]);
            pass.set_bind_group(1, group, &[]);
            if phase < 2 {
                pass.set_stencil_reference(1);
            }
            if phase == 2 && sample == 1 {
                pass.set_blend_constant(LinearRgba::new(0.5, 0.5, 0.5, 0.5).into());
            }
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests;
