//! First-run disc selection and a spoiler-safe, animated background import.
mod pacing;
pub mod process;
mod ui;

use anyhow::{Result, ensure};
use bevy::{
    camera::ScalingMode,
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::MouseWheel,
    input_focus::tab_navigation::TabNavigationPlugin,
    prelude::*,
    window::FileDragAndDrop,
    winit::{UpdateMode, WinitSettings},
};
use resonance_importer::ImportProgress;
use resonance_yggdrasil::{
    Renderer, YggdrasilOutput, YggdrasilPlayback, YggdrasilPlugin, YggdrasilSystems,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Select,
    Importing,
    Complete,
    Failed,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Hidden,
    Warning,
    Output,
}

type Selection = (usize, std::result::Result<Option<PathBuf>, String>);

#[derive(Resource)]
struct Setup {
    assets: PathBuf,
    mods: PathBuf,
    reimport: bool,
    extracted: Option<PathBuf>,
    discs: [Option<PathBuf>; 2],
    picker: Option<Mutex<mpsc::Receiver<Selection>>>,
    workers: usize,
    phase: Phase,
    panel: Panel,
    spoiler_consent: bool,
    notice: String,
    stage: String,
    log: VecDeque<String>,
    log_offset: usize,
    job: Option<process::Job>,
    pacing: pacing::Pacing,
    rebuild: bool,
    play: Arc<AtomicBool>,
    scene_seconds: f64,
    visit: usize,
}

impl Setup {
    fn new(assets: PathBuf, mods: PathBuf, extracted: Option<PathBuf>) -> Self {
        let workers = process::worker_budget();
        let reimport = process::library_status(&assets) == process::LibraryStatus::Outdated;
        Self {
            assets,
            mods,
            reimport,
            extracted,
            discs: [None, None],
            picker: None,
            workers,
            phase: Phase::Select,
            panel: Panel::Hidden,
            spoiler_consent: false,
            notice: String::new(),
            stage: String::new(),
            log: VecDeque::new(),
            log_offset: 0,
            job: None,
            pacing: pacing::Pacing::new(workers),
            rebuild: true,
            play: Arc::new(AtomicBool::new(false)),
            scene_seconds: 0.,
            visit: 0,
        }
    }

    fn source(&self) -> Option<process::Source> {
        if let Some(directory) = &self.extracted {
            return Some(process::Source::Extracted {
                directory: directory.clone(),
            });
        }
        let [Some(disc1), Some(disc2)] = &self.discs else {
            return None;
        };
        Some(process::Source::Discs {
            disc1: disc1.clone(),
            disc2: disc2.clone(),
        })
    }

    fn output(&mut self, line: String) {
        if self.log_offset > 0 {
            self.log_offset += 1;
        }
        self.log.push_back(line);
        if self.log.len() > 2000 {
            self.log.pop_front();
        }
        self.log_offset = self.log_offset.min(self.log.len().saturating_sub(1));
    }

    fn failed(&mut self, error: impl std::fmt::Display) {
        self.output(error.to_string());
        self.phase = Phase::Failed;
        self.stage = "The import could not finish. You can try again or view the details.".into();
        self.rebuild = true;
    }

    fn select(&mut self, slot: usize, paths: Vec<PathBuf>) {
        if self.picker.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.picker = Some(Mutex::new(rx));
        self.notice = "Choosing a disc…".into();
        self.rebuild = true;
        std::thread::spawn(move || {
            let paths = if paths.is_empty() {
                rfd::FileDialog::new()
                    .set_title(format!("Tales of Symphonia — Disc {}", slot + 1))
                    .add_filter("GameCube disc image", &["rvz", "iso", "gcm", "gcz", "wia"])
                    .pick_file()
                    .into_iter()
                    .collect()
            } else {
                paths
            };
            if paths.is_empty() {
                let _ = tx.send((slot, Ok(None)));
            }
            for (offset, path) in paths.into_iter().take(2 - slot).enumerate() {
                let slot = slot + offset;
                let result = resonance_importer::validate_disc(&path, slot as u8)
                    .map(|()| Some(path))
                    .map_err(|error| {
                        let message = error.to_string();
                        if message.starts_with("Expected disc") || message.starts_with("Please use") {
                            message
                        } else {
                            format!("Disc {} could not be read. Choose a readable GameCube image in a supported format.", slot + 1)
                        }
                    });
                if tx.send((slot, result)).is_err() {
                    break;
                }
            }
        });
    }
}

