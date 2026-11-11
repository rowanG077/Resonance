use bevy::{
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d},
    ecs::system::SystemParam,
    prelude::*,
    render::{
        erased_render_asset::ErasedRenderAssets,
        extract_resource::ExtractResource,
        material_bind_groups::MaterialBindGroupAllocators,
        mesh::RenderMesh,
        render_asset::RenderAssets,
        render_phase::ViewSortedRenderPhases,
        render_resource::{CachedPipelineState, CachedRenderPipelineId, PipelineCache, PollType},
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        texture::GpuImage,
    },
};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Bevy retries these states when the shader or one of its imports arrives.
pub(crate) fn shader_pending(error: &bevy::shader::ShaderCacheError) -> bool {
    use bevy::shader::ShaderCacheError;
    matches!(
        error,
        ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable
    )
}

#[derive(Resource, Clone, Default)]
pub(crate) struct Shared(pub Arc<Mutex<Report>>);

impl Shared {
    /// Register an independently owned preparation with the render observer.
    pub(crate) fn new(world: &World) -> Self {
        let shared = Self::default();
        world.resource::<crate::gpu_prepare::Registry>().watch(
            &shared.0,
            |report| report,
            None,
            |report, _, observer| observer.render(report, None),
        );
        shared
    }
}

/// A fresh token is extracted with the pose whose render submission must finish.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub(crate) struct Capture(pub Option<Arc<AtomicBool>>);

pub(crate) fn capture_submitted(
    capture: Res<Capture>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let Some(completed) = &capture.0 else { return };
    let _ = device.poll(PollType::Poll);
    if !completed.load(Ordering::Acquire) {
        let completed = completed.clone();
        queue.on_submitted_work_done(move || completed.store(true, Ordering::Release));
    }
}
/// One preparation report covers uploads, real draw pipelines and submission.
/// It owns no scene or asset handles; the requesting owner retains those.
#[derive(Default)]
pub(crate) struct Report {
    pub armed: bool,
    pub direct: bool,
    pub requests: Vec<crate::gpu_prepare::Request>,
    pub requested: HashSet<crate::gpu_prepare::Request>,
    pub material_bindings: HashSet<(bool, bevy::asset::UntypedAssetId)>,
    pub expected: HashSet<MainEntity>,
    pub pending: HashSet<MainEntity>,
    pub pipelines: HashSet<CachedRenderPipelineId>,
    pub meshes: HashSet<AssetId<Mesh>>,
    pub images: HashSet<AssetId<Image>>,
    pub completed: Arc<AtomicBool>,
    pub submitted: bool,
    pub error: Option<String>,
    pub seen: usize,
}

impl Report {
    /// Keep prepared assets and pipelines, but wait for a fresh submission.
    /// Earlier GPU callbacks retain their old completion token.
    pub(crate) fn restart_submission(&mut self, armed: bool) {
        self.armed = armed;
        self.submitted = false;
        self.completed = Arc::new(AtomicBool::new(false));
    }

    pub(crate) fn failure(
        &mut self,
        error: String,
        policy: Option<(&resonance_content::diagnostics::Diagnostics, &str)>,
    ) -> bool {
        if let Some((diagnostics, scope)) = policy
            && diagnostics
                .report(scope, anyhow::anyhow!(error.clone()))
                .is_ok()
        {
            true
        } else {
            self.error = Some(error);
            false
        }
    }
    fn failed_draw(
        &mut self,
        entity: MainEntity,
        error: String,
        policy: Option<(&resonance_content::diagnostics::Diagnostics, &str)>,
    ) -> bool {
        let tolerated = self.failure(error, policy);
        if tolerated {
            self.expected.remove(&entity);
        }
        tolerated
    }
}

#[derive(SystemParam)]
pub(crate) struct Observer<'w> {
    phases: Res<'w, ViewSortedRenderPhases<Transparent3d>>,
    quads: Res<'w, ViewSortedRenderPhases<Transparent2d>>,
    pub cache: Res<'w, PipelineCache>,
    device: Res<'w, RenderDevice>,
    queue: Res<'w, RenderQueue>,
    meshes: Res<'w, RenderAssets<RenderMesh>>,
    images: Res<'w, RenderAssets<GpuImage>>,
    pub materials: Res<'w, ErasedRenderAssets<bevy::pbr::PreparedMaterial>>,
    pub allocators: Res<'w, MaterialBindGroupAllocators>,
    quad_materials: Res<'w, ErasedRenderAssets<bevy::sprite_render::PreparedMaterial2d>>,
}

