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
    run(root, output, resolution, Options::default())
}

/// The title screen without a window, for title shots and audits.
pub fn run_offscreen_title(
    root: &Path,
    output: &Path,
    resolution: crate::Resolution,
    anti_aliasing: resonance_content::menu_data::AntiAliasing,
) -> anyhow::Result<()> {
    run(
        root,
        output,
        resolution,
        Options {
            offscreen: true,
            anti_aliasing: Some(anti_aliasing),
            ..Options::default()
        },
    )
}

/// A saved gameplay workload for a windowless, wall-clock render benchmark.
pub struct ScenarioBenchmark<'a> {
    pub save: &'a Path,
    pub encounter: Option<resonance_events::battle::Setup>,
    pub replay: &'a crate::saves::CheckpointReplay,
    pub seconds: u64,
    pub profile: bool,
    pub anti_aliasing: resonance_content::menu_data::AntiAliasing,
}

pub fn run_offscreen_benchmark(
    root: &Path,
    output: &Path,
    resolution: crate::Resolution,
    scenario: ScenarioBenchmark<'_>,
) -> anyhow::Result<()> {
    run_scenario(root, output, resolution, scenario, true)
}

/// [`run_offscreen_benchmark`] in a window on the desktop, presenting every
/// frame through the swap chain.
pub fn run_window_benchmark(
    root: &Path,
    output: &Path,
    resolution: crate::Resolution,
    scenario: ScenarioBenchmark<'_>,
) -> anyhow::Result<()> {
    run_scenario(root, output, resolution, scenario, false)
}

fn run_scenario(
    root: &Path,
    output: &Path,
    resolution: crate::Resolution,
    scenario: ScenarioBenchmark<'_>,
    offscreen: bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        scenario.seconds >= 30,
        "benchmark requires at least 30 seconds"
    );
    run(
        root,
        output,
        resolution,
        Options {
            benchmark: Some((scenario.seconds, scenario.profile)),
            encounter: scenario.encounter.map(|setup| (scenario.save, setup)),
            save: Some(scenario.save),
            replay: Some(scenario.replay),
            offscreen,
            anti_aliasing: Some(scenario.anti_aliasing),
        },
    )
}

#[derive(Default)]
struct Options<'a> {
    benchmark: Option<(u64, bool)>,
    encounter: Option<(&'a Path, resonance_events::battle::Setup)>,
    replay: Option<&'a crate::saves::CheckpointReplay>,
    save: Option<&'a Path>,
    offscreen: bool,
    anti_aliasing: Option<resonance_content::menu_data::AntiAliasing>,
}

/// Measure a fixed render resolution through the live classroom or an encounter.
/// Record the compositor's physical output size and require it to remain stable.
pub fn run_frame_benchmark(
    root: &Path,
    output: &Path,
    resolution: crate::Resolution,
    seconds: u64,
    profile: bool,
    encounter: Option<(&Path, resonance_events::battle::Setup)>,
    replay: Option<&crate::saves::CheckpointReplay>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        seconds >= 30,
        "benchmark requires at least 30 field seconds"
    );
    run(
        root,
        output,
        resolution,
        Options {
            benchmark: Some((seconds, profile)),
            encounter,
            replay,
            ..Default::default()
        },
    )
}

