//! Optional wall-clock diagnostics. No game-clock changes or GPU synchronization.
pub(crate) mod affinity;
mod movie_probe;
mod overlay;
pub use movie_probe::run_movie_probe;
mod gpu_profile;
mod render_profile;
pub(crate) mod smoothing_profile;
mod stats;
mod thread_cpu;
mod window_probe;
pub use window_probe::{
    ScenarioBenchmark, run_frame_benchmark, run_offscreen_benchmark, run_offscreen_title,
    run_window_benchmark, run_window_probe,
};

use anyhow::{Context as ContextExt, Result};
use bevy::prelude::*;
use serde::Serialize;
use stats::{History, Sample};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub struct PerformanceOptions {
    /// Initially show the live overlay. Its Controls binding defaults to F3.
    pub overlay: bool,
    /// Continuously record frame samples and periodic rolling summaries as JSONL.
    pub dump: Option<PathBuf>,
}

#[derive(Resource)]
struct Monitor {
    started: Instant,
    frame_started: Option<Instant>,
    app_ms: f64,
    cpu_started: Option<u64>,
    app_cpu_ms: Option<f64>,
    frame: u64,
    history: History,
    shown: bool,
    file: Option<BufWriter<File>>,
    dump_directory: PathBuf,
    last_flush: Instant,
    notice: &'static str,
    notice_until: Option<Instant>,
}

#[derive(Serialize)]
struct Metadata {
    kind: &'static str,
    version: u8,
    unix_seconds: u64,
    headless: bool,
    window_seconds: f64,
    frame_time: &'static str,
    app_time: &'static str,
    low_fps: &'static str,
}

fn metadata(headless: bool) -> Metadata {
    Metadata {
        kind: "metadata",
        version: 1,
        unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        headless,
        window_seconds: stats::WINDOW_SECONDS,
        frame_time: "Wall time between main-loop starts; includes pacing, vsync and stalls. Not GPU presentation timestamps.",
        app_time: "Previous main-app First-to-Last schedule duration; excludes separate render-app/GPU work. Not GPU frame time.",
        low_fps: "1000 / nearest-rank p99 or p99.9 frame time in milliseconds; not a mean of the slowest subset. Warm-up: 1000 samples.",
    }
}

pub(crate) fn elapsed_seconds(world: &World) -> Option<f64> {
    world
        .get_resource::<Monitor>()
        .map(|monitor| monitor.started.elapsed().as_secs_f64())
}

fn create_file(path: &Path) -> Result<BufWriter<File>> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| {
                format!(
                    "create performance dump {} (existing files are preserved)",
                    path.display()
                )
            })?,
    ))
}

fn line(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub(super) fn install(app: &mut App, options: PerformanceOptions, headless: bool) -> Result<()> {
    let now = Instant::now();
    if let Some(gpu) = app.world().get_resource::<gpu_profile::Shared>() {
        gpu.set_origin(now);
    }
    let mut file = options.dump.as_deref().map(create_file).transpose()?;
    if let Some(writer) = &mut file {
        line(writer, &metadata(headless))?;
        writer.flush()?;
    }
    app.insert_resource(Monitor {
        started: now,
        frame_started: None,
        app_ms: 0.,
        cpu_started: None,
        app_cpu_ms: None,
        frame: 0,
        history: History::default(),
        // `RESONANCE_PERFORMANCE_OVERLAY=1` starts with the panel shown, as F3.
        shown: (options.overlay
            || std::env::var_os("RESONANCE_PERFORMANCE_OVERLAY").is_some_and(|v| v != "0"))
            && !headless,
        file,
        dump_directory: options
            .dump
            .as_deref()
            .and_then(Path::parent)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("local/performance"))
            .to_owned(),
        last_flush: now,
        notice: "",
        notice_until: None,
    })
    .add_systems(First, begin_frame)
    .add_systems(
        Last,
        end_frame
            .after(crate::materials::SurfacePublication)
            .after(crate::native::Compile),
    );
    if !headless {
        app.add_systems(Startup, overlay::setup)
            .add_systems(PreUpdate, controls.in_set(crate::input::HostActions))
            .add_systems(Update, overlay::update);
    }
    Ok(())
}

