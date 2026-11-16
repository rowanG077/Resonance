//! Opt-in wall timings around render stages, paired with Bevy GPU diagnostics.
//! These samples are asynchronous relative to main-loop frames.
use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        renderer::{RenderGraph, RenderGraphSystems},
    },
};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Resource, Clone, Default)]
pub(super) struct Shared(Arc<Mutex<Sample>>);
#[derive(Clone, Default, serde::Serialize)]
pub(super) struct Sample {
    frame: u64,
    total_ms: f64,
    thread_cpu_ms: f64,
    assets_thread_cpu_ms: f64,
    resources_thread_cpu_ms: f64,
    encode_thread_cpu_ms: f64,
    submit_thread_cpu_ms: f64,
    prepare_ms: f64,
    assets_ms: f64,
    geometry_ms: f64,
    views_ms: f64,
    queue_sort_ms: f64,
    resources_ms: f64,
    encode_ms: f64,
    submit_ms: f64,
    present_cleanup_ms: f64,
    title_records_updated: usize,
    title_record_bytes: usize,
    title_record_uploads: usize,
    // Historical schema names: title and world material-record writes only.
    // Native frame tables and Bevy mesh/image uploads are outside these totals.
    scene_upload_copies: usize,
    scene_upload_bytes: usize,
    /// Packed geometry snapshot copies and vertex-buffer uploads, separately.
    geometry_streams: crate::native::render::GeometryStats,
    meshes_extracted: usize,
    images_extracted: usize,
    /// Resolved draw items, including UI/offscreen items and pending pipelines.
    scene_items: usize,
}
impl Shared {
    pub fn snapshot(&self) -> Sample {
        self.0.lock().unwrap().clone()
    }
}
#[derive(Resource)]
struct Times {
    start: Instant,
    graph: Instant,
    encoded: Instant,
    submitted: Instant,
    assets: Instant,
    views_start: Instant,
    views_end: Instant,
    sorted: Instant,
    extracted: [usize; 2],
    cpu: [Option<u64>; 6],
}

