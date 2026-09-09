//! Optional wall-clock diagnostics. No game-clock changes or GPU synchronization.
mod movie_probe;
mod overlay;
pub use movie_probe::run_movie_probe;
mod render_profile;
mod stats;
mod window_output;
mod window_probe;
pub use window_probe::{run_frame_benchmark, run_window_probe};

use anyhow::{Context, Result};
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
    /// Initially show the live overlay. F3 toggles it while playing.
    pub overlay: bool,
    /// Continuously record frame samples and periodic rolling summaries as JSONL.
    pub dump: Option<PathBuf>,
}

#[derive(Resource)]
struct Monitor {
    started: Instant,
    frame_started: Option<Instant>,
    app_ms: f64,
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
    let mut file = options.dump.as_deref().map(create_file).transpose()?;
    if let Some(writer) = &mut file {
        line(writer, &metadata(headless))?;
        writer.flush()?;
    }
    app.insert_resource(Monitor {
        started: now,
        frame_started: None,
        app_ms: 0.,
        frame: 0,
        history: History::default(),
        shown: options.overlay && !headless,
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
    .add_systems(Last, end_frame);
    if !headless {
        app.add_systems(Startup, overlay::setup)
            .add_systems(PreUpdate, controls.after(bevy::input::InputSystems))
            .add_systems(Update, overlay::update);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn begin_frame(
    mut monitor: ResMut<Monitor>,
    display: Option<Res<crate::display::Display>>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    session: Option<Res<crate::new_game::Session>>,
    pending: Option<Res<crate::loading::Pending>>,
    resident: Option<Res<crate::loading::Resident>>,
    sampled: Option<Res<crate::scene::SampledImages>>,
    images: Option<Res<Assets<Image>>>,
    meshes: Option<Res<Assets<Mesh>>>,
    surfaces: Option<Res<Assets<crate::materials::TitleSurface>>>,
    movie: Option<Res<crate::movie::Playback>>,
    profile: Option<Res<render_profile::Shared>>,
    diagnostics: Option<Res<bevy::diagnostic::DiagnosticsStore>>,
) {
    let now = Instant::now();
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
    let result = (|| -> Result<()> {
        if let Some(writer) = &mut monitor.file {
            line(
                writer,
                &serde_json::json!({"kind":"frame", "sample":sample,
                    "render_profile":profile.as_ref().map(|p| p.snapshot()),
                    "render_diagnostics":profile.as_ref().and(diagnostics.as_ref()).map(|d| d.iter().filter_map(|v| v.value().map(|value| (v.path().as_str(),value))).collect::<std::collections::BTreeMap<_,_>>()),
                    "resolution":display.as_ref().map(|d| d.0),
                    "window_resolution":window.as_ref().map(|w| crate::Resolution { width: w.physical_width(), height: w.physical_height() }),
                    "phase": if pending.is_some() || session.is_some() && resident.as_ref().is_some_and(|r| !r.active.load(std::sync::atomic::Ordering::Acquire)) {"field_preparation"} else if movie.as_ref().is_some_and(|m| m.active) {"movie"} else if session.is_some() {"field"} else {"title"},
                    "map":session.as_ref().map(|s| s.assets.map_id),
                    "tick":session.as_ref().map(|s| s.field.events.tick()),
                    "resources": {"images":images.as_ref().map(|a| a.len()), "meshes":meshes.as_ref().map(|a| a.len()), "surfaces":surfaces.as_ref().map(|a| a.len()),
                        "sampler_hits":sampled.as_ref().map(|s| s.hits), "sampler_misses":sampled.as_ref().map(|s| s.misses),
                        "memory_reads":resident.as_ref().map(|r| r.memory_reads.load(std::sync::atomic::Ordering::Relaxed)),
                        "late_reads":resident.as_ref().map(|r| r.late_reads.load(std::sync::atomic::Ordering::Relaxed))}}),
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

fn end_frame(mut monitor: ResMut<Monitor>, mut exit: MessageReader<AppExit>) {
    if let Some(start) = monitor.frame_started {
        monitor.app_ms = start.elapsed().as_secs_f64() * 1000.;
    }
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

fn controls(keys: Res<ButtonInput<KeyCode>>, mut monitor: ResMut<Monitor>) {
    if keys.just_pressed(KeyCode::F3) {
        monitor.shown = !monitor.shown;
    }
    if keys.just_pressed(KeyCode::F4) {
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
            .init_resource::<ButtonInput<KeyCode>>();
        install(&mut app, PerformanceOptions::default(), false).unwrap();
        app.world_mut().resource_mut::<Monitor>().dump_directory = directory.clone();
        app.update();
        assert!(!app.world().resource::<Monitor>().shown);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F3);
        app.update();
        assert!(app.world().resource::<Monitor>().shown);
        let camera = app
            .world_mut()
            .query_filtered::<&Camera, With<overlay::OverlayCamera>>()
            .single(app.world())
            .unwrap();
        assert!(camera.is_active);
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::F4);
        app.update();
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
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::F3);
        app.update();
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
