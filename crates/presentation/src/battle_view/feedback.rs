//! The retained-frame expansion in battle REL 1878/C49C, before the HUD.
//! The encounter supplies C5B4's amount; rendering never advances its clock.
use anyhow::Result;
use bevy::{
    core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d},
    image::ImageSampler,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
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
        view::ViewTarget,
    },
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub generation: u64,
    pub update: u64,
    pub amount: u8,
    pub alpha: u8,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            generation: 0,
            update: 0,
            amount: 0,
            alpha: 128,
        }
    }
}

#[derive(Default)]
struct Warmup {
    encoded: AtomicBool,
    submitted: AtomicBool,
    completed: AtomicBool,
    error: Mutex<Option<String>>,
}

#[derive(Component, Clone, ExtractComponent)]
struct Capture {
    images: [Handle<Image>; 2],
    frame: Frame,
    warming: bool,
    warmup: Arc<Warmup>,
}

#[derive(Component, Clone, ExtractComponent, ShaderType)]
struct Settings {
    parameters: Vec4,
}

pub(super) struct Feedback(Capture);

impl Feedback {
    pub fn new(images: &mut Assets<Image>) -> Self {
        // C49C(2, 4): rodata490[1] is GX_RGBA8, with both dimensions halved.
        let images = std::array::from_fn(|_| {
            let mut image = Image::new_target_texture(320, 224, TextureFormat::Bgra8Unorm, None);
            image.sampler = ImageSampler::linear();
            images.add(image)
        });
        Self(Capture {
            images,
            frame: Frame {
                amount: 8,
                ..default()
            },
            warming: true,
            warmup: Arc::default(),
        })
    }

    pub fn attach(&self, commands: &mut Commands, camera: Entity) {
        commands.entity(camera).insert((
            self.0.clone(),
            Settings {
                parameters: Vec4::new(
                    f32::from(self.0.frame.amount),
                    f32::from(self.0.frame.alpha) / 255.,
                    0.,
                    0.,
                ),
            },
        ));
    }

    pub fn apply(&mut self, commands: &mut Commands, camera: Entity, frame: Frame) {
        self.0.frame = frame;
        self.0.warming = false;
        self.attach(commands, camera);
    }

    /// Attach this fresh request after changing the camera to its final target.
    /// An outstanding callback for the isolated target cannot release this gate.
    pub fn rearm(&mut self) {
        self.0.warmup = Arc::default();
        self.0.warming = true;
        self.0.frame = Frame {
            amount: 8,
            ..default()
        };
    }

    /// Both passes and both retained images must have reached the GPU.
    pub fn ready(&self) -> Result<bool> {
        if let Some(error) = &*self.0.warmup.error.lock().unwrap() {
            anyhow::bail!("battle feedback pipeline failed: {error}");
        }
        Ok(self.0.warmup.completed.load(Ordering::Acquire))
    }
}

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "feedback.wgsl");
    app.add_plugins((
        ExtractComponentPlugin::<Capture>::default(),
        ExtractComponentPlugin::<Settings>::default(),
        UniformComponentPlugin::<Settings>::default(),
    ));
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, prepare)
        .add_systems(
            Render,
            prepare_composites.in_set(RenderSystems::PrepareResources),
        )
        .add_systems(Core3d, render.in_set(Core3dSystems::PostProcess))
        .add_systems(Render, submitted.in_set(RenderSystems::Cleanup));
}

#[derive(Resource)]
struct Pipeline {
    composite_layout: BindGroupLayoutDescriptor,
    capture_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    composite: RenderPipelineDescriptor,
    composites: HashMap<TextureFormat, CachedRenderPipelineId>,
    capture: CachedRenderPipelineId,
}