fn run(
    root: &Path,
    output: &Path,
    resolution: crate::Resolution,
    options: Options<'_>,
) -> anyhow::Result<()> {
    let Options {
        benchmark,
        encounter,
        replay,
        save,
        offscreen,
        anti_aliasing,
    } = options;
    anyhow::ensure!(
        replay.is_none() || encounter.is_some() || save.is_some(),
        "replay requires a saved scene"
    );
    anyhow::ensure!(!output.exists(), "window probe output already exists");
    std::fs::create_dir_all(output)?;
    if let Some(anti_aliasing) = anti_aliasing {
        let settings = resonance_content::menu_data::CustomizeSettings {
            anti_aliasing,
            resolution: [resolution.width, resolution.height],
            vsync: false,
            learn_both_arte_paths: true,
            learn_elemental_artes: true,
            flexible_battle_party: true,
            skit_notifications: false,
            ..Default::default()
        };
        std::fs::create_dir_all(output.join("slots"))?;
        std::fs::write(
            output.join("slots/config.toml"),
            toml::to_string_pretty(&settings)?,
        )?;
    }
    std::fs::write(
        output.join("probe.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "resolution":resolution, "field_seconds":benchmark.map_or(50, |(seconds,_)|seconds),
            "profile":benchmark.is_some_and(|(_,p)|p), "scheduler":"player", "direct_output":true,
            "pass_diagnostics":benchmark.is_some_and(|(_,p)|p) && std::env::var_os("RESONANCE_PROBE_GPU_ONLY").is_none(),
            "pid":std::process::id(), "silent":true, "fixed_render_resolution":true,
            "forced_window_resize":benchmark.is_none(),
            "offscreen":offscreen, "anti_aliasing":anti_aliasing,
            "encounter":encounter.as_ref().map(|(save, setup)| serde_json::json!({"save":save,"setup":format!("{setup:?}")})),
            "republish_each_frame":std::env::var_os("RESONANCE_BENCH_REPUBLISH").is_some(),
            "replay":replay.is_some(),
        }))?,
    )?;
    let (mut app, _) = crate::build_app(
        crate::RunOptions {
            // `RESONANCE_BENCH_MODS=DIR[:DIR...]` enables mods, as the game's
            // `--mod`, so saves made with them load.
            mods: std::env::var_os("RESONANCE_BENCH_MODS")
                .map(|mods| std::env::split_paths(&mods).collect())
                .unwrap_or_default(),
            script_root: None,
            saves: crate::saves::SaveOptions {
                load: encounter
                    .as_ref()
                    .map(|(save, _)| save.to_path_buf())
                    .or_else(|| save.map(Path::to_path_buf)),
                directory: Some(output.join("slots")),
                ..Default::default()
            },
            assets: root.into(),
            capture_at: None,
            paranoid: true,
            presentation_start: None,
            capture: None,
            reveal: true,
            silent: true,
            replay: None,
            skip_intro: true,
            skip_battles: false,
            allow_incomplete_scripts: false,
            record_playthrough: None,
            record_title_ticks: 1000,
            speculative: std::env::var_os("RESONANCE_PROBE_SMOOTHING").is_some(),
        },
        if offscreen {
            crate::DisplaySource::Offscreen(resolution)
        } else {
            crate::DisplaySource::Fixed(resolution)
        },
    )?;
    if anti_aliasing.is_some() {
        let preferences = crate::config::Preferences::load(output.join("slots/config.toml"))?;
        app.insert_resource(crate::anti_aliasing::AntiAliasingSettings(
            preferences.settings.anti_aliasing,
        ));
        app.insert_resource(preferences);
    }
    let probe_path = output.join("probe.json");
    app.add_systems(
        Startup,
        move |device: Res<bevy::render::renderer::RenderDevice>| {
            let mut probe: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&probe_path).expect("probe metadata"))
                    .expect("probe metadata JSON");
            let limits = device.limits();
            probe["thread_cpu_clock"] = super::thread_cpu::now().is_some().into();
            probe["gpu_timestamp_slots"] = super::gpu_profile::SLOTS.into();
            probe["compute_threads"] = bevy::tasks::ComputeTaskPool::get().thread_num().into();
            probe["binding_limits"] = serde_json::json!({
                "texture_arrays": device.features().contains(wgpu::Features::TEXTURE_BINDING_ARRAY),
                "array_samplers_per_stage": limits.max_binding_array_sampler_elements_per_shader_stage,
                "array_elements_per_stage": limits.max_binding_array_elements_per_shader_stage,
                "storage_buffers_per_stage": limits.max_storage_buffers_per_shader_stage,
            });
            std::fs::write(&probe_path, serde_json::to_vec_pretty(&probe).unwrap())
                .expect("record material binding mode");
        },
    );
    app.add_systems(
        Startup,
        |options: Res<crate::RunOptions>,
         window: Option<Single<&mut Window, With<PrimaryWindow>>>| {
            if let Some(mut window) = window {
                window.title = format!(
                    "Resonance probe — smoothing {}",
                    if options.speculative { "ON" } else { "OFF" }
                );
            }
        },
    );
    let title_capture = std::env::var_os("RESONANCE_PROBE_TITLE_CAPTURE").is_some();
    let speed_probe = std::env::var_os("RESONANCE_PROBE_SPEEDS").is_some();
    if title_capture {
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_nanos(4_170_833),
        ));
        app.add_systems(Update, capture_title.after(crate::layout));
    }
    // `RESONANCE_PROBE_TITLE_SHOTS` saves the offscreen title every 60 ticks.
    let title_shots = std::env::var_os("RESONANCE_PROBE_TITLE_SHOTS").is_some();
    if title_shots {
        app.add_systems(Update, capture_title_shots.after(crate::layout));
    }
    let smoothing_profile = std::env::var_os("RESONANCE_PROBE_PROFILE").is_some();
    if smoothing_profile || benchmark.is_some_and(|(_, profile)| profile) {
        super::smoothing_profile::enable();
    }
    if smoothing_profile || benchmark.is_some_and(|(_, profile)| profile) {
        super::render_profile::install(
            &mut app,
            std::env::var_os("RESONANCE_PROBE_GPU_ONLY").is_none(),
        );
    }
    if benchmark.is_some() {
        app.add_systems(
            Startup,
            |window: Option<Single<&mut Window, With<PrimaryWindow>>>| {
                if let Some(mut window) = window {
                    window.title = "Resonance performance probe".into();
                }
            },
        );
        if std::env::var_os("RESONANCE_BENCH_REPUBLISH").is_some() {
            app.add_systems(Update, |battle: Option<ResMut<crate::battle::Owner>>| {
                if let Some(mut battle) = battle {
                    battle.republish();
                }
            });
        }
    }
    super::install(
        &mut app,
        super::PerformanceOptions {
            overlay: false,
            dump: Some(output.join("frames.jsonl")),
        },
        offscreen,
    )?;
    let completed = Arc::new(AtomicBool::new(false));
    if let Some(replay) = replay {
        crate::saves::install_benchmark_replay(&mut app, replay, output, completed.clone())?;
    }
    if let Some((_, setup)) = &encounter {
        let setup = setup.clone();
        let mut ready_since = None;
        let mut requested = false;
        // Effect replays begin at the saved scene, before its field script advances.
        // The ordinary entry benchmark retains its ten-second field baseline.
        let field_seconds = if replay.is_some() { 0 } else { 10 };
        app.add_systems(Update, move |world: &mut World| {
            if requested
                || !crate::new_game::scene_ready(world)
                || !world.contains_resource::<crate::field_audio::Control>()
            {
                return;
            }
            let mut session = world.resource_mut::<crate::new_game::Session>();
            if !session.is_field() || !session.field().player_has_control() {
                return;
            }
            if ready_since
                .get_or_insert_with(Instant::now)
                .elapsed()
                .as_secs()
                >= field_seconds
            {
                session
                    .events_mut()
                    .world
                    .request_battle(setup.clone())
                    .expect("benchmark encounter");
                requested = true;
            }
        });
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
        checkpoint: encounter.is_some() || save.is_some(),
        offscreen,
        replay: replay.is_some(),
        battle: None,
        measured_window: None,
        completed: completed.clone(),
        title_only: speed_probe
            || title_capture
            || title_shots
            || std::env::var_os("RESONANCE_PROBE_TITLE").is_some(),
        speed_step: speed_probe.then_some(0),
    })
    .add_systems(
        PreUpdate,
        drive
            .after(bevy::input::InputSystems)
            .before(crate::testing::controls)
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

fn capture_title(
    mut commands: Commands,
    probe: Res<Probe>,
    events: Option<Res<crate::Events>>,
    fixed: Res<Time<Fixed>>,
    mut captured: Local<u32>,
) {
    let Some(events) = events else {
        return;
    };
    let tick = events.0.tick();
    // Both title lighting atlases change row at tick 617. Capture the interval
    // around that boundary at four render samples per authoritative update.
    if !(614..=620).contains(&tick) {
        return;
    }
    let path = probe.output.join(format!(
        "title-{tick}-{:03}-{:03}.png",
        (fixed.overstep_fraction() * 1000.) as u32,
        *captured
    ));
    *captured += 1;
    commands
        .spawn(Screenshot::primary_window())
        .observe(move |event: On<ScreenshotCaptured>| {
            crate::screenshot::write(&event.image, &path, None)
                .expect("title diagnostic screenshot");
        });
}

fn capture_title_shots(
    mut commands: Commands,
    probe: Res<Probe>,
    events: Option<Res<crate::Events>>,
    framebuffer: Res<crate::Framebuffer>,
    mut last: Local<Option<u32>>,
) {
    let Some(events) = events else {
        return;
    };
    let tick = events.0.tick();
    if tick == 0 || !tick.is_multiple_of(60) || *last == Some(tick) {
        return;
    }
    *last = Some(tick);
    let bevy::camera::RenderTarget::Image(image) = &framebuffer.0 else {
        return;
    };
    let name = format!("shot{:04}", tick);
    let path = probe.output.join(format!("{name}.png"));
    commands
        .spawn(Screenshot::image(image.handle.clone()))
        .observe(move |event: On<ScreenshotCaptured>| {
            crate::screenshot::write(&event.image, &path, None).expect("title shot");
        });
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
    checkpoint: bool,
    offscreen: bool,
    replay: bool,
    battle: Option<Instant>,
    measured_window: Option<UVec2>,
    completed: Arc<AtomicBool>,
    title_only: bool,
    speed_step: Option<u8>,
}
#[allow(clippy::too_many_arguments)] // Diagnostic input, field state, window and finite lifetime.
fn drive(
    mut commands: Commands,
    mut probe: ResMut<Probe>,
    mut input: ResMut<ButtonInput<KeyCode>>,
    menu: Res<crate::Menu>,
    session: Option<Res<crate::new_game::Session>>,
    movie: Res<crate::movie::Playback>,
    window: Option<Single<&mut Window, With<PrimaryWindow>>>,
    mut exit: MessageWriter<AppExit>,
    display: Res<crate::display::Display>,
    targets: Res<crate::display::Targets>,
    images: Res<Assets<Image>>,
    projections: Query<(&Projection, &bevy::camera::RenderTarget)>,
    battle: Option<Res<crate::battle::Owner>>,
) {
    if !probe.replay {
        input.reset_all();
    }
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
    let window_valid = if probe.offscreen {
        window.is_none()
    } else {
        window.as_ref().is_some_and(|window| {
            window.resizable == forced_resize && !window.enabled_buttons.maximize
        })
    };
    let offscreen_valid = !probe.offscreen
        || targets.output.as_ref().is_some_and(|output| {
            images.get(output).is_some_and(|image| {
                image.size() == UVec2::new(probe.size.width, probe.size.height)
            })
        });
    if !stable || !window_valid || !offscreen_valid {
        error!("Fixed startup resolution/window policy changed during the probe");
        exit.write(AppExit::error());
        return;
    }
    if probe.started.elapsed().as_secs() > probe.benchmark.map_or(180, |(s, _)| s + 150) {
        error!("Window probe timed out");
        exit.write(AppExit::error());
        return;
    }
    if probe.title_only {
        if let Some(step) = probe.speed_step {
            let seconds = probe.started.elapsed().as_secs();
            if step < 4 && seconds >= 3 + u64::from(step) * 3 {
                input.press(KeyCode::F10);
                probe.speed_step = Some(step + 1);
            }
            if probe.screenshots < 4 && seconds >= 4 + u64::from(probe.screenshots) * 3 {
                let path = probe
                    .output
                    .join(format!("speed-{}.png", probe.screenshots));
                commands.spawn(Screenshot::primary_window()).observe(
                    move |event: On<ScreenshotCaptured>| {
                        crate::screenshot::write(&event.image, &path, None)
                            .expect("speed diagnostic screenshot");
                    },
                );
                probe.screenshots += 1;
            }
        }
        if probe.started.elapsed().as_secs() >= 20 {
            probe.completed.store(true, Ordering::Release);
            exit.write(AppExit::Success);
        }
        return;
    }
    if movie.active {
        if movie.presented_frame.is_some_and(|frame| frame > 60) {
            input.press(KeyCode::Escape);
        }
        return;
    }
    let window_size = window
        .as_ref()
        .map_or(UVec2::new(probe.size.width, probe.size.height), |window| {
            UVec2::new(window.physical_width(), window.physical_height())
        });
    if probe
        .measured_window
        .is_some_and(|size| size != window_size)
    {
        error!(
            "Benchmark physical window size changed to {}x{}",
            window_size.x, window_size.y
        );
        exit.write(AppExit::error());
        return;
    }
    if probe.replay {
        if session
            .as_ref()
            .is_some_and(|session| session.ready_for_field)
        {
            probe.measured_window.get_or_insert(window_size);
        }
        return;
    }
    if let Some(battle) = battle {
        if battle.presenting()
            && probe
                .battle
                .get_or_insert_with(Instant::now)
                .elapsed()
                .as_secs()
                >= probe.benchmark.map_or(50, |(seconds, _)| seconds)
        {
            probe.completed.store(true, Ordering::Release);
            exit.write(AppExit::Success);
        }
        return;
    }
    let Some(session) = session else {
        if probe.started.elapsed().as_secs() >= 20 && menu.0.tick.is_multiple_of(30) {
            input.press(KeyCode::Enter);
        }
        return;
    };
    if !session.is_field() {
        return;
    }
    let field = session.field();
    let tick = field.events.tick();
    if session.map_id() == 5 {
        if tick >= 360
            && let Some(choice) = field.events.world.choices.get(&1)
        {
            if choice
                .selection
                .lines()
                .is_some_and(|lines| lines.selected_line == 0)
            {
                input.press(KeyCode::ArrowDown);
            } else if tick >= 390 && tick.is_multiple_of(30) {
                input.press(KeyCode::Enter);
            }
        }
        return;
    }
    if !session.ready_for_field
        || (!probe.checkpoint && movie.presented_frame.is_none())
        || tick <= 30
    {
        return;
    }
    if probe.benchmark.is_some() {
        probe.measured_window.get_or_insert(window_size);
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
    if let Some((seconds, _)) = probe.benchmark {
        if !probe.checkpoint && elapsed >= seconds {
            probe.completed.store(true, Ordering::Release);
            exit.write(AppExit::Success);
        }
        return;
    }
    let Some(mut window) = window else { return };
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
                    if let Err(error) = crate::screenshot::write(&event.image, &path, None) {
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
    if !probe.checkpoint && elapsed >= probe.benchmark.map_or(50, |(s, _)| s) {
        info!("Window probe complete");
        probe.completed.store(true, Ordering::Release);
        exit.write(AppExit::Success);
    }
}
