//! Finite, muted window-loop diagnostics using ordinary keyboard input systems.
use bevy::{
    ecs::system::NonSendMarker,
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
            script_root: None,
            saves: Default::default(),
            assets: root.into(),
            capture_at: None,
            capture: None,
            reveal: !compare_output,
            selected: 0,
            silent: true,
            paranoid: true,
            skip_intro: true,
            skip_battles: false,
            allow_incomplete_scripts: false,
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
    if !compare_output {
        crate::saves::install_scenario_input(
            &mut app,
            serde_json::from_str(include_str!("../../examples/scenarios/new-game.json"))?,
        )?;
        app.add_systems(PostUpdate, measure_field);
    }
    app.insert_resource(Probe {
        started: Instant::now(),
        field: FieldWorkload::default(),
        size: resolution,
        resize: 0,
        source: None,
        output: output.into(),
        screenshots: 0,
        capture_pending: false,
        minimized: None,
        compare_output,
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
    field: FieldWorkload,
    size: crate::Resolution,
    resize: u8,
    source: Option<AssetId<Image>>,
    output: PathBuf,
    screenshots: u8,
    capture_pending: bool,
    minimized: Option<Instant>,
    compare_output: bool,
    benchmark: Option<(u64, bool)>,
    completed: Arc<AtomicBool>,
}
impl Probe {
    fn captures_complete(&self) -> bool {
        self.benchmark.is_some()
            || (self.resize == 4
                && self.minimized.is_some()
                && self.screenshots == 4
                && !self.capture_pending)
    }
}

/// Counts only successive eligible game updates, independently of render frequency.
#[derive(Default)]
struct FieldWorkload {
    started: Option<Instant>,
    previous_tick: Option<u32>,
    updates: u64,
}
impl FieldWorkload {
    fn observe(&mut self, tick: Option<u32>) {
        if let Some(tick) = tick {
            self.started.get_or_insert_with(Instant::now);
            if let Some(previous) = self.previous_tick {
                self.updates += u64::from(tick.saturating_sub(previous));
            }
        }
        self.previous_tick = tick;
    }
    fn seconds(&self) -> f64 {
        self.updates as f64 / resonance_game::clock::UPDATE_HZ
    }
    fn complete(&self, seconds: u64) -> bool {
        self.previous_tick.is_some()
            && u128::from(self.updates) * u128::from(resonance_game::clock::UPDATE_RATE_DENOMINATOR)
                >= u128::from(seconds) * u128::from(resonance_game::clock::UPDATE_RATE_NUMERATOR)
    }
}
fn measure_field(world: &mut World) {
    let ready = world.resource::<crate::saves::ScenarioInput>().complete()
        && crate::field_view::ready(world)
        && world
            .resource::<crate::loading::Resident>()
            .active
            .load(Ordering::Acquire)
        && !world.resource::<crate::movie::Playback>().active
        && !world.contains_resource::<crate::battle::Owner>();
    let tick = ready
        .then(|| world.get_resource::<crate::new_game::Session>())
        .flatten()
        .filter(|session| {
            session.is_field()
                && session.ready_for_field
                && session.map_id() == 340
                && session.field().player_has_control()
        })
        .map(|session| session.field().events.tick());
    world.resource_mut::<Probe>().field.observe(tick);
}

#[allow(clippy::too_many_arguments)] // Diagnostic input, field state, window and finite lifetime.
fn drive(
    mut commands: Commands,
    mut probe: ResMut<Probe>,
    mut window: Single<(Entity, &mut Window), With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
    display: Res<crate::display::Display>,
    targets: Res<crate::display::Targets>,
    images: Res<Assets<Image>>,
    mut time: ResMut<Time<Virtual>>,
    cameras: Query<
        (Entity, &Camera, &Projection, &bevy::camera::RenderTarget),
        With<crate::FieldCamera>,
    >,
    output_cameras: Query<
        (&Camera, &bevy::camera::RenderTarget),
        With<crate::display::OutputCamera>,
    >,
    _main_thread: NonSendMarker,
) {
    let (entity, window) = &mut *window;
    let source = *probe.source.get_or_insert(targets.source.id());
    let camera = cameras
        .single()
        .ok()
        .filter(|(_, camera, projection, target)| {
            scene_camera(camera, projection, target, &targets.source, probe.size)
        })
        .map(|(entity, ..)| entity);
    let stable = display.0 == probe.size
        && targets.source.id() == source
        && images.get(&targets.source).is_some_and(|image| {
            image.size() == UVec2::new(probe.size.width, probe.size.scene_height())
        })
        && camera.is_some()
        && output_cameras.iter().any(|(camera, target)| {
            camera.is_active && matches!(target, bevy::camera::RenderTarget::Window(_))
        });
    let forced_resize = probe.benchmark.is_none() && (1..=3).contains(&probe.resize);
    if !stable || window.resizable != forced_resize || window.enabled_buttons.maximize {
        error!("Fixed startup resolution/window policy changed during the probe");
        exit.write(AppExit::error());
        return;
    }
    let timeout = if probe.compare_output {
        180
    } else {
        probe.benchmark.map_or(700, |(s, _)| s.saturating_add(650))
    };
    if probe.started.elapsed().as_secs() > timeout {
        error!(
            "Window probe timed out: resize stage {}, completed screenshots {}, pending {}",
            probe.resize, probe.screenshots, probe.capture_pending
        );
        exit.write(AppExit::error());
        return;
    }
    if probe.compare_output {
        return;
    }
    let Some(actual) = surface(*entity) else {
        return;
    };
    if probe.benchmark.is_some() && (actual.size != UVec2::new(probe.size.width, probe.size.height))
    {
        error!(
            "Benchmark requires {}x{}, actual window is {}x{}",
            probe.size.width, probe.size.height, actual.size.x, actual.size.y
        );
        exit.write(AppExit::error());
        return;
    }
    let capture = (probe.benchmark.is_none() && probe.field.complete(34) && !probe.capture_pending)
        .then(|| next_capture(probe.resize, probe.screenshots, probe.size, actual))
        .flatten();
    if let Some(name) = capture {
        let camera = camera.unwrap();
        let path = probe.output.join(format!("{name}.png"));
        let index = probe.screenshots;
        let source = targets.source.clone();
        probe.capture_pending = true;
        commands.spawn(Screenshot::primary_window()).observe(
                move |event: On<ScreenshotCaptured>, mut probe: ResMut<Probe>, targets: Res<crate::display::Targets>, cameras: Query<(&Camera, &Projection, &bevy::camera::RenderTarget), With<crate::FieldCamera>>, mut exit: MessageWriter<AppExit>| {
                    let result = (|| -> anyhow::Result<()> {
                        let content = field_content(&event.image, actual.size);
                        crate::screenshot::write(&event.image, &path, Some(&serde_json::json!({
                            "milestone":name, "observed_width":actual.size.x, "observed_height":actual.size.y,
                            "readback_completed":true, "scene_camera":camera.to_bits(),
                            "scene_region_coverage":content.as_ref().ok().map(|(coverage, _)| coverage),
                            "scene_channel_variation":content.as_ref().ok().map(|(_, variation)| variation),
                            "content_error":content.as_ref().err().map(|error| error.to_string()),
                        })))?;
                        anyhow::ensure!(probe.capture_pending && probe.screenshots == index, "unexpected probe screenshot callback");
                        anyhow::ensure!(targets.source == source && cameras.get(camera).is_ok_and(|(camera, projection, target)| scene_camera(camera, projection, target, &source, probe.size)), "scene camera/target changed before {name} readback");
                        content?;
                        Ok(())
                    })();
                    if let Err(error) = result {
                        error!("Fixed-resolution probe screenshot failed: {error:#}");
                        exit.write(AppExit::error());
                    } else {
                        probe.screenshots += 1;
                        probe.capture_pending = false;
                    }
                },
            );
    }
    if probe.benchmark.is_none() {
        if probe.resize == 0 && probe.screenshots == 1 {
            // Diagnostic-only: production keeps the startup window policy fixed.
            window.resizable = true;
            window.resize_constraints = default();
            window.resolution.set_physical_resolution(900, 900);
            probe.resize = 1;
            info!("Window probe: requested 900x900; awaiting observed surface and readback");
        } else if probe.resize == 1 && probe.screenshots == 2 {
            window.resolution.set_physical_resolution(1200, 500);
            probe.resize = 2;
            info!("Window probe: requested 1200x500; awaiting observed surface and readback");
        } else if probe.resize == 2 && probe.screenshots == 3 {
            window.set_minimized(true);
            probe.resize = 3;
            info!("Window probe: requested minimize; awaiting window-system confirmation");
        } else if probe.resize == 3 {
            match actual.minimized {
                None => {
                    error!(
                        "Window system cannot report minimization; resize probe cannot verify its workload"
                    );
                    exit.write(AppExit::error());
                }
                Some(true) => {
                    if probe
                        .minimized
                        .get_or_insert_with(Instant::now)
                        .elapsed()
                        .as_secs()
                        >= 2
                    {
                        window.set_minimized(false);
                        window.resizable = false;
                        window.resize_constraints =
                            crate::display::window(probe.size).resize_constraints;
                        window
                            .resolution
                            .set_physical_resolution(probe.size.width, probe.size.height);
                        probe.resize = 4;
                        info!("Window probe: observed minimize, requested restore at startup size");
                    }
                }
                Some(false) => {}
            }
        }
    }
    let seconds = probe.benchmark.map_or(50, |(s, _)| s);
    if probe.field.complete(seconds) && probe.captures_complete() && !probe.capture_pending {
        let camera = camera.unwrap();
        let source = targets.source.clone();
        let expected = UVec2::new(probe.size.width, probe.size.scene_height());
        // Screenshot routes these cameras into a fresh attachment. Its completed
        // pixels must qualify, never an earlier good image retained by the source.
        time.pause();
        probe.capture_pending = true;
        commands.spawn(Screenshot::image(source.clone())).observe(
            move |event: On<ScreenshotCaptured>, probe: Res<Probe>, targets: Res<crate::display::Targets>, cameras: Query<(&Camera, &Projection, &bevy::camera::RenderTarget), With<crate::FieldCamera>>, mut monitor: ResMut<super::Monitor>, mut exit: MessageWriter<AppExit>| {
                let result = (|| -> anyhow::Result<()> {
                    crate::screenshot::write(&event.image, &probe.output.join("source.png"), Some(&serde_json::json!({
                        "scene_camera":camera.to_bits(), "requested_width":expected.x, "requested_height":expected.y,
                        "fresh_source_attachment":true, "readback_completed":true,
                    })))?;
                    anyhow::ensure!(targets.source == source && cameras.get(camera).is_ok_and(|(camera, projection, target)| scene_camera(camera, projection, target, &source, probe.size)), "scene camera/target changed during final readback");
                    anyhow::ensure!(probe.field.complete(seconds), "field workload lost player control before final readback");
                    let (coverage, variation) = field_content(&event.image, expected)?;
                    monitor.finish_recording()?;
                    std::fs::write(probe.output.join("workload.json"), serde_json::to_vec_pretty(&serde_json::json!({
                        "map_id":340, "player_control":true, "source_readback_completed":true,
                        "scene_region_coverage":coverage, "scene_channel_variation":variation,
                        "scenario_complete":true, "field_updates":probe.field.updates,
                        "field_seconds":probe.field.seconds(), "requested_field_seconds":seconds,
                        "field_wall_seconds":probe.field.started.map(|started| started.elapsed().as_secs_f64()),
                        "total_wall_seconds":probe.started.elapsed().as_secs_f64(),
                        "completed_screenshots":probe.screenshots,
                    }))?)?;
                    Ok(())
                })();
                match result {
                    Ok(()) => { probe.completed.store(true, Ordering::Release); exit.write(AppExit::Success); }
                    Err(error) => { error!("Window probe final source validation failed: {error:#}"); exit.write(AppExit::error()); }
                }
            },
        );
    }
}

pub(super) fn scene_camera(
    camera: &Camera,
    projection: &Projection,
    target: &bevy::camera::RenderTarget,
    source: &Handle<Image>,
    size: crate::Resolution,
) -> bool {
    camera.is_active
        && matches!(target, bevy::camera::RenderTarget::Image(image) if &image.handle == source)
        && matches!(projection, Projection::Custom(p) if p.get::<crate::camera::TitleProjection>()
            .is_some_and(|p| (p.0.aspect_ratio - size.aspect()).abs() < 0.00001))
}

fn field_content(image: &Image, expected: UVec2) -> anyhow::Result<(f64, u8)> {
    const CROP_MARGIN_DENOMINATOR: u32 = 4;
    const DARK_CHANNEL: u8 = 16;
    const MIN_COVERAGE: f64 = 0.2;
    const MIN_VARIATION: u8 = 24;
    anyhow::ensure!(
        image.size() == expected,
        "source readback expected {expected:?}, got {:?}",
        image.size()
    );
    let pixels = image.clone().try_into_dynamic()?.to_rgba8();
    let mut count = 0;
    let mut visible = 0;
    let mut low = [255; 3];
    let mut high = [0; 3];
    // The central half excludes peripheral HUD. Require substantial scene coverage
    // and color variation; a clear or a few isolated sprites cannot pass.
    let margin = expected / CROP_MARGIN_DENOMINATOR;
    for y in margin.y..expected.y - margin.y {
        for x in margin.x..expected.x - margin.x {
            count += 1;
            let pixel = pixels.get_pixel(x, y).0;
            visible += usize::from(pixel[..3].iter().any(|&value| value > DARK_CHANNEL));
            for channel in 0..3 {
                low[channel] = low[channel].min(pixel[channel]);
                high[channel] = high[channel].max(pixel[channel]);
            }
        }
    }
    let coverage = visible as f64 / count.max(1) as f64;
    let variation = (0..3)
        .map(|channel| high[channel].saturating_sub(low[channel]))
        .max()
        .unwrap();
    anyhow::ensure!(
        coverage >= MIN_COVERAGE && variation >= MIN_VARIATION,
        "source lacks visible field content: central coverage {coverage:.3}, channel variation {variation}"
    );
    Ok((coverage, variation))
}

/// Read the native window, not the mutable ECS resize request. Call on the main thread.
#[derive(Clone, Copy)]
pub(super) struct Surface {
    pub(super) size: UVec2,
    pub(super) minimized: Option<bool>,
}
pub(super) fn surface(entity: Entity) -> Option<Surface> {
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        windows.get_window(entity).map(|window| {
            let size = window.inner_size();
            Surface {
                size: UVec2::new(size.width, size.height),
                minimized: window.is_minimized(),
            }
        })
    })
}

