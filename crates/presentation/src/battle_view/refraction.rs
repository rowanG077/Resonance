//! The screen-texture list at the end of 484D8. Its particles use the existing
//! mesh/material renderer, after a current-frame C49C(1, 6) scene capture.
use super::Warmup;
use bevy::{
    camera::Viewport,
    core_pipeline::{Core3dSystems, FullscreenShader, core_3d::Transparent3d, schedule::Core3d},
    image::ImageSampler,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        render_phase::{PhaseItem, ViewSortedRenderPhases},
        render_resource::{
            binding_types::{sampler, texture_2d},
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::GpuImage,
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) const ORDER: u32 = crate::draw_order::EFFECTS + 65536;

#[derive(Component, Clone, Copy, ExtractComponent)]
pub(super) struct ScreenDraw(pub bool);

#[derive(Component, Clone, ExtractComponent)]
pub(super) struct Capture {
    pub image: Handle<Image>,
    warmup: Warmup,
    encoded: Arc<AtomicBool>,
}
impl Capture {
    pub fn new(images: &mut Assets<Image>, warmup: &Warmup) -> Self {
        // Native copy format 4, source rectangle (0,0,640,448), half-size.
        // RGB565 is expanded to RGBA8 for filtering; alpha is always one.
        let mut image = Image::new_target_texture(320, 224, TextureFormat::Bgra8Unorm, None);
        image.sampler = ImageSampler::linear();
        Self {
            image: images.add(image),
            warmup: warmup.clone(),
            encoded: warmup.0.lock().unwrap().scene_encoded.clone(),
        }
    }

    pub fn attach(&self, commands: &mut Commands, camera: Entity) {
        commands.entity(camera).insert(self.clone());
    }

    pub fn rearm(&mut self) {
        let mut report = self.warmup.0.lock().unwrap();
        self.encoded = Arc::new(AtomicBool::new(report.screen_failed));
        report.scene_encoded = self.encoded.clone();
    }

    fn fail(&self, error: String) {
        let mut report = self.warmup.0.lock().unwrap();
        if report.tolerant() {
            let _ = report
                .diagnostics
                .as_ref()
                .unwrap()
                .report("battle screen capture disabled", anyhow::anyhow!(error));
            report.screen_failed = true;
            self.encoded.store(true, Ordering::Release);
        } else {
            report.draws.error.get_or_insert(error);
        }
    }
}

#[derive(Resource, Default)]
struct LatePhases(ViewSortedRenderPhases<Transparent3d>);

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "refraction.wgsl");
    app.add_plugins((
        ExtractComponentPlugin::<Capture>::default(),
        ExtractComponentPlugin::<ScreenDraw>::default(),
    ));
    app.sub_app_mut(RenderApp)
        .init_resource::<LatePhases>()
        .add_systems(RenderStartup, prepare)
        .add_systems(
            Render,
            (
                split
                    .after(RenderSystems::Prepare)
                    .before(RenderSystems::Render),
                restore
                    .after(RenderSystems::Render)
                    .before(RenderSystems::Cleanup),
            ),
        )
        .add_systems(Core3d, render.in_set(Core3dSystems::EarlyPostProcess));
}

/// Move only already-prepared draws. Screen materials have a distinct pipeline
/// and these pool meshes disable automatic batching, so no batch crosses this
/// boundary. Keep transient keys in the original phase for its next-frame sweep.
fn split(
    views: Query<&ExtractedView, With<Capture>>,
    draws: Query<&ScreenDraw>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut late: ResMut<LatePhases>,
) {
    for view in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let target = late.0.entry(view.retained_view_entity).or_default();
        let keys: Vec<_> = phase
            .items
            .iter()
            .filter_map(|(key, item)| {
                draws
                    .get(item.entity())
                    .is_ok_and(|draw| draw.0)
                    .then_some(*key)
            })
            .collect();
        for key in keys {
            let item = phase.items.shift_remove(&key).unwrap();
            target.items.insert(key, item);
        }
    }
}