#[derive(bevy::ecs::system::SystemParam)]
struct FrameMetrics<'w> {
    sampled: Option<Res<'w, crate::scene::SampledImages>>,
    images: Option<Res<'w, Assets<Image>>>,
    meshes: Option<Res<'w, Assets<Mesh>>>,
    surfaces: Option<Res<'w, Assets<crate::materials::TitleSurface>>>,
    world_surfaces: Option<Res<'w, Assets<crate::materials::WorldSurface>>>,
    movie: Option<Res<'w, crate::movie::Playback>>,
    profile: Option<Res<'w, render_profile::Shared>>,
    gpu_profile: Option<Res<'w, gpu_profile::Shared>>,
    prediction: Option<Res<'w, crate::field_view::Prediction>>,
    testing: Option<Res<'w, crate::testing::Controls>>,
    native: Option<Res<'w, crate::native::Stats>>,
    diagnostics: Option<Res<'w, bevy::diagnostic::DiagnosticsStore>>,
    transforms: Option<Res<'w, crate::renderer::TransformStats>>,
    world_residency: Option<Res<'w, crate::overworld::ResidencyStats>>,
}

type ProfileCamera = Or<(
    With<crate::FieldCamera>,
    With<crate::FieldOverlayCamera>,
    With<Camera3d>,
)>;

#[allow(clippy::too_many_arguments)]
fn begin_frame(
    (_, mut monitor): (bevy::ecs::system::NonSendMarker, ResMut<Monitor>),
    display: Option<Res<crate::display::Display>>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    session: Option<Res<crate::new_game::Session>>,
    battle: Option<Res<crate::battle::Owner>>,
    pending: Option<Res<crate::loading::Pending>>,
    resident: Option<Res<crate::loading::Resident>>,
    metrics: FrameMetrics,
    field_cameras: Query<(&Camera, &Msaa), ProfileCamera>,
) {
    let now = Instant::now();
    monitor.cpu_started = if metrics.profile.is_some() {
        thread_cpu::now()
    } else {
        None
    };
    let previous = monitor.frame_started.replace(now);
    let Some(previous) = previous else {
        return;
    };
    monitor.frame += 1;
    let sample = Sample {
        frame: monitor.frame,
        elapsed_seconds: now.duration_since(monitor.started).as_secs_f64(),
        frame_ms: now.duration_since(previous).as_secs_f64() * 1000.,
        app_ms: monitor.app_ms,
    };
    monitor.history.push(sample);
    let flush = now.duration_since(monitor.last_flush).as_secs_f64() >= 1.;
    // Sort at most once per second for the stream; never sort once per frame.
    let summary = (flush && monitor.file.is_some()).then(|| monitor.history.summary());
    let app_cpu_ms = monitor.app_cpu_ms;
    let result = (|| -> Result<()> {
        if let Some(writer) = &mut monitor.file {
            let sampler_stats = metrics.sampled.as_ref().map(|sampled| sampled.stats());
            line(
                writer,
                &serde_json::json!({"kind":"frame", "sample":sample,
                    "app_thread_cpu_ms":app_cpu_ms,
                    "render_profile":metrics.profile.as_ref().map(|p| p.snapshot()),
                    "gpu_render_profile":metrics.gpu_profile.as_ref().map(|p| p.take()),
                    "smoothing_ns":smoothing_profile::take(),
                    "prediction_built_reused":metrics.prediction.as_ref().map(|p| p.counts()),
                    "transform_work":metrics.profile.as_ref().and(metrics.transforms.as_ref()).map(|s| &**s),
                    "world_residency":metrics.profile.as_ref().and(metrics.world_residency.as_ref()).map(|s| &**s),
                    "game_speed":metrics.testing.as_ref().map_or(1., |t| t.speed.factor()),
                    "render_diagnostics":metrics.profile.as_ref().and(metrics.diagnostics.as_ref()).map(|d| d.iter().filter_map(|v| v.value().map(|value| (v.path().as_str(),value))).collect::<std::collections::BTreeMap<_,_>>()),
                    "resolution":display.as_ref().map(|d| d.0),
                    "window_resolution":window.as_ref().map(|w| crate::Resolution { width: w.physical_width(), height: w.physical_height() }),
                    "active_scene_cameras":field_cameras.iter().filter(|(camera,_)| camera.is_active).count(),
                    "scene_camera_msaa_samples":field_cameras.iter().filter(|(camera,_)|camera.is_active).map(|(_,msaa)|msaa.samples()).collect::<Vec<_>>(),
                    "native_frames":metrics.native.as_ref().map(|n| serde_json::json!({
                        "rendered":n.frames, "native":n.native_frames,
                        "scene_sampling":n.particle_frames, "draws":n.draws,
                    })),
                    "model_particles":battle.as_ref().and_then(|owner| owner.model_particle_stats()),
                    "battle_workload":battle.as_ref().and_then(|owner| owner.benchmark_counts()),
                    "overworld":session.as_ref().and_then(|s|s.overworld()).map(|s|s.session.travel.state()),
                    "phase": if let Some(battle) = &battle { if battle.capture_ready() {"battle"} else {"battle_preparation"} } else if pending.is_some() || session.is_some() && resident.as_ref().is_some_and(|r| !r.active.load(std::sync::atomic::Ordering::Acquire)) {"field_preparation"} else if metrics.movie.as_ref().is_some_and(|m| m.active) {"movie"} else if session.is_some() {"field"} else {"title"},
                    "map":session.as_ref().map(|s| s.map_id()),
                    "tick":session.as_ref().map(|s| s.events().tick()),
                    "resources": {"images":metrics.images.as_ref().map(|a| a.len()), "meshes":metrics.meshes.as_ref().map(|a| a.len()), "surfaces":metrics.surfaces.as_ref().map(|a| a.len() + metrics.world_surfaces.as_ref().map_or(0, |w| w.len())),
                        "world_surfaces":metrics.world_surfaces.as_ref().map(|a| a.len()),
                        "sampler_hits":sampler_stats.map(|(hits, _)| hits), "sampler_misses":sampler_stats.map(|(_, misses)| misses),
                        "memory_reads":resident.as_ref().map(|r| r.memory_reads.load(std::sync::atomic::Ordering::Relaxed)),
                        "late_reads":resident.as_ref().map(|r| r.unprepared_reads.load(std::sync::atomic::Ordering::Relaxed))}}),
            )?;
            if let Some(summary) = summary {
                line(
                    writer,
                    &serde_json::json!({"kind":"summary", "elapsed_seconds":sample.elapsed_seconds, "summary":summary}),
                )?;
                writer.flush()?;
            }
        }
        Ok(())
    })();
    if flush {
        monitor.last_flush = now;
    }
    if let Err(error) = result {
        monitor.recording_failed(error);
    }
}