fn next_capture(
    stage: u8,
    completed: u8,
    size: crate::Resolution,
    actual: Surface,
) -> Option<&'static str> {
    let (name, expected) = match (stage, completed) {
        (0, 0) => ("before", UVec2::new(size.width, size.height)),
        (1, 1) => ("square", UVec2::new(900, 900)),
        (2, 2) => ("wide", UVec2::new(1200, 500)),
        (4, 3) if actual.minimized == Some(false) => {
            ("restored", UVec2::new(size.width, size.height))
        }
        _ => return None,
    };
    (actual.size == expected && actual.minimized != Some(true)).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_source_readback_rejects_clear_hud_only_and_wrong_dimensions() {
        use bevy::render::render_resource::TextureFormat;
        let size = UVec2::splat(32);
        let image = |pixel: fn(u32, u32) -> [u8; 4]| {
            let mut image =
                Image::new_target_texture(size.x, size.y, TextureFormat::Bgra8Unorm, None);
            image.data = Some(
                (0..size.y)
                    .flat_map(|y| (0..size.x).flat_map(move |x| pixel(x, y)))
                    .collect(),
            );
            image
        };
        let retained_good = image(|x, y| [32 + x as u8 * 4, 32 + y as u8 * 4, 64, 255]);
        assert!(field_content(&retained_good, size).is_ok());
        assert!(field_content(&retained_good, UVec2::splat(64)).is_err());
        // A stopped scene can retain an earlier good source. Screenshot's new
        // attachment is still clear, and that actual callback image must fail.
        let fresh_clear = image(|_, _| [0, 0, 0, 255]);
        assert!(field_content(&fresh_clear, size).is_err());
        let uniform_clear = image(|_, _| [80, 100, 120, 255]);
        assert!(field_content(&uniform_clear, size).is_err());
        let hud_only = image(|x, y| {
            if !(8..24).contains(&y) {
                [32 + x as u8 * 4, 80, 120, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        assert!(field_content(&hud_only, size).is_err());
        let isolated_sprite = image(|x, y| {
            if (14..18).contains(&x) && (14..18).contains(&y) {
                [200, 160, 80, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        assert!(field_content(&isolated_sprite, size).is_err());
    }

    #[test]
    fn gameplay_workload_requires_active_progress_not_repeated_or_paused_frames() {
        let mut field = FieldWorkload::default();
        field.observe(None);
        assert!(!field.complete(30));
        field.observe(Some(100));
        for _ in 0..3 {
            field.observe(Some(100));
        }
        assert_eq!(field.updates, 0);
        field.observe(Some(110));
        assert_eq!(field.updates, 10);
        field.observe(None);
        field.observe(Some(1000));
        assert_eq!(field.updates, 10, "unavailable time is not active progress");
        field.observe(Some(1010));
        assert_eq!(field.updates, 20);
        assert!(!field.complete(1));
        field.observe(Some(1050));
        assert!(field.complete(1));
        field.observe(None);
        assert!(
            !field.complete(1),
            "completion requires current player control"
        );
    }

    #[test]
    fn resize_captures_require_observed_dimensions_and_prior_callbacks() {
        let size = crate::Resolution {
            width: 640,
            height: 480,
        };
        let observed = |width, height| Surface {
            size: UVec2::new(width, height),
            minimized: Some(false),
        };
        assert_eq!(next_capture(0, 0, size, observed(640, 480)), Some("before"));
        // A resize request alone, or a missing prior screenshot callback, proves nothing.
        assert_eq!(next_capture(1, 1, size, observed(640, 480)), None);
        assert_eq!(next_capture(1, 0, size, observed(900, 900)), None);
        assert_eq!(next_capture(1, 1, size, observed(900, 900)), Some("square"));
        assert_eq!(next_capture(2, 2, size, observed(900, 900)), None);
        assert_eq!(next_capture(2, 2, size, observed(1200, 500)), Some("wide"));
        for minimized in [None, Some(true)] {
            assert_eq!(
                next_capture(
                    4,
                    3,
                    size,
                    Surface {
                        minimized,
                        ..observed(640, 480)
                    }
                ),
                None
            );
        }
        assert_eq!(
            next_capture(4, 3, size, observed(640, 480)),
            Some("restored")
        );
        assert_eq!(next_capture(4, 4, size, observed(640, 480)), None);
    }

    #[test]
    fn completion_requires_minimize_restore_and_final_readback() {
        let mut probe = Probe {
            started: Instant::now(),
            field: FieldWorkload::default(),
            size: Default::default(),
            resize: 4,
            source: None,
            output: PathBuf::new(),
            screenshots: 3,
            capture_pending: true,
            minimized: Some(Instant::now()),
            compare_output: false,
            benchmark: None,
            completed: Arc::default(),
        };
        assert!(!probe.captures_complete());
        probe.screenshots = 4;
        assert!(!probe.captures_complete());
        probe.capture_pending = false;
        assert!(probe.captures_complete());
        probe.minimized = None;
        assert!(!probe.captures_complete());
        probe.minimized = Some(Instant::now());
        probe.resize = 3;
        assert!(!probe.captures_complete());
    }
}