impl Observer<'_> {
    pub fn assets_ready(&self, report: &Report) -> bool {
        report
            .meshes
            .iter()
            .all(|id| self.meshes.get(*id).is_some())
            && report
                .images
                .iter()
                .all(|id| self.images.get(*id).is_some())
            && report.material_bindings.iter().all(|(flat, id)| {
                let binding = if *flat {
                    self.quad_materials.get(*id).map(|m| m.binding)
                } else {
                    self.materials.get(*id).map(|m| m.binding)
                };
                binding.is_some_and(|binding| {
                    self.allocators
                        .get(&id.type_id())
                        .and_then(|allocator| allocator.get(binding.group))
                        .and_then(|slab| slab.bind_group())
                        .is_some()
                })
            })
    }

    pub fn failed(&self, report: &Report) -> Option<String> {
        report.pipelines.iter().find_map(|&pipeline| {
            match self.cache.get_render_pipeline_state(pipeline) {
                CachedPipelineState::Err(error) if !shader_pending(error) => {
                    Some(error.to_string())
                }
                _ => None,
            }
        })
    }

    /// Pipeline failures follow the requesting scene's existing diagnostic policy.
    pub fn render(
        &self,
        report: &mut Report,
        policy: Option<(&resonance_content::diagnostics::Diagnostics, &str)>,
    ) {
        if !report.armed
            || report.completed.load(Ordering::Acquire)
            || !report.requests.is_empty()
            || (policy.is_none() && report.expected.is_empty() && !report.direct)
        {
            return;
        }
        let _ = self.device.poll(PollType::Poll);
        if report.submitted {
            return;
        }
        if report.direct {
            let failed: Vec<_> = report
                .pipelines
                .iter()
                .filter_map(|&id| match self.cache.get_render_pipeline_state(id) {
                    CachedPipelineState::Err(error) if !shader_pending(error) => {
                        Some((id, error.to_string()))
                    }
                    _ => None,
                })
                .collect();
            for (id, error) in failed {
                if report.failure(error, policy) {
                    report.pipelines.remove(&id);
                } else {
                    return;
                }
            }
            if !report.pipelines.iter().all(|&id| {
                matches!(
                    self.cache.get_render_pipeline_state(id),
                    CachedPipelineState::Ok(_)
                )
            }) {
                return;
            }
        }
        self.observe(report, policy, true);
        if report.error.is_none() && report.pending.is_empty() && self.assets_ready(report) {
            self.submit(report);
        }
    }

    /// Active battles still observe shader replacement failures after admission.
    pub fn watch(
        &self,
        report: &mut Report,
        policy: Option<(&resonance_content::diagnostics::Diagnostics, &str)>,
    ) {
        self.observe(report, policy, false);
    }

    fn observe(
        &self,
        report: &mut Report,
        policy: Option<(&resonance_content::diagnostics::Diagnostics, &str)>,
        collect_ready: bool,
    ) {
        let mut ready = HashSet::new();
        for (entity, pipeline) in draws(&self.phases, &self.quads) {
            if !report.expected.contains(&entity) {
                continue;
            }
            report.pipelines.insert(pipeline);
            match self.cache.get_render_pipeline_state(pipeline) {
                CachedPipelineState::Ok(_) if collect_ready => {
                    ready.insert(entity);
                }
                CachedPipelineState::Err(error) if !shader_pending(error) => {
                    if report.failed_draw(entity, error.to_string(), policy) {
                        report.pipelines.remove(&pipeline);
                    } else {
                        return;
                    }
                }
                _ => {}
            }
        }
        if collect_ready {
            ready.retain(|entity| report.expected.contains(entity));
            report.seen = ready.len();
            report.pending = report.expected.difference(&ready).copied().collect();
        }
    }

    /// Upload batches do not need draw entities, but still wait for the GPU.
    pub fn uploaded(&self, report: &mut Report) {
        if !report.armed || report.completed.load(Ordering::Acquire) {
            return;
        }
        let _ = self.device.poll(PollType::Poll);
        if !report.submitted && self.assets_ready(report) {
            self.submit(report);
        }
    }

    fn submit(&self, report: &mut Report) {
        report.submitted = true;
        let completed = report.completed.clone();
        self.queue
            .on_submitted_work_done(move || completed.store(true, Ordering::Release));
    }
}

pub(crate) fn rendered(shared: Res<Shared>, observer: Observer) {
    observer.render(&mut shared.0.lock().unwrap(), None);
}

/// Draws queued by both cameras; callers retain their own pipeline/error policy.
pub(crate) fn draws<'a>(
    models: &'a ViewSortedRenderPhases<Transparent3d>,
    ui: &'a ViewSortedRenderPhases<Transparent2d>,
) -> impl Iterator<Item = (MainEntity, CachedRenderPipelineId)> + 'a {
    let models = models
        .0
        .values()
        .flat_map(|p| p.items.values())
        .map(|p| (p.entity.1, p.pipeline));
    let ui =
        ui.0.values()
            .flat_map(|p| p.items.values())
            .map(|p| (p.entity.1, p.pipeline));
    models.chain(ui)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::diagnostics::Diagnostics;

    #[test]
    fn failed_draw_policy_preserves_healthy_work_and_never_acknowledges_the_gpu() {
        let mut world = World::new();
        let failed: MainEntity = world.spawn_empty().id().into();
        let healthy: MainEntity = world.spawn_empty().id().into();
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut report = Report {
                armed: true,
                expected: [failed, healthy].into(),
                ..default()
            };
            assert_eq!(
                report.failed_draw(failed, "bad shader".into(), Some((&diagnostics, "draw"))),
                !paranoid
            );
            assert!(report.expected.contains(&healthy));
            assert_eq!(report.expected.contains(&failed), paranoid);
            assert_eq!(report.error.is_some(), paranoid);
            assert_eq!(diagnostics.entries().len(), 1);
            if !paranoid {
                assert!(report.failed_draw(
                    healthy,
                    "also failed".into(),
                    Some((&diagnostics, "draw"))
                ));
                assert!(report.expected.is_empty());
            }
            assert!(!report.submitted);
            assert!(!report.completed.load(Ordering::Acquire));
        }
        let mut strict = Report::default();
        assert!(!strict.failed_draw(failed, "required pipeline".into(), None));
        assert_eq!(strict.error.as_deref(), Some("required pipeline"));
    }
}