pub fn run(assets: PathBuf, mods: PathBuf) -> Result<()> {
    let extracted = PathBuf::from("local/extracted-discs");
    let extracted = resonance_importer::extracted_discs_available(&extracted).then_some(extracted);
    let setup = Setup::new(assets, mods, extracted);
    let play = setup.play.clone();
    let mut app = App::new();
    app.add_plugins(
        resonance_presentation::native_render_plugins(default())
            .disable::<bevy::log::LogPlugin>()
            .set(ImagePlugin::default_nearest())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Resonance · A new beginning".into(),
                    resolution: (1024, 800).into(),
                    resize_constraints: bevy::window::WindowResizeConstraints {
                        min_width: 800.,
                        min_height: 640.,
                        ..default()
                    },
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins(TabNavigationPlugin)
    .insert_resource(ClearColor(Color::srgb(0.06, 0.09, 0.14)))
    .insert_resource(setup)
    .insert_resource(YggdrasilPlayback {
        paused: true,
        progress: Some(0.),
        ..default()
    })
    .insert_resource(WinitSettings {
        focused_mode: UpdateMode::reactive(Duration::from_millis(33)),
        unfocused_mode: UpdateMode::reactive_low_power(Duration::from_millis(100)),
    })
    .add_plugins(YggdrasilPlugin)
    .add_systems(Startup, (scene, ui::load_art))
    .add_systems(
        Update,
        (poll, animate, layout_scene, ui::rebuild, ui::refresh)
            .chain()
            .before(YggdrasilSystems::Render),
    )
    .add_systems(Update, (input, ui::button_appearance))
    .add_observer(ui::activate);
    configure_schedules(&mut app);
    let result = app.run();
    // The runner owns and drops the original world (including the worker).
    drop(app);
    if matches!(result, AppExit::Error(code) if code.get() == 130) {
        return Ok(()); // Terminal cancellation is not an application failure.
    }
    ensure!(result == AppExit::Success, "Setup exited with {result:?}");
    if play.load(Ordering::Relaxed) {
        // Winit supports one event loop per process on desktop platforms. Start
        // the normal game with the exact original arguments in a fresh process.
        let status = anyhow::Context::context(
            std::process::Command::new(std::env::current_exe()?)
                .args(std::env::args_os().skip(1))
                .status(),
            "launch Resonance",
        )?;
        ensure!(status.success(), "Resonance exited with {status}");
    }
    Ok(())
}

fn configure_schedules(app: &mut App) {
    // This small UI benefits from serial schedules while the render app keeps
    // its own thread. Avoid spending the importer's CPU allowance on dispatch.
    use bevy::ecs::schedule::{ScheduleLabel, SingleThreadedExecutor};
    for label in [
        First.intern(),
        PreUpdate.intern(),
        Update.intern(),
        PostUpdate.intern(),
        Last.intern(),
    ] {
        app.edit_schedule(label, |s| {
            s.set_executor(SingleThreadedExecutor::new());
        });
    }
    app.sub_app_mut(bevy::render::RenderApp)
        .edit_schedule(bevy::render::Render, |s| {
            s.set_executor(SingleThreadedExecutor::new());
        });
}

#[derive(Component)]
struct SceneBackdrop;

fn scene(mut commands: Commands, output: Res<YggdrasilOutput>) {
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin {
                min_width: 1024.,
                min_height: 640.,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
    commands.spawn((SceneBackdrop, Sprite::from_image(output.image.clone())));
}

fn layout_scene(
    state: Res<Setup>,
    cameras: Query<&Camera, With<Camera2d>>,
    mut backdrops: Query<&mut Transform, With<SceneBackdrop>>,
) {
    let Some(size) = cameras
        .single()
        .ok()
        .and_then(Camera::logical_viewport_size)
    else {
        return;
    };
    let full_scale = (size.x / 1024.).min(size.y / 640.);
    if full_scale <= 0. {
        return;
    }
    // Reserve space for the controls instead of covering the camp's front row.
    // The default 1024 × 800 window retains the artwork's native pixel size.
    let footer = if state.phase == Phase::Select {
        0.
    } else {
        160.
    };
    let scale = (size.x / 1024.).min((size.y - footer).max(1.) / 640.) / full_scale;
    for mut transform in &mut backdrops {
        transform.scale = Vec3::new(scale, scale, 1.);
        transform.translation.y = footer / (2. * full_scale);
    }
}

fn poll(mut state: ResMut<Setup>) {
    if let Some(receiver) = &state.picker {
        let mut selections = Vec::new();
        let finished = loop {
            match receiver.lock().unwrap().try_recv() {
                Ok(selection) => selections.push(selection),
                Err(mpsc::TryRecvError::Disconnected) => break true,
                Err(mpsc::TryRecvError::Empty) => break false,
            }
        };
        for (slot, result) in selections {
            state.notice = match result {
                Ok(Some(path)) => {
                    state.discs[slot] = Some(path);
                    String::new()
                }
                Ok(None) => String::new(),
                Err(error) => error,
            };
            state.rebuild = true;
        }
        if finished {
            state.picker = None;
            state.rebuild = true;
        }
    }
    let Some(job) = &mut state.job else {
        return;
    };
    let status = job.status();
    let mut messages = job.drain();
    // Once reaped, both pipes can still have a final buffered message. The exit
    // status, plus published assets, is authoritative for completion.
    for message in messages.drain(..) {
        match message {
            process::Message::Progress(event) => {
                state.stage = match &event {
                    ImportProgress::Extracting { disc, .. } => format!("Reading disc {disc} of 2"),
                    ImportProgress::Cooking { stage, total } => {
                        format!("Preparing your world · step {stage} of {total}")
                    }
                    ImportProgress::Publishing | ImportProgress::Finished => {
                        "Putting everything in place…".into()
                    }
                };
                state.pacing.event(&event);
            }
            process::Message::Output(line) => state.output(line),
        }
    }
    match status {
        Ok(Some(status)) => {
            state.job.take();
            if status.success() && process::imported(&state.assets) {
                state.phase = Phase::Complete;
                state.stage = "Your world is ready. A new journey awaits.".into();
                state.rebuild = true;
            } else {
                state.failed(format!("Importer exited with {status}"));
            }
        }
        Err(error) => {
            state.job.take();
            state.failed(error);
        }
        _ => {}
    }
}

fn animate(
    time: Res<Time<Real>>,
    renderer: Res<Renderer>,
    mut state: ResMut<Setup>,
    mut playback: ResMut<YggdrasilPlayback>,
) {
    let dt = time.delta_secs_f64().min(0.2);
    let complete = state.phase == Phase::Complete;
    let progress = if state.phase == Phase::Importing || complete {
        state.pacing.update(dt, complete)
    } else {
        state.pacing.shown
    };
    playback.progress = Some(progress);
    // A visit starts at its authored growth threshold and then plays at its
    // native rate. Waiting between visits never freezes a character mid-action.
    let visits = renderer.events();
    if state.phase != Phase::Select && state.visit < visits.len() {
        let visit = &visits[state.visit];
        let threshold = renderer.progress_at(visit.start);
        if progress >= threshold || state.scene_seconds >= visit.start {
            state.scene_seconds =
                (state.scene_seconds.max(visit.start) + dt).min(visit.start + visit.duration);
            if state.scene_seconds >= visit.start + visit.duration {
                state.visit += 1;
            }
        }
    } else if complete {
        state.scene_seconds += dt;
    }
    playback.seconds = state.scene_seconds;
}

fn input(
    mut state: ResMut<Setup>,
    keys: Res<ButtonInput<KeyCode>>,
    mut drops: MessageReader<FileDragAndDrop>,
    mut wheel: MessageReader<MouseWheel>,
) {
    let paths = drops
        .read()
        .filter_map(|event| match event {
            FileDragAndDrop::DroppedFile { path_buf, .. } => Some(path_buf.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !paths.is_empty() && state.phase == Phase::Select && state.panel == Panel::Hidden {
        state.extracted = None;
        let slot = usize::from(paths.len() == 1 && state.discs[0].is_some());
        state.select(slot, paths);
    }
    if keys.just_pressed(KeyCode::Escape) && state.panel != Panel::Hidden {
        state.panel = Panel::Hidden;
        state.rebuild = true;
    }
    for event in wheel.read() {
        if state.panel == Panel::Output {
            if event.y > 0. {
                state.log_offset = (state.log_offset + 3).min(state.log.len().saturating_sub(1));
            }
            if event.y < 0. {
                state.log_offset = state.log_offset.saturating_sub(3);
            }
        }
    }
    if state.panel == Panel::Output {
        if keys.just_pressed(KeyCode::PageUp) {
            state.log_offset = (state.log_offset + 16).min(state.log.len().saturating_sub(1));
        }
        if keys.just_pressed(KeyCode::PageDown) {
            state.log_offset = state.log_offset.saturating_sub(16);
        }
        if keys.just_pressed(KeyCode::End) {
            state.log_offset = 0;
        }
    }
}

#[cfg(test)]
mod tests;
