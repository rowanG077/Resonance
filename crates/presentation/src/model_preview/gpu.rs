use bevy::{
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d},
    prelude::*,
    render::{
        extract_resource::ExtractResource,
        render_phase::ViewSortedRenderPhases,
        render_resource::{CachedPipelineState, PipelineCache, PollType},
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
    },
};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Resource, Clone, Default)]
pub(super) struct Shared(pub Arc<Mutex<Report>>);

/// A fresh token is extracted with the pose whose render submission must finish.
#[derive(Resource, Clone, Default, ExtractResource)]
pub(super) struct Capture(pub Option<Arc<AtomicBool>>);

pub(super) fn capture_submitted(
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
#[derive(Default)]
pub(super) struct Report {
    pub armed: bool,
    pub expected: HashSet<MainEntity>,
    pub pending: HashSet<MainEntity>,
    pub completed: Arc<AtomicBool>,
    pub submitted: bool,
    pub error: Option<String>,
}
pub(super) fn rendered(
    shared: Res<Shared>,
    phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    quads: Res<ViewSortedRenderPhases<Transparent2d>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let mut report = shared.0.lock().unwrap();
    if !report.armed || report.expected.is_empty() || report.completed.load(Ordering::Acquire) {
        return;
    }
    let _ = device.poll(PollType::Poll);
    if report.submitted {
        return;
    }
    let mut ready = HashSet::new();
    for (entity, pipeline) in crate::field_warm::draws(&phases, &quads) {
        if !report.expected.contains(&entity) {
            continue;
        }
        match cache.get_render_pipeline_state(pipeline) {
            CachedPipelineState::Ok(_) => {
                ready.insert(entity);
            }
            CachedPipelineState::Err(error) => report.error = Some(error.to_string()),
            _ => {}
        }
    }
    report.pending = report.expected.difference(&ready).copied().collect();
    if report.pending.is_empty() {
        report.submitted = true;
        let completed = report.completed.clone();
        queue.on_submitted_work_done(move || completed.store(true, Ordering::Release));
    }
}
