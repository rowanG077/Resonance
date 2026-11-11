//! Opt-in diagnostic scopes; inclusive synchronous wall time, with worker overlap.
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::Instant,
};

pub(crate) fn enabled() -> bool {
    *OnceLock::get_or_init(&ENABLED, || {
        std::env::var_os("RESONANCE_PREPARATION_TRACE").is_some()
    })
}
static ENABLED: OnceLock<bool> = OnceLock::new();
type Counters = BTreeMap<&'static str, (u64, f64, f64)>;
fn counters() -> &'static Mutex<Counters> {
    static COUNTERS: OnceLock<Mutex<Counters>> = OnceLock::new();
    COUNTERS.get_or_init(Default::default)
}
pub(crate) struct Span(&'static str, Instant);
impl Span {
    pub fn new(label: &'static str) -> Option<Self> {
        enabled().then(|| Self(label, Instant::now()))
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        let elapsed = self.1.elapsed().as_secs_f64() * 1000.;
        let mut counters = counters().lock().unwrap();
        let entry = counters.entry(self.0).or_default();
        entry.0 += 1;
        entry.1 += elapsed;
        entry.2 = entry.2.max(elapsed);
    }
}
pub(crate) fn take() -> serde_json::Value {
    if !enabled() {
        return serde_json::Value::Null;
    }
    serde_json::Value::Object(
        std::mem::take(&mut *counters().lock().unwrap())
            .into_iter()
            .map(|(label, (calls, total, max))| {
                (
                    label.into(),
                    serde_json::json!({"calls":calls,"total_ms":total,"max_ms":max}),
                )
            })
            .collect(),
    )
}

/// Diagnostic schedules wrap whole main stages without changing system order.
pub(crate) fn install(app: &mut bevy::prelude::App) {
    if !enabled() {
        return;
    }
    use bevy::{app::MainScheduleOrder, ecs::schedule::ScheduleLabel, prelude::*};
    #[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
    struct Probe(&'static str, bool);
    fn wrap(app: &mut App, stage: impl ScheduleLabel + Clone, name: &'static str) {
        let span = std::sync::Arc::new(Mutex::new(None));
        let begin = span.clone();
        app.add_systems(Probe(name, false), move || {
            *begin.lock().unwrap() = Span::new(name);
        });
        app.add_systems(Probe(name, true), move || {
            span.lock().unwrap().take();
        });
        let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
        order.insert_before(stage.clone(), Probe(name, false));
        order.insert_after(stage, Probe(name, true));
    }
    wrap(app, First, "main First");
    wrap(app, PreUpdate, "main PreUpdate");
    wrap(app, RunFixedMainLoop, "main fixed loop");
    wrap(app, Update, "main Update");
    wrap(app, PostUpdate, "main PostUpdate");
    wrap(app, Last, "main Last");
    use bevy::render::{Render, RenderApp, RenderSystems};
    let render = app.sub_app_mut(RenderApp);
    for (before, stage, after, name) in [
        (
            RenderSystems::ExtractCommands,
            RenderSystems::PrepareAssets,
            RenderSystems::PrepareMeshes,
            "render assets",
        ),
        (
            RenderSystems::PhaseSort,
            RenderSystems::Prepare,
            RenderSystems::Render,
            "render prepare",
        ),
        (
            RenderSystems::Prepare,
            RenderSystems::Render,
            RenderSystems::Cleanup,
            "render submit",
        ),
    ] {
        let span = std::sync::Arc::new(Mutex::new(None));
        let begin = span.clone();
        render.add_systems(
            Render,
            (move || {
                *begin.lock().unwrap() = Span::new(name);
            })
            .after(before)
            .before(stage.clone()),
        );
        render.add_systems(
            Render,
            (move || {
                span.lock().unwrap().take();
            })
            .after(stage)
            .before(after),
        );
    }
    macro_rules! asset_probe {
        ($system:path, $name:literal) => {{
            let span = std::sync::Arc::new(Mutex::new(None));
            let begin = span.clone();
            render.add_systems(
                Render,
                (move || {
                    *begin.lock().unwrap() = Span::new($name);
                })
                .in_set(RenderSystems::PrepareAssets)
                .before($system),
            );
            render.add_systems(
                Render,
                (move || {
                    span.lock().unwrap().take();
                })
                .in_set(RenderSystems::PrepareAssets)
                .after($system),
            );
        }};
    }
    use bevy::render::{
        erased_render_asset::prepare_erased_assets, mesh::RenderMesh, render_asset::prepare_assets,
        texture::GpuImage,
    };
    asset_probe!(
        bevy::render::mesh::allocator::allocate_and_free_meshes,
        "through mesh allocation"
    );
    asset_probe!(prepare_assets::<GpuImage>, "through image preparation");
    asset_probe!(prepare_assets::<RenderMesh>, "through mesh preparation");
    asset_probe!(
        prepare_erased_assets::<MeshMaterial3d<crate::materials::TitleSurface>>,
        "through model material preparation"
    );
    asset_probe!(
        prepare_erased_assets::<MeshMaterial2d<crate::field_ui::Surface>>,
        "through UI material preparation"
    );
}