/// Report observes the complete original phase only after the late pass was
/// encoded. Restore retained entries before next frame's queue and transient sweep.
fn restore(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut late: ResMut<LatePhases>,
) {
    for (view, phase) in late.0.iter_mut() {
        if let Some(target) = phases.get_mut(view) {
            target.items.extend(phase.items.drain(..));
        } else {
            phase.items.clear();
        }
    }
    late.0.retain(|view, _| phases.contains_key(view));
}

#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    capture: CachedRenderPipelineId,
}

fn prepare(
    mut commands: Commands,
    device: Res<RenderDevice>,
    server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "resonance/battle-screen-capture",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let capture = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("resonance/battle-screen-capture".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: server.load("embedded://resonance_presentation/battle_view/refraction.wgsl"),
            entry_point: Some("capture".into()),
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
        capture,
    });
}

#[derive(Default)]
struct Bindings {
    identity: Option<(TextureViewId, TextureViewId, TextureViewId)>,
    captures: HashMap<TextureViewId, BindGroup>,
}

#[allow(clippy::too_many_arguments)] // Existing mesh phase, camera attachments and one scene copy.
fn render(
    world: &World,
    view: ViewQuery<(
        &ExtractedCamera,
        &ExtractedView,
        &ViewTarget,
        &ViewDepthTexture,
        &Capture,
    )>,
    late: Res<LatePhases>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    mut bindings: Local<Bindings>,
    mut context: RenderContext,
) {
    let view_entity = view.entity();
    let (camera, extracted, target, depth, request) = view.into_inner();
    if request.warmup.0.lock().unwrap().screen_failed {
        return;
    }
    if let CachedPipelineState::Err(error) = cache.get_render_pipeline_state(pipeline.capture) {
        request.fail(format!("battle screen capture pipeline failed: {error}"));
        return;
    }
    let (Some(capture_pipeline), Some(image)) = (
        cache.get_render_pipeline(pipeline.capture),
        images.get(&request.image),
    ) else {
        return;
    };
    let a = target.main_texture_view();
    let b = target.main_texture_other_view();
    if !bindings.identity.is_some_and(|(x, y, capture)| {
        capture == image.texture_view.id()
            && (x == a.id() && y == b.id() || x == b.id() && y == a.id())
    }) {
        if request
            .warmup
            .0
            .lock()
            .unwrap()
            .prepared_pipelines
            .is_some()
        {
            request.fail("battle screen capture target changed after activation".into());
            return;
        }
        bindings.identity = Some((a.id(), b.id(), image.texture_view.id()));
        bindings.captures.clear();
        // Prepare both target views while the simulation is held.
        for source in [a, b] {
            bindings.captures.insert(
                source.id(),
                context.render_device().create_bind_group(
                    "resonance/battle-screen-capture",
                    &cache.get_bind_group_layout(&pipeline.layout),
                    &BindGroupEntries::sequential((source, &pipeline.sampler)),
                ),
            );
        }
    }
    {
        let mut pass = context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("resonance/battle-screen-capture"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &image.texture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(default()),
                        store: StoreOp::Store,
                    },
                })],
                ..default()
            });
        pass.set_pipeline(capture_pipeline);
        pass.set_bind_group(0, &bindings.captures[&a.id()], &[]);
        pass.draw(0..3, 0..1);
    }
    if let Some(phase) = late.0.get(&extracted.retained_view_entity)
        && !phase.items.is_empty()
    {
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("resonance/battle-screen-particles"),
            color_attachments: &[Some(target.get_color_attachment())],
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            ..default()
        });
        if let Some(viewport) = Viewport::from_viewport_and_override(camera.viewport.as_ref(), None)
        {
            pass.set_camera_viewport(&viewport);
        }
        if let Err(error) = phase.render(&mut pass, world, view_entity) {
            request.fail(format!("battle screen particle draw failed: {error:?}"));
            return;
        }
    }
    // The ordinary shared report adds its submitted-work fence after this pass.
    request.encoded.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        core_pipeline::core_3d::TransparentSortingInfo3d,
        ecs::system::RunSystemOnce,
        render::{
            render_phase::{DrawFunctionId, PhaseItemExtraIndex, SortedRenderPhase},
            sync_world::MainEntity,
            view::RetainedViewEntity,
        },
    };

    #[test]
    fn disabled_capture_releases_its_warmup_even_after_target_rearm() {
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(false);
        let warmup = Warmup::default();
        warmup.0.lock().unwrap().diagnostics = Some(diagnostics.clone());
        let mut images = Assets::<Image>::default();
        let mut capture = Capture::new(&mut images, &warmup);
        capture.fail("missing capture pipeline".into());
        assert!(warmup.0.lock().unwrap().screen_failed);
        assert!(capture.encoded.load(Ordering::Acquire));
        capture.rearm();
        assert!(capture.encoded.load(Ordering::Acquire));
        assert!(
            warmup
                .0
                .lock()
                .unwrap()
                .scene_encoded
                .load(Ordering::Acquire)
        );
        assert_eq!(diagnostics.entries().len(), 1);
    }

    #[test]
    fn late_draws_return_to_the_shared_report_and_next_frame_sweep() {
        let mut world = World::new();
        let entities =
            [false, false, true, true].map(|screen| world.spawn(ScreenDraw(screen)).id());
        let view = RetainedViewEntity::new(MainEntity::from(Entity::PLACEHOLDER), None, 0);
        let mut phase = SortedRenderPhase::<Transparent3d>::default();
        for (index, entity) in entities.into_iter().enumerate() {
            phase.add_transient(Transparent3d {
                sorting_info: TransparentSortingInfo3d::AlwaysOnTop,
                distance: index as f32,
                pipeline: CachedRenderPipelineId::new(index),
                entity: (entity, MainEntity::from(entity)),
                draw_function: DrawFunctionId(0),
                batch_range: index as u32..index as u32 + 1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            });
        }
        let mut phases = ViewSortedRenderPhases::default();
        phases.insert(view, phase);
        world.insert_resource(phases);
        world.init_resource::<LatePhases>();
        world.spawn((
            ExtractedView {
                retained_view_entity: view,
                clip_from_view: Mat4::IDENTITY,
                world_from_view: GlobalTransform::IDENTITY,
                clip_from_world: None,
                target_format: TextureFormat::Bgra8Unorm,
                viewport: UVec4::new(0, 0, 640, 448),
                color_grading: default(),
                invert_culling: false,
            },
            Capture {
                image: Handle::default(),
                warmup: Warmup::default(),
                encoded: Arc::default(),
            },
        ));
        world.run_system_once(split).unwrap();
        let phases = world.resource::<ViewSortedRenderPhases<Transparent3d>>();
        assert_eq!(
            phases[&view].iter_entities().collect::<Vec<_>>(),
            entities[..2]
        );
        assert_eq!(phases[&view].transient_items.len(), 4);
        let late = world.resource::<LatePhases>();
        assert_eq!(
            late.0[&view].iter_entities().collect::<Vec<_>>(),
            entities[2..]
        );
        assert_eq!(
            late.0[&view].items.values().next().unwrap().batch_range,
            2..3
        );
        world.run_system_once(restore).unwrap();
        let mut phases = world.resource_mut::<ViewSortedRenderPhases<Transparent3d>>();
        assert_eq!(phases[&view].iter_entities().collect::<Vec<_>>(), entities);
        assert_eq!(
            phases[&view]
                .items
                .values()
                .map(|p| p.pipeline.id())
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        phases.prepare_for_new_frame(view);
        assert!(phases[&view].items.is_empty());
        assert!(world.resource::<LatePhases>().0[&view].items.is_empty());
    }
}
