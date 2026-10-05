//! GPU compositing and playback regression checks using the production passes.
use crate::{
    Playback, YggdrasilOutput, YggdrasilPlayback, YggdrasilPlugin, YggdrasilSystems,
    drawing::{BlendMode, Drawing},
    gpu,
    renderer::Frame,
};
use bevy::{
    ecs::schedule::ScheduleLabel,
    prelude::*,
    render::{
        RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::PollType,
        renderer::RenderDevice,
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Resource)]
struct Capturing;

struct Capture {
    app: App,
}
impl Capture {
    fn new() -> Self {
        let mut app = App::new();
        let plugins = DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
        app.add_plugins(plugins)
            .insert_resource(YggdrasilPlayback {
                paused: true,
                ..default()
            })
            .add_plugins(YggdrasilPlugin)
            .configure_sets(
                Update,
                YggdrasilSystems::Render.run_if(not(resource_exists::<Capturing>)),
            );
        // This standalone 2D harness has little parallel system work. Avoid
        // dispatching hundreds of empty systems to a thread pool every frame.
        // YggdrasilPlugin itself leaves the embedding application's schedulers alone.
        for label in [
            First.intern(),
            PreUpdate.intern(),
            Update.intern(),
            PostUpdate.intern(),
            Last.intern(),
        ] {
            app.get_schedule_mut(label)
                .unwrap()
                .set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        }
        app.sub_app_mut(bevy::render::RenderApp)
            .get_schedule_mut(bevy::render::Render)
            .unwrap()
            .set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        app.finish();
        app.cleanup();
        Self { app }
    }
    fn gpu_resources(&mut self) -> (usize, u64) {
        self.update();
        gpu::resource_usage(self.app.sub_app(bevy::render::RenderApp).world())
    }
    fn update(&mut self) {
        self.app.update();
        let _ = self
            .app
            .world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            });
    }
    fn resize(&mut self, width: u32, height: u32) {
        let output = self.app.world().resource::<YggdrasilOutput>().image.clone();
        let mut images = self.app.world_mut().resource_mut::<Assets<Image>>();
        let mut image = images.get_mut(&output).unwrap();
        if image.width() != width || image.height() != height {
            *image = gpu::target(width, height);
        }
    }
    fn resume(&mut self) {
        self.app.world_mut().remove_resource::<Capturing>();
        self.app
            .world_mut()
            .resource_mut::<crate::plugin::LastSample>()
            .0 = None;
        self.resize(crate::WIDTH, crate::HEIGHT);
    }
    fn render(&mut self, frame: &Frame) -> Result<Vec<u8>, &'static str> {
        self.app.world_mut().insert_resource(Capturing);
        self.resize(frame.width(), frame.height());
        let output = self.app.world().resource::<YggdrasilOutput>().image.clone();
        *self.app.world_mut().resource_mut::<gpu::Input>() = gpu::Input {
            frame: Some(frame.clone()),
            output,
        };
        // Wait for this scene to reach the render graph before reading it back.
        let start = Instant::now();
        while self
            .app
            .sub_app(bevy::render::RenderApp)
            .world()
            .resource::<gpu::CaptureStatus>()
            .completed_frame
            != Some(frame.image.0.id)
        {
            self.update();
            if start.elapsed() > Duration::from_secs(30) {
                return Err("scene preparation timed out");
            }
            std::thread::yield_now();
        }
        let result = Arc::new(Mutex::new(None));
        let saved = result.clone();
        let image = self.app.world().resource::<YggdrasilOutput>().image.clone();
        let (width, height) = (frame.width() as usize, frame.height() as usize);
        self.app
            .world_mut()
            .spawn(Readback::texture(image))
            .observe(move |event: On<ReadbackComplete>, mut commands: Commands| {
                let row = width * 4;
                let stride = row.div_ceil(256) * 256;
                let rgba = event
                    .data
                    .chunks(stride)
                    .take(height)
                    .flat_map(|bytes| bytes[..row].iter().copied())
                    .collect();
                *saved.lock().unwrap() = Some(rgba);
                commands.entity(event.entity).despawn();
            });
        let start = Instant::now();
        loop {
            self.update();
            if let Some(result) = result.lock().unwrap().take() {
                return Ok(result);
            }
            if start.elapsed() > Duration::from_secs(30) {
                return Err("GPU capture timed out");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn image(&mut self, image: crate::drawing::Image) -> Result<Vec<u8>, &'static str> {
        self.render(&Frame {
            image,
            progress: 0.,
            seconds: 0.,
        })
    }
}

