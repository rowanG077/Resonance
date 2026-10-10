//! Compare one frozen scene through completed offscreen and window readbacks.
use anyhow::{Result, ensure};
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    core_pipeline::{core_2d::Transparent2d, tonemapping::Tonemapping},
    ecs::system::NonSendMarker,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_phase::ViewSortedRenderPhases,
        render_resource::{
            CachedPipelineState, PipelineCache, PollType, TextureFormat, TextureUsages,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::PrimaryWindow,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const CHANNEL_TOLERANCE: u8 = 2;

#[derive(Resource)]
struct Check {
    directory: PathBuf,
    size: crate::Resolution,
    allow_surface_scaling: bool,
    surface: Option<UVec2>,
    held: Option<(u32, u32)>,
    captured: [Option<Image>; 2],
    requested: bool,
    completed: Arc<AtomicBool>,
}

#[derive(Resource, Clone, Default)]
struct MirrorFence(Arc<Mutex<Mirror>>);
#[derive(Default)]
struct Mirror {
    view: Option<MainEntity>,
    draw: Option<MainEntity>,
    submitted: bool,
    completed: Arc<AtomicBool>,
    error: Option<String>,
}

pub(super) fn install(
    app: &mut App,
    directory: &Path,
    size: crate::Resolution,
    allow_surface_scaling: bool,
    completed: Arc<AtomicBool>,
) {
    let mirror = MirrorFence::default();
    app.insert_resource(mirror.clone())
        .insert_resource(Check {
            directory: directory.into(),
            size,
            allow_surface_scaling,
            surface: None,
            held: None,
            captured: [None, None],
            requested: false,
            completed,
        })
        .add_systems(Update, capture.after(crate::layout));
    app.sub_app_mut(RenderApp)
        .insert_resource(mirror)
        .add_systems(Render, mirror_rendered.in_set(RenderSystems::Cleanup));
}

fn mirror_rendered(
    mirror: Res<MirrorFence>,
    phases: Res<ViewSortedRenderPhases<Transparent2d>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let mut mirror = mirror.0.lock().unwrap();
    if mirror.view.is_none() || mirror.error.is_some() || mirror.completed.load(Ordering::Acquire) {
        return;
    }
    let _ = device.poll(PollType::Poll);
    if mirror.submitted {
        return;
    }
    let draw = phases
        .0
        .iter()
        .filter(|(view, _)| Some(view.main_entity) == mirror.view)
        .flat_map(|(_, phase)| phase.items.values())
        .find(|item| Some(item.entity.1) == mirror.draw);
    let Some(draw) = draw else {
        return;
    };
    match cache.get_render_pipeline_state(draw.pipeline) {
        CachedPipelineState::Ok(_) => {
            mirror.submitted = true;
            let completed = mirror.completed.clone();
            queue.on_submitted_work_done(move || completed.store(true, Ordering::Release));
        }
        CachedPipelineState::Err(error) if !crate::model_preview::gpu::shader_pending(error) => {
            mirror.error = Some(error.to_string())
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)] // Actual surface, paired readbacks and held scene clocks.
fn capture(
    mut commands: Commands,
    mut check: ResMut<Check>,
    ready: Res<crate::timing::Ready>,
    mirror: Res<MirrorFence>,
    quad: Single<Entity, With<crate::display::OutputQuad>>,
    window: Single<Entity, With<PrimaryWindow>>,
    mut targets: ResMut<crate::display::Targets>,
    mut images: ResMut<Assets<Image>>,
    cameras: Query<(&Projection, &RenderTarget), With<crate::display::OutputCamera>>,
    menu: Res<crate::Menu>,
    clock: Res<crate::Clock>,
    mut time: ResMut<Time<Virtual>>,
    mut exit: MessageWriter<AppExit>,
    _main_thread: NonSendMarker,
) {
    let Some(surface) = super::window_probe::surface(*window) else {
        return;
    };
    let clocks = (menu.0.tick, clock.0.tick());
    if let Some(held) = check.held {
        if held != clocks || check.surface != Some(surface.size) {
            error!("Window comparison scene or surface changed during preparation/readback");
            exit.write(AppExit::error());
            return;
        }
        if check.requested {
            return;
        }
        let mirror = mirror.0.lock().unwrap();
        if let Some(error) = &mirror.error {
            error!("Window comparison mirror draw failed: {error}");
            exit.write(AppExit::error());
            return;
        }
        if !mirror.completed.load(Ordering::Acquire) {
            return;
        }
    } else {
        if !ready.0
            || !menu.0.revealed
            || menu.0.opacity != u8::MAX
            || surface.size.min_element() == 0
            || surface.minimized == Some(true)
            || (!check.allow_surface_scaling
                && surface.size != UVec2::new(check.size.width, check.size.height))
        {
            return;
        }
        // Match the actual surface and its AutoMin projection, including black margins.
        // Both cameras sample the same retained scene; no CPU resizing masks differences.
        let Some((projection, _)) = cameras
            .iter()
            .find(|(_, target)| matches!(target, RenderTarget::Window(_)))
        else {
            return;
        };
        let mut image = Image::new_target_texture(
            surface.size.x,
            surface.size.y,
            TextureFormat::Bgra8UnormSrgb,
            None,
        );
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let image = images.add(image);
        targets.output = Some(image.clone());
        let view = commands
            .spawn((
                Camera2d,
                Camera {
                    order: -1,
                    clear_color: Color::BLACK.into(),
                    ..default()
                },
                Tonemapping::None,
                Msaa::Off,
                RenderLayers::layer(2),
                crate::display::OutputCamera,
                RenderTarget::Image(image.into()),
                projection.clone(),
            ))
            .id();
        *mirror.0.lock().unwrap() = Mirror {
            view: Some(view.into()),
            draw: Some((*quad).into()),
            ..default()
        };
        // The mirror owns a fresh view-specific submission fence. Keep the selected
        // scene frozen while its pipeline and GPU work become ready, then read back.
        time.pause();
        check.held = Some(clocks);
        check.surface = Some(surface.size);
        return;
    }
    check.requested = true;
    for (index, (name, screenshot)) in [
        (
            "offscreen.png",
            Screenshot::image(targets.output.clone().unwrap()),
        ),
        ("window.png", Screenshot::primary_window()),
    ]
    .into_iter()
    .enumerate()
    {
        commands.spawn(screenshot).observe(
            move |event: On<ScreenshotCaptured>, mut check: ResMut<Check>, menu: Res<crate::Menu>, clock: Res<crate::Clock>, mut monitor: ResMut<super::Monitor>, mut exit: MessageWriter<AppExit>| {
                let result = (|| -> Result<bool> {
                    ensure!(clocks == (menu.0.tick, clock.0.tick()), "scene advanced before screenshot completion");
                    crate::screenshot::write(&event.image, &check.directory.join(name), Some(&serde_json::json!({
                        "title_tick":clocks.0, "presentation_counter":clocks.1, "state_tick_locked":true,
                    })))?;
                    check.captured[index] = Some(event.image.clone());
                    let [Some(offscreen), Some(window)] = &check.captured else { return Ok(false); };
                    let max_delta = compare(offscreen, window, surface.size)?;
                    monitor.finish_recording()?;
                    std::fs::write(check.directory.join("comparison.json"), serde_json::to_vec_pretty(&serde_json::json!({
                        "width":surface.size.x, "height":surface.size.y,
                        "channel_tolerance":CHANNEL_TOLERANCE, "maximum_channel_difference":max_delta,
                    }))?)?;
                    Ok(true)
                })();
                match result {
                    Ok(true) => { check.completed.store(true, Ordering::Release); exit.write(AppExit::Success); }
                    Ok(false) => {}
                    Err(error) => { error!("Window output comparison failed: {error:#}"); exit.write(AppExit::error()); }
                }
            },
        );
    }
}

fn compare(offscreen: &Image, window: &Image, expected: UVec2) -> Result<u8> {
    ensure!(
        offscreen.size() == expected && window.size() == expected,
        "expected {expected:?} captures, got offscreen {:?}, window {:?}",
        offscreen.size(),
        window.size()
    );
    let offscreen = offscreen.clone().try_into_dynamic()?.to_rgba8();
    let window = window.clone().try_into_dynamic()?.to_rgba8();
    let mut max_delta = 0;
    for (index, (&a, &b)) in offscreen.as_raw().iter().zip(window.as_raw()).enumerate() {
        let delta = a.abs_diff(b);
        ensure!(
            delta <= CHANNEL_TOLERANCE,
            "pixel ({}, {}), channel {} differs: offscreen {a}, window {b} (tolerance {CHANNEL_TOLERANCE})",
            index / 4 % expected.x as usize,
            index / 4 / expected.x as usize,
            index % 4
        );
        max_delta = max_delta.max(delta);
    }
    Ok(max_delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension},
    };

    #[test]
    fn comparison_rejects_wrong_pixels_and_dimensions() {
        let image = |bytes| {
            Image::new(
                Extent3d {
                    width: 2,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                bytes,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            )
        };
        let reference = image(vec![10, 20, 30, 255, 100, 110, 120, 200]);
        assert_eq!(
            compare(&reference, &reference, UVec2::new(2, 1)).unwrap(),
            0
        );
        let rounded = image(vec![12, 18, 31, 255, 100, 109, 122, 198]);
        assert_eq!(compare(&reference, &rounded, UVec2::new(2, 1)).unwrap(), 2);
        for channel in 0..8 {
            let mut wrong = reference.clone();
            wrong.data.as_mut().unwrap()[channel] -= 3;
            assert!(compare(&reference, &wrong, UVec2::new(2, 1)).is_err());
        }
        assert!(compare(&reference, &reference, UVec2::new(1, 2)).is_err());
        let wrong_size = Image::new_target_texture(1, 2, TextureFormat::Rgba8UnormSrgb, None);
        assert!(compare(&reference, &wrong_size, UVec2::new(2, 1)).is_err());
    }
}