pub(super) fn install(app: &mut App, pass_diagnostics: bool) {
    app.init_resource::<crate::renderer::TransformStats>();
    app.world_mut()
        .resource_mut::<crate::renderer::TransformStats>()
        .enabled = true;
    super::gpu_profile::install(app);
    let shared = Shared::default();
    app.insert_resource(shared.clone());
    if pass_diagnostics {
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
    }
    let render = app.sub_app_mut(RenderApp);
    let now = Instant::now();
    render
        .insert_resource(shared)
        .insert_resource(Times {
            start: now,
            graph: now,
            encoded: now,
            submitted: now,
            assets: now,
            views_start: now,
            views_end: now,
            sorted: now,
            extracted: [0; 2],
            cpu: [None; 6],
        })
        .add_systems(
            Render,
            (
                start.before(RenderSystems::ExtractCommands),
                assets
                    .after(RenderSystems::PrepareAssets)
                    .before(RenderSystems::PrepareMeshes),
                views_start
                    .after(RenderSystems::Specialize)
                    .before(RenderSystems::PrepareViews),
                views_end
                    .after(RenderSystems::PrepareViews)
                    .before(RenderSystems::Queue),
                sorted
                    .after(RenderSystems::PhaseSort)
                    .before(RenderSystems::Prepare),
                finish.after(RenderSystems::PostCleanup),
            ),
        )
        .add_systems(
            RenderGraph,
            (
                graph.before(RenderGraphSystems::Begin),
                encoded
                    .after(RenderGraphSystems::Render)
                    .before(RenderGraphSystems::Submit),
                submitted
                    .after(RenderGraphSystems::Submit)
                    .before(RenderGraphSystems::Finish),
            ),
        );
}
fn start(
    _: bevy::ecs::system::NonSendMarker,
    mut t: ResMut<Times>,
    meshes: Option<
        Res<bevy::render::render_asset::ExtractedAssets<bevy::render::mesh::RenderMesh>>,
    >,
    images: Option<
        Res<bevy::render::render_asset::ExtractedAssets<bevy::render::texture::GpuImage>>,
    >,
) {
    t.start = Instant::now();
    t.cpu[0] = super::thread_cpu::now();
    t.extracted = [
        meshes.as_ref().map_or(0, |a| a.extracted.len()),
        images.as_ref().map_or(0, |a| a.extracted.len()),
    ];
}
fn graph(_: bevy::ecs::system::NonSendMarker, mut t: ResMut<Times>) {
    t.graph = Instant::now();
    t.cpu[3] = super::thread_cpu::now();
}
fn assets(_: bevy::ecs::system::NonSendMarker, mut t: ResMut<Times>) {
    t.assets = Instant::now();
    t.cpu[1] = super::thread_cpu::now();
}
fn views_start(mut t: ResMut<Times>) {
    t.views_start = Instant::now();
}
fn views_end(mut t: ResMut<Times>) {
    t.views_end = Instant::now();
}
fn sorted(_: bevy::ecs::system::NonSendMarker, mut t: ResMut<Times>) {
    t.sorted = Instant::now();
    t.cpu[2] = super::thread_cpu::now();
}
fn encoded(_: bevy::ecs::system::NonSendMarker, mut t: ResMut<Times>) {
    t.encoded = Instant::now();
    t.cpu[4] = super::thread_cpu::now();
}
fn submitted(_: bevy::ecs::system::NonSendMarker, mut t: ResMut<Times>) {
    t.submitted = Instant::now();
    t.cpu[5] = super::thread_cpu::now();
}
fn finish(
    _: bevy::ecs::system::NonSendMarker,
    t: Res<Times>,
    shared: Res<Shared>,
    records: Option<Res<crate::materials::SurfaceRecordStats>>,
    uploads: Option<Res<crate::renderer::Uploads>>,
    drawn: Option<Res<crate::native::Drawn>>,
) {
    let mut s = shared.0.lock().unwrap();
    s.frame += 1;
    s.total_ms = t.start.elapsed().as_secs_f64() * 1000.;
    s.thread_cpu_ms = super::thread_cpu::elapsed_ms(t.cpu[0], super::thread_cpu::now());
    s.assets_thread_cpu_ms = super::thread_cpu::elapsed_ms(t.cpu[0], t.cpu[1]);
    s.resources_thread_cpu_ms = super::thread_cpu::elapsed_ms(t.cpu[2], t.cpu[3]);
    s.encode_thread_cpu_ms = super::thread_cpu::elapsed_ms(t.cpu[3], t.cpu[4]);
    s.submit_thread_cpu_ms = super::thread_cpu::elapsed_ms(t.cpu[4], t.cpu[5]);
    s.prepare_ms = t.graph.duration_since(t.start).as_secs_f64() * 1000.;
    s.assets_ms = t.assets.duration_since(t.start).as_secs_f64() * 1000.;
    s.geometry_ms = t.views_start.duration_since(t.assets).as_secs_f64() * 1000.;
    s.views_ms = t.views_end.duration_since(t.views_start).as_secs_f64() * 1000.;
    s.queue_sort_ms = t.sorted.duration_since(t.views_end).as_secs_f64() * 1000.;
    s.resources_ms = t.graph.duration_since(t.sorted).as_secs_f64() * 1000.;
    s.encode_ms = t.encoded.duration_since(t.graph).as_secs_f64() * 1000.;
    s.submit_ms = t.submitted.duration_since(t.encoded).as_secs_f64() * 1000.;
    s.present_cleanup_ms = t.submitted.elapsed().as_secs_f64() * 1000.;
    s.title_records_updated = records.as_ref().map_or(0, |r| r.updated);
    s.title_record_bytes = records.as_ref().map_or(0, |r| r.bytes);
    s.title_record_uploads = records.as_ref().map_or(0, |r| r.uploads);
    s.scene_upload_copies = uploads.as_ref().map_or(0, |u| u.stats.copies);
    s.scene_upload_bytes = uploads.as_ref().map_or(0, |u| u.stats.bytes);
    s.geometry_streams = drawn
        .as_ref()
        .map_or_else(Default::default, |drawn| drawn.geometry);
    s.meshes_extracted = t.extracted[0];
    s.images_extracted = t.extracted[1];
    s.scene_items = drawn.map_or(0, |drawn| drawn.draws.len());
}