fn prepare(
    mut commands: Commands,
    device: Res<RenderDevice>,
    server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let composite_layout = BindGroupLayoutDescriptor::new(
        "resonance/battle-feedback",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                uniform_buffer::<Settings>(true),
            ),
        ),
    );
    let capture_layout = BindGroupLayoutDescriptor::new(
        "resonance/battle-feedback-capture",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let pipeline = |label: &'static str,
                    layout: &BindGroupLayoutDescriptor,
                    entry: &'static str| {
        RenderPipelineDescriptor {
            label: Some(label.into()),
            layout: vec![layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: server.load("embedded://resonance_presentation/battle_view/feedback.wgsl"),
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Bgra8Unorm,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    };
    let composite = pipeline("resonance/battle-feedback", &composite_layout, "composite");
    let capture = cache.queue_render_pipeline(pipeline(
        "resonance/battle-feedback-capture",
        &capture_layout,
        "capture",
    ));
    commands.insert_resource(Pipeline {
        composite_layout,
        capture_layout,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        composite,
        composites: HashMap::new(),
        capture,
    });
}

fn prepare_composites(
    targets: Query<(&ViewTarget, &Capture)>,
    mut pipeline: ResMut<Pipeline>,
    cache: Res<PipelineCache>,
) {
    for (target, request) in &targets {
        let format = target.main_texture_format();
        if pipeline.composites.contains_key(&format) {
            continue;
        }
        if !request.warming {
            request.warmup.error.lock().unwrap().get_or_insert_with(|| {
                "battle feedback target format changed after activation".into()
            });
            continue;
        }
        // The isolated image and the final camera can use different formats.
        // Only the composite writes to the camera's ping-pong target; retained
        // captures keep their own fixed format. Prepare both before activation.
        let mut descriptor = pipeline.composite.clone();
        descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .format = format;
        let id = cache.queue_render_pipeline(descriptor);
        pipeline.composites.insert(format, id);
    }
}

#[derive(Default)]
struct History {
    generation: Option<u64>,
    update: Option<u64>,
    latest: usize,
    previous: Option<usize>,
}
impl History {
    /// Repeated presentations of one held simulation frame use the same input
    /// image. Only a new source world visit can replace the retained output.
    fn advance(&mut self, frame: Frame) -> bool {
        if self.generation != Some(frame.generation) {
            *self = Self {
                generation: Some(frame.generation),
                ..default()
            };
        }
        if self.update == Some(frame.update) {
            return false;
        }
        self.previous = self.update.map(|_| self.latest);
        self.latest = self.previous.map_or(0, |previous| 1 - previous);
        self.update = Some(frame.update);
        true
    }
}

#[derive(Default)]
struct Bindings {
    owner: Option<AssetId<Image>>,
    identity: Option<(TextureViewId, TextureViewId, BufferId)>,
    composites: HashMap<(TextureViewId, usize), BindGroup>,
    captures: HashMap<TextureViewId, BindGroup>,
    history: History,
}

#[allow(clippy::too_many_arguments)] // Existing camera target, two retained images, and a GPU fence.
fn render(
    view: ViewQuery<(&ViewTarget, &Capture, &DynamicUniformIndex<Settings>)>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<Settings>>,
    images: Res<RenderAssets<GpuImage>>,
    mut bindings: Local<Bindings>,
    mut context: RenderContext,
) {
    let (target, request, index) = view.into_inner();
    let Some(&composite) = pipeline.composites.get(&target.main_texture_format()) else {
        return;
    };
    for id in [composite, pipeline.capture] {
        if let CachedPipelineState::Err(error) = cache.get_render_pipeline_state(id) {
            request
                .warmup
                .error
                .lock()
                .unwrap()
                .get_or_insert_with(|| error.to_string());
            return;
        }
    }
    let (Some(composite_pipeline), Some(capture_pipeline)) = (
        cache.get_render_pipeline(composite),
        cache.get_render_pipeline(pipeline.capture),
    ) else {
        return;
    };
    let (Some(first), Some(second), Some(buffer)) = (
        images.get(&request.images[0]),
        images.get(&request.images[1]),
        uniforms.uniforms().buffer(),
    ) else {
        return;
    };
    let retained = [&first.texture_view, &second.texture_view];
    if bindings.owner != Some(request.images[0].id()) {
        *bindings = Bindings {
            owner: Some(request.images[0].id()),
            ..default()
        };
    }
    let a = target.main_texture_view();
    let b = target.main_texture_other_view();
    // Both ping-pong views are prepared together. Their order changes after a
    // post-process write, so the comparison treats it as an unordered pair.
    let changed = !bindings.identity.is_some_and(|(x, y, uniform)| {
        uniform == buffer.id() && (x == a.id() && y == b.id() || x == b.id() && y == a.id())
    });
    if changed {
        if !request.warming {
            request.warmup.error.lock().unwrap().get_or_insert_with(|| {
                "battle feedback target or uniform binding changed after activation".into()
            });
            return;
        }
        bindings.identity = Some((a.id(), b.id(), buffer.id()));
        bindings.composites.clear();
        bindings.captures.clear();
        for source in [a, b] {
            bindings.captures.insert(
                source.id(),
                context.render_device().create_bind_group(
                    "resonance/battle-feedback-capture",
                    &cache.get_bind_group_layout(&pipeline.capture_layout),
                    &BindGroupEntries::sequential((source, &pipeline.sampler)),
                ),
            );
            for (slot, image) in retained.iter().enumerate() {
                bindings.composites.insert(
                    (source.id(), slot),
                    context.render_device().create_bind_group(
                        "resonance/battle-feedback",
                        &cache.get_bind_group_layout(&pipeline.composite_layout),
                        &BindGroupEntries::sequential((
                            source,
                            &pipeline.sampler,
                            *image,
                            uniforms.uniforms().binding().unwrap(),
                        )),
                    ),
                );
            }
        }
    }
    if !request.warming && request.frame.amount == 0 {
        return;
    }
    let capture = if request.warming {
        true
    } else {
        bindings.history.advance(request.frame)
    };
    let previous = if request.warming {
        Some(0)
    } else {
        bindings.history.previous
    };
    if request.warming {
        // Initialize before the warm composite samples it, then exercise the
        // other retained image with the completed composite below.
        draw(
            &mut context,
            retained[0],
            capture_pipeline,
            &bindings.captures[&a.id()],
            &[],
        );
    }
    if let Some(previous) = previous {
        let post = target.post_process_write();
        draw(
            &mut context,
            post.destination,
            composite_pipeline,
            &bindings.composites[&(post.source.id(), previous)],
            &[index.index()],
        );
    }
    if capture {
        let destination = if request.warming {
            1
        } else {
            bindings.history.latest
        };
        draw(
            &mut context,
            retained[destination],
            capture_pipeline,
            &bindings.captures[&target.main_texture_view().id()],
            &[],
        );
    }
    if request.warming {
        request.warmup.encoded.store(true, Ordering::Release);
    }
}

fn draw(
    context: &mut RenderContext,
    target: &TextureView,
    pipeline: &RenderPipeline,
    binding: &BindGroup,
    offsets: &[u32],
) {
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("resonance/battle-feedback"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(default()),
                    store: StoreOp::Store,
                },
            })],
            ..default()
        });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, binding, offsets);
    pass.draw(0..3, 0..1);
}

