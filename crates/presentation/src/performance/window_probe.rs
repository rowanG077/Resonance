//! Finite, muted window-loop diagnostics using ordinary keyboard input systems.
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    window::PrimaryWindow,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub fn run_window_probe(
    root: &Path,
    output: &Path,
    resolution: crate::Resolution,
) -> anyhow::Result<()> {
    run(root, output, resolution, None)
}

/// Measure a constant physical resolution through the live classroom sequence.
/// The window manager must honor the requested size; a mismatch fails the run.
pub fn run_frame_benchmark(
    root: &Path,
    output: &Path,
    resolution: crate::Resolution,
    seconds: u64,
    profile: bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        seconds >= 30,
        "benchmark requires at least 30 field seconds"
    );
    run(root, output, resolution, Some((seconds, profile)))
}

fn run(
    root: &Path,
    output: &Path,
    resolution: crate::Resolution,
    benchmark: Option<(u64, bool)>,
) -> anyhow::Result<()> {
    anyhow::ensure!(!output.exists(), "window probe output already exists");
    std::fs::create_dir_all(output)?;
    let compare_output =
        benchmark.is_some() && std::env::var_os("RESONANCE_BENCH_COMPARE_OUTPUT").is_some();
    // A diagnostic surface can start with a different aspect even when the
    // compositor ignores later resize requests. Game rendering stays unchanged.
    let surface_size: Option<crate::Resolution> = std::env::var("RESONANCE_BENCH_WINDOW_SIZE")
        .ok()
        .map(|s| s.parse().map_err(anyhow::Error::msg))
        .transpose()?;
    anyhow::ensure!(
        surface_size.is_none() || compare_output,
        "RESONANCE_BENCH_WINDOW_SIZE requires the paired output diagnostic"
    );
    std::fs::write(
        output.join("probe.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "resolution":resolution, "field_seconds":benchmark.map_or(50, |(seconds,_)|seconds),
            "profile":benchmark.is_some_and(|(_,p)|p), "scheduler":"player", "no_clusters":true, "direct_output":true,
            "pid":std::process::id(), "silent":true, "fixed_render_resolution":true,
            "forced_window_resize":benchmark.is_none(), "compare_output":compare_output,
            "requested_window_resolution":surface_size.unwrap_or(resolution),
        }))?,
    )?;
    let (mut app, _) = crate::build_app_with_display(
        crate::RunOptions {
            saves: Default::default(),
            assets: root.into(),
            tick: compare_output.then_some(920),
            presentation_start: None,
            capture: None,
            reveal: true,
            selected: 0,
            silent: true,
            replay: None,
            movie_frame: None,
            boot_frame: None,
            skip_intro: true,
            record_playthrough: None,
            record_title_ticks: 1000,
        },
        resolution,
    )?;
    if let Some(size) = surface_size {
        let mut window = app
            .world_mut()
            .query_filtered::<&mut Window, With<PrimaryWindow>>()
            .single_mut(app.world_mut())?;
        *window = crate::display::window(size);
    }
    if let Some((_, profile)) = benchmark {
        app.add_systems(
            Startup,
            |mut window: Single<&mut Window, With<PrimaryWindow>>| {
                window.title = "Resonance performance probe".into();
            },
        );
        if profile {
            super::render_profile::install(&mut app);
        }
    }
    super::install(
        &mut app,
        super::PerformanceOptions {
            overlay: false,
            dump: Some(output.join("frames.jsonl")),
        },
        false,
    )?;
    let completed = Arc::new(AtomicBool::new(false));
    if compare_output {
        super::window_output::install(
            &mut app,
            output,
            resolution,
            surface_size.is_some(),
            completed.clone(),
        );
    }
    app.insert_resource(Probe {
        started: Instant::now(),
        field: None,
        size: resolution,
        resize: 0,
        source: None,
        output: output.into(),
        screenshots: 0,
        benchmark,
        completed: completed.clone(),
    })
    .add_systems(
        PreUpdate,
        drive
            .after(bevy::input::InputSystems)
            .before(crate::gather_input)
            .before(crate::field_view::gather_controls),
    );
    anyhow::ensure!(app.run() == AppExit::Success, "window probe failed");
    anyhow::ensure!(
        completed.load(Ordering::Acquire),
        "window probe exited before completing its workload"
    );
    Ok(())
}