fn end_frame(
    _: bevy::ecs::system::NonSendMarker,
    mut monitor: ResMut<Monitor>,
    mut exit: MessageReader<AppExit>,
) {
    if let Some(start) = monitor.frame_started {
        monitor.app_ms = start.elapsed().as_secs_f64() * 1000.;
    }
    monitor.app_cpu_ms = monitor.cpu_started.and_then(|start| {
        thread_cpu::now().map(|end| thread_cpu::elapsed_ms(Some(start), Some(end)))
    });
    if exit.read().next().is_some() {
        let summary = monitor.history.summary();
        if let Some(writer) = &mut monitor.file {
            let result = line(
                writer,
                &serde_json::json!({"kind":"summary", "final":true, "summary":summary}),
            )
            .and_then(|()| writer.flush().map_err(Into::into));
            if let Err(error) = result {
                monitor.recording_failed(error);
            }
        }
    }
}

impl Monitor {
    fn recording_failed(&mut self, error: anyhow::Error) {
        error!("Performance recording stopped: {error:#}");
        self.file = None;
        self.notice = "DUMP ERROR - SEE LOG";
        self.notice_until = Some(Instant::now() + std::time::Duration::from_secs(10));
    }

    fn snapshot(&self) -> Result<PathBuf> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = self
            .dump_directory
            .join(format!("performance-{stamp}-{}.json", std::process::id()));
        let mut writer = create_file(&path)?;
        serde_json::to_writer_pretty(
            &mut writer,
            &serde_json::json!({
                "metadata":metadata(false),
                "summary":self.history.summary(),
                "frames":self.history.0,
            }),
        )?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        Ok(path)
    }
}

pub(crate) fn show(world: &mut World) {
    if let Some(mut monitor) = world.get_resource_mut::<Monitor>() {
        monitor.shown = true;
    }
}

