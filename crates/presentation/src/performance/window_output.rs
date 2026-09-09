//! Paired captures of one frozen scene through the offscreen and window paths.
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
    render::render_resource::{TextureFormat, TextureUsages},
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    window::PrimaryWindow,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[derive(Resource)]
struct Check {
    directory: PathBuf,
    size: crate::Resolution,
    allow_surface_scaling: bool,
    settled: u32,
    requested: bool,
    captured: Arc<AtomicUsize>,
    completed: Arc<AtomicBool>,
}

pub(super) fn install(
    app: &mut App,
    directory: &Path,
    size: crate::Resolution,
    allow_surface_scaling: bool,
    completed: Arc<AtomicBool>,
) {
    app.insert_resource(Check {
        directory: directory.into(),
        size,
        allow_surface_scaling,
        settled: 0,
        requested: false,
        captured: Arc::default(),
        completed,
    })
    .add_systems(Update, capture.after(crate::layout))
    .add_systems(Startup, mirror.after(crate::display::initialize));
}

fn mirror(
    mut commands: Commands,
    mut targets: ResMut<crate::display::Targets>,
    display: Res<crate::display::Display>,
    mut images: ResMut<Assets<Image>>,
    projection: Single<&Projection, With<crate::display::OutputCamera>>,
) {
    let mut image = Image::new_target_texture(
        display.0.width,
        display.0.height,
        TextureFormat::Bgra8UnormSrgb,
        None,
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = images.add(image);
    targets.output = Some(image.clone());
    commands.spawn((
        Camera2d,
        Camera {
            order: -1,
            ..default()
        },
        Tonemapping::None,
        Msaa::Off,
        RenderLayers::layer(2),
        crate::display::OutputCamera,
        RenderTarget::Image(image.into()),
        projection.clone(),
    ));
}

fn capture(
    mut commands: Commands,
    mut check: ResMut<Check>,
    ready: Res<crate::timing::Ready>,
    pipelines: Res<crate::RenderReady>,
    window: Single<&Window, With<PrimaryWindow>>,
    targets: Res<crate::display::Targets>,
) {
    if check.requested {
        return;
    }
    if !ready.0
        || !pipelines.0.load(Ordering::Relaxed)
        || window.physical_width() == 0
        || window.physical_height() == 0
        || (!check.allow_surface_scaling
            && (window.physical_width() != check.size.width
                || window.physical_height() != check.size.height))
    {
        check.settled = 0;
        return;
    }
    check.settled += 1;
    if check.settled < 20 {
        return;
    }
    check.requested = true;
    for (name, screenshot) in [
        (
            "offscreen.png",
            Screenshot::image(targets.output.clone().unwrap()),
        ),
        ("window.png", Screenshot::primary_window()),
    ] {
        let path = check.directory.join(name);
        let captured = check.captured.clone();
        let completed = check.completed.clone();
        commands.spawn(screenshot).observe(
            move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                let result = event
                    .image
                    .clone()
                    .try_into_dynamic()
                    .map_err(anyhow::Error::from)
                    .and_then(|image| image.save(&path).map_err(anyhow::Error::from));
                if let Err(error) = result {
                    error!("Window output comparison capture failed: {error:#}");
                    exit.write(AppExit::error());
                } else if captured.fetch_add(1, Ordering::AcqRel) == 1 {
                    completed.store(true, Ordering::Release);
                    exit.write(AppExit::Success);
                }
            },
        );
    }
}
