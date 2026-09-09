//! A tiny bitmap panel keeps diagnostics independent of cooked fonts and UI crates.
use super::Monitor;
use bevy::{
    camera::visibility::RenderLayers,
    core_pipeline::tonemapping::Tonemapping,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::PrimaryWindow,
};
use std::time::{Duration, Instant};

const WIDTH: u32 = 368;
const HEIGHT: u32 = 226;
const LAYER: usize = 31;

#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(super) struct OverlayCamera;
#[derive(Resource)]
pub(super) struct Artwork {
    image: Handle<Image>,
    last_draw: Option<Instant>,
}

pub(super) fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    monitor: Res<Monitor>,
) {
    let mut image = Image::new(
        Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels(&monitor),
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    image.sampler = ImageSampler::nearest();
    let image = images.add(image);
    commands.spawn((
        Sprite::from_image(image.clone()),
        Panel,
        RenderLayers::layer(LAYER),
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 100,
            clear_color: ClearColorConfig::None,
            is_active: monitor.shown,
            ..default()
        },
        Tonemapping::None,
        Msaa::Off,
        OverlayCamera,
        RenderLayers::layer(LAYER),
    ));
    commands.insert_resource(Artwork {
        image,
        last_draw: None,
    });
}

pub(super) fn update(
    monitor: Res<Monitor>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut camera: Single<&mut Camera, With<OverlayCamera>>,
    mut panel: Single<&mut Transform, With<Panel>>,
    mut art: ResMut<Artwork>,
    mut images: ResMut<Assets<Image>>,
) {
    camera.is_active = monitor.shown;
    if !monitor.shown {
        art.last_draw = None;
        return;
    }
    if let Some(window) = window {
        let scale = ((window.width() - 16.) / WIDTH as f32)
            .min((window.height() - 16.) / HEIGHT as f32)
            .clamp(0.1, 1.);
        panel.scale = Vec3::splat(scale);
        panel.translation = Vec3::new(
            -window.width() / 2. + 8. + WIDTH as f32 * scale / 2.,
            window.height() / 2. - 8. - HEIGHT as f32 * scale / 2.,
            0.,
        );
    }
    let now = Instant::now();
    if art
        .last_draw
        .is_some_and(|last| now.duration_since(last) < Duration::from_millis(250))
    {
        return;
    }
    art.last_draw = Some(now);
    if let Some(mut image) = images.get_mut(&art.image) {
        image.data = Some(pixels(&monitor));
    }
}

fn pixels(monitor: &Monitor) -> Vec<u8> {
    let mut rgba = [12, 17, 25, 235].repeat((WIDTH * HEIGHT) as usize);
    let mut lines = vec!["PERFORMANCE  F3 HIDE  F4 SAVE".to_owned()];
    if let Some(s) = monitor.history.summary() {
        let current = monitor.history.0.back().unwrap();
        lines.extend([
            format!(
                "FPS {:5.1}   AVG {:5.1}",
                monitor.history.recent_fps().unwrap_or(0.),
                s.fps
            ),
            format!(
                "FRAME {:5.2}  APP {:5.2} MS",
                current.frame_ms, current.app_ms
            ),
            format!("P50 {:5.2}   P95 {:5.2} MS", s.p50_ms, s.p95_ms),
            format!("P99 {:5.2} P99.9 {:5.2} MS", s.p99_ms, s.p99_9_ms),
            format!(
                "1% LOW {:5.1}   0.1% {:5.1}",
                s.low_1_percent_fps, s.low_0_1_percent_fps
            ),
            format!(
                "MAX {:5.1} MS  >50MS {}",
                s.max_frame_ms, s.frames_over_50_ms
            ),
            format!(
                "{:.1}S  N {}  {}",
                s.window_seconds,
                s.samples,
                if s.tail_warmed_up { "" } else { "WARMUP" }
            ),
        ]);
    } else {
        lines.push("SAMPLING...".into());
    }
    for (row, text) in lines.iter().enumerate() {
        draw_text(
            &mut rgba,
            10,
            8 + row as u32 * 18,
            text,
            [220, 236, 249, 255],
        );
    }
    let notice = if monitor
        .notice_until
        .is_some_and(|until| Instant::now() < until)
    {
        monitor.notice
    } else if monitor.file.is_some() {
        "RECORDING - GRAPH TOP 50 MS"
    } else {
        "FRAME MS - GRAPH TOP 50 MS"
    };
    draw_text(&mut rgba, 10, 154, notice, [99, 215, 201, 255]);
    let top = 174;
    let bottom = HEIGHT - 9;
    let height = bottom - top;
    for threshold in [1000. / 60., 1000. / 30.] {
        let y = bottom - (threshold / 50. * f64::from(height)) as u32;
        for x in 10..WIDTH - 10 {
            put(&mut rgba, x, y, [64, 76, 91, 255]);
        }
    }
    for (index, sample) in monitor
        .history
        .0
        .iter()
        .rev()
        .take((WIDTH - 20) as usize)
        .enumerate()
    {
        let x = WIDTH - 11 - index as u32;
        let h = (sample.frame_ms.min(50.) / 50. * f64::from(height)) as u32;
        let color = if sample.frame_ms > 50. {
            [255, 99, 109, 255]
        } else if sample.frame_ms > 1000. / 30. {
            [255, 199, 101, 255]
        } else {
            [82, 201, 184, 255]
        };
        for y in bottom - h..=bottom {
            put(&mut rgba, x, y, color);
        }
    }
    rgba
}

