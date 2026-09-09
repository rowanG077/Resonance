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
    prepare_ms: f64,
    assets_ms: f64,
    geometry_ms: f64,
    views_ms: f64,
    queue_sort_ms: f64,
    resources_ms: f64,
    encode_ms: f64,
    submit_ms: f64,
    present_cleanup_ms: f64,
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
}

pub(super) fn install(app: &mut App) {
    let shared = Shared::default();
    app.insert_resource(shared.clone())
        .add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
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
fn start(mut t: ResMut<Times>) {
    t.start = Instant::now();
}
fn graph(mut t: ResMut<Times>) {
    t.graph = Instant::now();
}
fn assets(mut t: ResMut<Times>) {
    t.assets = Instant::now();
}
fn views_start(mut t: ResMut<Times>) {
    t.views_start = Instant::now();
}
fn views_end(mut t: ResMut<Times>) {
    t.views_end = Instant::now();
}
fn sorted(mut t: ResMut<Times>) {
    t.sorted = Instant::now();
}
fn encoded(mut t: ResMut<Times>) {
    t.encoded = Instant::now();
}
fn submitted(mut t: ResMut<Times>) {
    t.submitted = Instant::now();
}
fn finish(t: Res<Times>, shared: Res<Shared>) {
    let mut s = shared.0.lock().unwrap();
    s.frame += 1;
    s.total_ms = t.start.elapsed().as_secs_f64() * 1000.;
    s.prepare_ms = t.graph.duration_since(t.start).as_secs_f64() * 1000.;
    s.assets_ms = t.assets.duration_since(t.start).as_secs_f64() * 1000.;
    s.geometry_ms = t.views_start.duration_since(t.assets).as_secs_f64() * 1000.;
    s.views_ms = t.views_end.duration_since(t.views_start).as_secs_f64() * 1000.;
    s.queue_sort_ms = t.sorted.duration_since(t.views_end).as_secs_f64() * 1000.;
    s.resources_ms = t.graph.duration_since(t.sorted).as_secs_f64() * 1000.;
    s.encode_ms = t.encoded.duration_since(t.graph).as_secs_f64() * 1000.;
    s.submit_ms = t.submitted.duration_since(t.encoded).as_secs_f64() * 1000.;
    s.present_cleanup_ms = t.submitted.elapsed().as_secs_f64() * 1000.;
}