fn controls(input: Res<crate::input::HostInput>, mut monitor: ResMut<Monitor>) {
    use resonance_content::input::StandardAction as A;
    if input.pressed(A::PerformanceOverlay) {
        monitor.shown = !monitor.shown;
    }
    if input.pressed(A::PerformanceSnapshot) {
        match monitor.snapshot() {
            Ok(path) => {
                info!("Performance snapshot saved: {}", path.display());
                monitor.notice = "SNAPSHOT SAVED";
            }
            Err(error) => {
                error!("Performance snapshot failed: {error:#}");
                monitor.notice = "SAVE ERROR - SEE LOG";
            }
        }
        monitor.notice_until = Some(Instant::now() + std::time::Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(app: &mut App, window: Entity, key_code: KeyCode, state: bevy::input::ButtonState) {
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code,
                logical_key: bevy::input::keyboard::Key::Character("test".into()),
                state,
                text: None,
                repeat: false,
                window,
            });
        app.update();
    }

    #[test]
    fn performance_controls_follow_remaps_and_explicit_unbindings() {
        use resonance_content::input::{PhysicalInput, StandardAction as A};
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .add_plugins(bevy::input::InputPlugin)
            .init_resource::<crate::config::Preferences>();
        let window = app
            .world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow))
            .id();
        crate::input::install(&mut app);
        install(&mut app, PerformanceOptions::default(), false).unwrap();
        app.world_mut()
            .resource_mut::<crate::config::Preferences>()
            .settings
            .keymap
            .assign(A::PerformanceOverlay, PhysicalInput::Key("KeyP".into()));
        key(
            &mut app,
            window,
            KeyCode::F3,
            bevy::input::ButtonState::Pressed,
        );
        assert!(!app.world().resource::<Monitor>().shown);
        key(
            &mut app,
            window,
            KeyCode::KeyP,
            bevy::input::ButtonState::Pressed,
        );
        assert!(app.world().resource::<Monitor>().shown);
        app.world_mut()
            .resource_mut::<crate::config::Preferences>()
            .settings
            .keymap
            .clear(A::PerformanceOverlay)
            .unwrap();
        key(
            &mut app,
            window,
            KeyCode::KeyP,
            bevy::input::ButtonState::Released,
        );
        key(
            &mut app,
            window,
            KeyCode::KeyP,
            bevy::input::ButtonState::Pressed,
        );
        assert!(app.world().resource::<Monitor>().shown);
    }

    #[test]
    fn hotkeys_toggle_the_overlay_and_save_a_readable_snapshot_without_devices() {
        let directory = std::env::temp_dir().join(format!(
            "resonance-perf-controls-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .add_plugins(bevy::input::InputPlugin)
            .init_resource::<crate::config::Preferences>();
        let window = app
            .world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow))
            .id();
        crate::input::install(&mut app);
        install(&mut app, PerformanceOptions::default(), false).unwrap();
        app.world_mut().resource_mut::<Monitor>().dump_directory = directory.clone();
        app.update();
        assert!(!app.world().resource::<Monitor>().shown);
        key(
            &mut app,
            window,
            KeyCode::F3,
            bevy::input::ButtonState::Pressed,
        );
        assert!(app.world().resource::<Monitor>().shown);
        key(
            &mut app,
            window,
            KeyCode::F4,
            bevy::input::ButtonState::Pressed,
        );
        let path = fs::read_dir(&directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let snapshot: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(snapshot["metadata"]["version"], 1);
        assert_eq!(snapshot["summary"]["samples"], 2);
        assert_eq!(snapshot["frames"].as_array().unwrap().len(), 2);
        assert!(
            app.world().resource::<Monitor>().shown,
            "saving must not toggle visibility"
        );
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::F3,
                logical_key: bevy::input::keyboard::Key::F3,
                state: bevy::input::ButtonState::Released,
                text: None,
                repeat: false,
                window,
            });
        key(
            &mut app,
            window,
            KeyCode::F3,
            bevy::input::ButtonState::Pressed,
        );
        assert!(!app.world().resource::<Monitor>().shown);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn continuous_dump_has_metadata_frames_and_final_summary_without_devices() {
        let path = std::env::temp_dir().join(format!(
            "resonance-perf-test-{}-{}.jsonl",
            std::process::id(),
            metadata(true).unix_seconds
        ));
        let mut app = App::new();
        install(
            &mut app,
            PerformanceOptions {
                overlay: false,
                dump: Some(path.clone()),
            },
            true,
        )
        .unwrap();
        assert!(create_file(&path).is_err(), "must not overwrite recordings");
        app.update();
        app.update();
        app.world_mut().write_message(AppExit::Success);
        app.update();
        let text = fs::read_to_string(&path).unwrap();
        let rows: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows[0]["kind"], "metadata");
        assert_eq!(rows[0]["headless"], true);
        assert_eq!(rows[1]["kind"], "frame");
        assert!(rows[1]["sample"]["frame_ms"].as_f64().unwrap() > 0.);
        assert_eq!(rows.last().unwrap()["final"], true);
        assert_eq!(rows.last().unwrap()["summary"]["samples"], 2);
        fs::remove_file(path).unwrap();
    }
}