fn put(rgba: &mut [u8], x: u32, y: u32, color: [u8; 4]) {
    if x < WIDTH && y < HEIGHT {
        let at = ((y * WIDTH + x) * 4) as usize;
        rgba[at..at + 4].copy_from_slice(&color);
    }
}

fn draw_text(rgba: &mut [u8], x: u32, y: u32, text: &str, color: [u8; 4]) {
    for (index, c) in text.chars().enumerate() {
        for (column, bits) in glyph(c).into_iter().enumerate() {
            for row in 0..7 {
                if bits & (1 << row) != 0 {
                    for dy in 0..2 {
                        for dx in 0..2 {
                            put(
                                rgba,
                                x + index as u32 * 12 + column as u32 * 2 + dx,
                                y + row * 2 + dy,
                                color,
                            );
                        }
                    }
                }
            }
        }
    }
}

fn glyph(c: char) -> [u8; 5] {
    match c {
        'A' => [126, 17, 17, 17, 126],
        'B' => [127, 73, 73, 73, 54],
        'C' => [62, 65, 65, 65, 34],
        'D' => [127, 65, 65, 34, 28],
        'E' => [127, 73, 73, 73, 65],
        'F' => [127, 9, 9, 9, 1],
        'G' => [62, 65, 73, 73, 122],
        'H' => [127, 8, 8, 8, 127],
        'I' => [0, 65, 127, 65, 0],
        'J' => [32, 64, 65, 63, 1],
        'K' => [127, 8, 20, 34, 65],
        'L' => [127, 64, 64, 64, 64],
        'M' => [127, 2, 12, 2, 127],
        'N' => [127, 4, 8, 16, 127],
        'O' => [62, 65, 65, 65, 62],
        'P' => [127, 9, 9, 9, 6],
        'Q' => [62, 65, 81, 33, 94],
        'R' => [127, 9, 25, 41, 70],
        'S' => [38, 73, 73, 73, 50],
        'T' => [1, 1, 127, 1, 1],
        'U' => [63, 64, 64, 64, 63],
        'V' => [31, 32, 64, 32, 31],
        'W' => [63, 64, 56, 64, 63],
        'X' => [99, 20, 8, 20, 99],
        'Y' => [7, 8, 112, 8, 7],
        'Z' => [97, 81, 73, 69, 67],
        '0' => [62, 81, 73, 69, 62],
        '1' => [0, 66, 127, 64, 0],
        '2' => [66, 97, 81, 73, 70],
        '3' => [33, 65, 69, 75, 49],
        '4' => [24, 20, 18, 127, 16],
        '5' => [39, 69, 69, 69, 57],
        '6' => [60, 74, 73, 73, 48],
        '7' => [1, 113, 9, 5, 3],
        '8' => [54, 73, 73, 73, 54],
        '9' => [6, 73, 73, 41, 30],
        '.' => [0, 96, 96, 0, 0],
        '%' => [99, 19, 8, 100, 99],
        '-' => [8; 5],
        '>' => [0, 65, 34, 20, 8],
        _ => [0; 5],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::performance::{PerformanceOptions, install, stats::Sample};

    #[test]
    fn panel_can_render_before_samples_and_with_long_frame_times() {
        let mut app = App::new();
        install(&mut app, PerformanceOptions::default(), true).unwrap();
        let mut monitor = app.world_mut().resource_mut::<Monitor>();
        assert_eq!(pixels(&monitor).len(), (WIDTH * HEIGHT * 4) as usize);
        for frame in 0..1800 {
            monitor.history.push(Sample {
                frame,
                elapsed_seconds: frame as f64 / 60.,
                frame_ms: if frame % 300 == 0 { 83. } else { 1000. / 60. },
                app_ms: 3.2,
            });
        }
        let rgba = pixels(&monitor);
        assert!(
            rgba.chunks_exact(4).any(|p| p == [255, 99, 109, 255]),
            "graph should identify stalls"
        );
        if let Some(path) = std::env::var_os("RESONANCE_PERFORMANCE_PANEL_CAPTURE") {
            Image::new(
                Extent3d {
                    width: WIDTH,
                    height: HEIGHT,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                rgba,
                TextureFormat::Rgba8UnormSrgb,
                default(),
            )
            .try_into_dynamic()
            .unwrap()
            .save(path)
            .unwrap();
        }
    }
}