#[derive(Resource)]
struct Probe {
    started: Instant,
    field: Option<Instant>,
    size: crate::Resolution,
    resize: u8,
    source: Option<AssetId<Image>>,
    output: PathBuf,
    screenshots: u8,
    benchmark: Option<(u64, bool)>,
    completed: Arc<AtomicBool>,
}
#[allow(clippy::too_many_arguments)] // Diagnostic input, field state, window and finite lifetime.
fn drive(
    mut commands: Commands,
    mut probe: ResMut<Probe>,
    mut input: ResMut<ButtonInput<KeyCode>>,
    menu: Res<crate::Menu>,
    session: Option<Res<crate::new_game::Session>>,
    movie: Res<crate::movie::Playback>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
    display: Res<crate::display::Display>,
    targets: Res<crate::display::Targets>,
    images: Res<Assets<Image>>,
    projections: Query<(&Projection, &bevy::camera::RenderTarget)>,
) {
    input.reset_all();
    let source = *probe.source.get_or_insert(targets.source.id());
    let stable = display.0 == probe.size
        && targets.source.id() == source
        && images.get(&targets.source).is_some_and(|image| {
            image.size() == UVec2::new(probe.size.width, probe.size.scene_height())
        })
        && projections
            .iter()
            .filter(|(_, target)| {
                matches!(target,
            bevy::camera::RenderTarget::Image(image) if image.handle == targets.source)
            })
            .all(|(p, _)| match p {
                Projection::Custom(p) => p
                    .get::<crate::camera::TitleProjection>()
                    .is_none_or(|p| (p.0.aspect_ratio - probe.size.aspect()).abs() < 0.00001),
                _ => true,
            });
    let forced_resize = probe.benchmark.is_none() && (1..=3).contains(&probe.resize);
    if !stable || window.resizable != forced_resize || window.enabled_buttons.maximize {
        error!("Fixed startup resolution/window policy changed during the probe");
        exit.write(AppExit::error());
        return;
    }
    if probe.started.elapsed().as_secs() > probe.benchmark.map_or(180, |(s, _)| s + 150) {
        error!("Window probe timed out");
        exit.write(AppExit::error());
        return;
    }
    if movie.active {
        if movie.presented_frame.is_some_and(|frame| frame > 60) {
            input.press(KeyCode::Escape);
        }
        return;
    }
    let Some(session) = session else {
        if probe.started.elapsed().as_secs() >= 20 && menu.0.tick.is_multiple_of(30) {
            input.press(KeyCode::Enter);
        }
        return;
    };
    let field = &session.field;
    let tick = field.events.tick();
    if session.assets.map_id == 5 {
        if tick >= 360
            && let Some(choice) = field.events.world.choices.get(&1)
        {
            if choice.selected_line == 0 {
                input.press(KeyCode::ArrowDown);
            } else if tick >= 390 && tick.is_multiple_of(30) {
                input.press(KeyCode::Enter);
            }
        }
        return;
    }
    if !session.ready_for_field || movie.presented_frame.is_none() || tick <= 30 {
        return;
    }
    if probe.benchmark.is_some()
        && (window.physical_width() != probe.size.width
            || window.physical_height() != probe.size.height)
    {
        error!(
            "Benchmark requires {}x{}, actual window is {}x{}",
            probe.size.width,
            probe.size.height,
            window.physical_width(),
            window.physical_height()
        );
        exit.write(AppExit::error());
        return;
    }
    let elapsed = probe
        .field
        .get_or_insert_with(|| {
            info!("Window probe: classroom measurement started");
            Instant::now()
        })
        .elapsed()
        .as_secs();
    if tick.is_multiple_of(30)
        && field
            .dialogue
            .values()
            .any(|p| !p.closed && !p.persistent && p.fully_revealed() && p.voice_finished())
    {
        input.press(KeyCode::Enter);
    }
    if probe.benchmark.is_none() {
        let next = [
            (34, "before"),
            (37, "square"),
            (40, "wide"),
            (46, "restored"),
        ]
        .get(usize::from(probe.screenshots))
        .copied();
        if let Some((at, name)) = next
            && elapsed >= at
        {
            let path = probe.output.join(format!("{name}.png"));
            commands.spawn(Screenshot::primary_window()).observe(
                move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                    if let Err(error) = event
                        .image
                        .clone()
                        .try_into_dynamic()
                        .map_err(anyhow::Error::from)
                        .and_then(|image| image.save(&path).map_err(anyhow::Error::from))
                    {
                        error!("Fixed-resolution probe screenshot failed: {error:#}");
                        exit.write(AppExit::error());
                    }
                },
            );
            probe.screenshots += 1;
        }
    }
    if probe.benchmark.is_none() && elapsed >= 35 && probe.resize == 0 {
        // Diagnostic-only: allow the test window to change size to exercise
        // a compositor override. Production never relaxes this policy.
        window.resizable = true;
        window.resize_constraints = default();
        window.resolution.set_physical_resolution(900, 900);
        probe.resize = 1;
        info!("Window probe: imposed 900x900; game targets must stay fixed");
    } else if probe.benchmark.is_none() && elapsed >= 38 && probe.resize == 1 {
        window.resolution.set_physical_resolution(1200, 500);
        probe.resize = 2;
        info!("Window probe: imposed 1200x500; game targets must stay fixed");
    } else if probe.benchmark.is_none() && elapsed >= 41 && probe.resize == 2 {
        window.set_minimized(true);
        probe.resize = 3;
        info!("Window probe: requested minimize");
    } else if probe.benchmark.is_none() && elapsed >= 43 && probe.resize == 3 {
        window.set_minimized(false);
        window.resizable = false;
        window.resize_constraints = crate::display::window(probe.size).resize_constraints;
        window
            .resolution
            .set_physical_resolution(probe.size.width, probe.size.height);
        probe.resize = 4;
        info!("Window probe: requested restore at startup size");
    }
    if elapsed >= probe.benchmark.map_or(50, |(s, _)| s) {
        info!("Window probe complete");
        probe.completed.store(true, Ordering::Release);
        exit.write(AppExit::Success);
    }
}