fn submitted(requests: Query<&Capture>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let _ = device.poll(PollType::Poll);
    for request in &requests {
        if request.warmup.encoded.load(Ordering::Acquire)
            && !request.warmup.submitted.swap(true, Ordering::AcqRel)
        {
            let warmup = request.warmup.clone();
            queue.on_submitted_work_done(move || warmup.completed.store(true, Ordering::Release));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_frame_changes_once_per_visit_and_resets_for_a_new_request() {
        let mut history = History::default();
        let mut frame = Frame {
            generation: 1,
            update: 100,
            amount: 1,
            alpha: 128,
        };
        assert!(history.advance(frame));
        assert_eq!(history.previous, None);
        assert_eq!(history.latest, 0);
        assert!(!history.advance(frame));
        assert_eq!(history.previous, None);
        frame.update += 1;
        frame.amount += 1;
        assert!(history.advance(frame));
        assert_eq!(history.previous, Some(0));
        assert_eq!(history.latest, 1);
        assert!(!history.advance(frame));
        assert_eq!(history.previous, Some(0));
        frame.update += 1;
        assert!(history.advance(frame));
        assert_eq!(history.previous, Some(1));
        assert_eq!(history.latest, 0);
        frame.generation += 1;
        assert!(history.advance(frame));
        assert_eq!(history.previous, None);
    }
}