#[test]
fn gpu_compositing_playback_and_resource_lifetime() {
    let mut capture = Capture::new();
    let mut layer = Drawing::new(8, 8);
    layer.fill([0., 0., 4., 8.], "#ff0000", 1., BlendMode::SrcOver);
    layer.fill([0., 0., 8., 8.], "#0000ff", 0.5, BlendMode::SrcATop);
    let mut scene = Drawing::new(8, 8);
    scene.fill([0., 0., 8., 8.], "#00ff00", 1., BlendMode::SrcOver);
    scene.image(&layer.snapshot(), 0., 0.);
    let diagnostic = scene.snapshot();
    let rgba = capture.image(diagnostic.clone()).unwrap();
    let pixel = |x: usize, y: usize| &rgba[(y * 8 + x) * 4..(y * 8 + x + 1) * 4];
    assert_eq!(
        pixel(6, 4),
        &[0, 255, 0, 255],
        "tint must preserve transparent holes"
    );
    let p = pixel(1, 4);
    assert!(
        p[0].abs_diff(128) <= 1 && p[2].abs_diff(128) <= 1 && p[1] == 0 && p[3] == 255,
        "sRGB source-atop: {p:?}"
    );
    let mut parent = Drawing::new(8, 8);
    parent.image(&diagnostic, 0., 0.);
    assert_eq!(
        capture.image(parent.snapshot()).unwrap(),
        rgba,
        "a retained previous output can become a child without sampling its own render target"
    );

    // A one-pixel eyelid reduced to 75% must keep its connected bottom row.
    // Exercise texel-boundary ties at different positions, including a flip.
    let mut eyelid = Drawing::new(8, 8);
    for rect in [[0., 4., 1., 1.], [7., 4., 1., 1.], [1., 5., 6., 1.]] {
        eyelid.fill(rect, "#ffffff", 1., BlendMode::SrcOver);
    }
    let eyelid = eyelid.snapshot();
    // The same thin lines must survive an offset atlas region beside vivid
    // neighboring pixels, including reduced and mirrored draws.
    let mut atlas_pixels = vec![255; 2048 * 512 * 4];
    let source = capture.image(eyelid.clone()).unwrap();
    for y in 0..8 {
        let start = ((471 + y) * 2048 + 1901) * 4;
        atlas_pixels[start..start + 8 * 4].copy_from_slice(&source[y * 8 * 4..(y + 1) * 8 * 4]);
    }
    let atlas = crate::drawing::image_rgba(2048, 512, &atlas_pixels);
    let packed = crate::drawing::image_region(&atlas, [1901, 471, 8, 8]);
    assert_eq!(&*packed.source_pixels(), source);
    for eyelid in [eyelid, packed] {
        for width in [19, 32, 1024] {
            let mut scene = Drawing::new(width, 9);
            for (x, flip) in [(1., false), (10., true)] {
                scene.save();
                scene.translate(x + if flip { 6. } else { 0. }, 1.);
                scene.scale(if flip { -1. } else { 1. }, 1.);
                scene.image_scaled(&eyelid, 0., 0., 6., 6.);
                scene.restore();
            }
            let rgba = capture.image(scene.snapshot()).unwrap();
            for origin in [1, 10] {
                for (row, expected) in
                    [(3, [255, 0, 0, 0, 0, 255]), (4, [0, 255, 255, 255, 255, 0])]
                {
                    let alpha: Vec<_> = (0..6)
                        .map(|x| rgba[((row + 1) * width as usize + origin + x) * 4 + 3])
                        .collect();
                    assert_eq!(alpha, expected, "thin line at x={origin}, width={width}");
                }
            }
        }
    }

    // Fresh nested layers exercise allocation/reuse without relying on story art.
    let render_batch = |capture: &mut Capture| {
        for tick in 0..32 {
            let mut moving = Drawing::new(8, 8);
            moving.image(&diagnostic, 0., 0.);
            moving.fill(
                [f64::from(tick % 4), 0., 2., 2.],
                "#ffffff",
                1.,
                BlendMode::SrcOver,
            );
            let mut scene = Drawing::new(8, 8);
            scene.image(&moving.snapshot(), 0., 0.);
            capture.image(scene.snapshot()).unwrap();
        }
    };
    render_batch(&mut capture);
    let resources_before = capture.gpu_resources();
    render_batch(&mut capture);
    assert_eq!(
        capture.gpu_resources(),
        resources_before,
        "repeating the same workload must not accumulate GPU resources"
    );
    assert_eq!(
        capture.image(diagnostic).unwrap(),
        rgba,
        "a cached child selected as the output must reproduce its pixels"
    );

    capture.resume();
    *capture.app.world_mut().resource_mut::<YggdrasilPlayback>() = YggdrasilPlayback {
        playback: Playback::Growth,
        seconds: 8.,
        progress: Some(0.35),
        paused: true,
        ..default()
    };
    capture.update();
    let output = capture.app.world().resource::<YggdrasilOutput>();
    assert_eq!(output.seconds, 8.);
    assert_eq!(output.progress, 0.35);
    assert!(output.error.is_none());
    let output_handle = output.image.clone();
    capture
        .app
        .world_mut()
        .resource_mut::<YggdrasilPlayback>()
        .playback = Playback::Event("missing".into());
    capture.update();
    assert!(
        capture
            .app
            .world()
            .resource::<YggdrasilOutput>()
            .error
            .is_some()
    );
    assert_eq!(
        capture.app.world().resource::<YggdrasilOutput>().image,
        output_handle
    );

    // A bad selection must not prevent subsequent valid requests from rendering.
    *capture.app.world_mut().resource_mut::<YggdrasilPlayback>() = YggdrasilPlayback {
        playback: Playback::Growth,
        seconds: 9.,
        progress: Some(0.75),
        paused: true,
        ..default()
    };
    capture.update();
    let output = capture.app.world().resource::<YggdrasilOutput>();
    assert!(output.error.is_none());
    assert_eq!((output.seconds, output.progress), (9., 0.75));
    assert_eq!(
        capture.app.world().resource::<YggdrasilOutput>().image,
        output_handle
    );
}
