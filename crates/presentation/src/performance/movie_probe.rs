//! Muted real-window evidence for fullscreen movie deadlines and render stalls.
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::RenderTarget,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    window::{PrimaryWindow, WindowRef},
};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub fn run_movie_probe(
    root: &Path,
    output: &Path,
    stalls: bool,
    known_content: bool,
) -> Result<()> {
    ensure!(!output.exists(), "movie probe output already exists");
    std::fs::create_dir_all(output)?;
    let options = crate::RunOptions {
        script_root: None,
        saves: Default::default(),
        assets: root.into(),
        capture_at: None,
        capture: None,
        reveal: false,
        selected: 0,
        silent: true,
        paranoid: true,
        skip_intro: false,
        record_playthrough: None,
        record_title_ticks: 1000,
        skip_battles: false,
        allow_incomplete_scripts: false,
    };
    let fixture = if known_content {
        write_known_clip(output)?;
        Some(crate::movie::Playback::load(output, &options)?)
    } else {
        None
    };
    let (mut app, _) =
        crate::build_app_with_display(options, "1920x1080".parse().map_err(anyhow::Error::msg)?)?;
    if let Some(fixture) = fixture {
        app.insert_resource(fixture);
    }
    super::install(
        &mut app,
        super::PerformanceOptions {
            overlay: false,
            dump: Some(output.join("frames.jsonl")),
        },
        false,
    )?;
    let asset = app
        .world()
        .resource::<crate::movie::Playback>()
        .asset
        .as_ref()
        .context("movie probe has no movie")?;
    let interval = Duration::from_micros(u64::from(asset.frame_micros));
    let mut probe = Probe::new(
        output,
        stalls,
        interval,
        interval * (asset.frames - 1),
        Duration::from_secs_f64(asset.audio_frames as f64 / f64::from(asset.sample_rate)),
    );
    probe.known_content = known_content;
    app.insert_resource(probe);
    app.add_systems(PreUpdate, stall.before(crate::gather_input));
    app.add_systems(Last, observe);
    let exit = app.run();
    let result: serde_json::Value = serde_json::from_slice(
        &std::fs::read(output.join("movie.json")).context("movie probe exited without evidence")?,
    )?;
    ensure!(
        exit == AppExit::Success && result["accepted"] == true,
        "movie probe rejected: {}",
        result["failure"]
    );
    Ok(())
}

// A few solid interior regions give an independent expectation for the complete
// decoder -> movie material -> output material -> window path. Every plateau
// repeats its exact pixels for 90 frames; CPU progress need not change an image.
const COLORS: [[[u8; 3]; 4]; 3] = [
    [[224, 40, 72], [48, 192, 88], [56, 80, 208], [192, 160, 32]],
    [[48, 192, 88], [56, 80, 208], [192, 160, 32], [224, 40, 72]],
    [[192, 160, 32], [224, 40, 72], [48, 192, 88], [56, 80, 208]],
];
fn write_known_clip(output: &Path) -> Result<()> {
    use resonance_media::encode::{AUDIO_BLOCK, MovieWriter};
    const FRAMES: u32 = 240;
    const MICROS: u32 = 33_333;
    const RATE: u32 = 32_000;
    let directory = output.join("movies");
    std::fs::create_dir(&directory)?;
    let path = directory.join("known.mkv");
    let audio_frames = u64::from(FRAMES) * u64::from(MICROS) * u64::from(RATE) / 1_000_000;
    let mut writer = MovieWriter::new(std::fs::File::create(&path)?, 640, 480, MICROS, RATE)?;
    let images: Vec<Vec<u8>> = COLORS
        .iter()
        .map(|colors| {
            (0..480)
                .flat_map(|y| {
                    (0..640).flat_map(move |x| {
                        colors[usize::from(x >= 320) + 2 * usize::from(y >= 240)]
                    })
                })
                .collect()
        })
        .collect();
    let (mut frame, mut audio) = (0u32, 0u64);
    while frame < FRAMES || audio < audio_frames {
        if audio < audio_frames
            && (frame == FRAMES
                || audio * 1_000_000 <= u64::from(frame) * u64::from(MICROS) * u64::from(RATE))
        {
            let count = (audio_frames - audio).min(AUDIO_BLOCK as u64);
            writer.audio(&vec![0; count as usize * 2])?;
            audio += count;
        } else {
            writer.video(
                &images[(frame / 90) as usize],
                Duration::from_micros(u64::from(frame) * u64::from(MICROS)),
            )?;
            frame += 1;
        }
    }
    writer.finish()?;
    let asset = resonance_content::MovieAsset {
        version: 2,
        path: "movies/known.mkv".into(),
        sha256: format!("{:x}", Sha256::digest(std::fs::read(path)?)),
        width: 640,
        height: 480,
        frames: FRAMES,
        frame_micros: MICROS,
        sample_rate: RATE,
        channels: 2,
        audio_frames,
        audio_track: 0,
    };
    std::fs::write(directory.join("0.json"), serde_json::to_vec_pretty(&asset)?)?;
    Ok(())
}

fn check_known_pixels(
    image: &Image,
    display: crate::Resolution,
    first: u32,
    last: u32,
) -> Result<u32> {
    ensure!(
        last >= first && last < 240,
        "invalid known movie readback interval"
    );
    let pixels = image.clone().try_into_dynamic()?.to_rgba8();
    let ui = display.ui_size();
    let scale = (pixels.width() as f32 / ui.x).min(pixels.height() as f32 / ui.y);
    let center = Vec2::new(pixels.width() as f32, pixels.height() as f32) * 0.5;
    let region = |position: Vec2, expected: [u8; 3]| {
        let point = center + position * scale;
        let x = point.x as i32;
        let y = point.y as i32;
        (-3..=3).all(|dy| {
            (-3..=3).all(|dx| {
                let (x, y) = (x + dx, y + dy);
                x >= 0
                    && y >= 0
                    && x < pixels.width() as i32
                    && y < pixels.height() as i32
                    && pixels.get_pixel(x as u32, y as u32).0[..3]
                        .iter()
                        .zip(expected)
                        .all(|(&actual, expected)| actual.abs_diff(expected) <= 3)
            })
        })
    };
    // Rendering is asynchronous. Any plateau selected between request and
    // callback is eligible, but a prior plateau cannot satisfy a later request.
    for epoch in first / 90..=last / 90 {
        if [
            Vec2::new(-160., -120.),
            Vec2::new(160., -120.),
            Vec2::new(-160., 120.),
            Vec2::new(160., 120.),
        ]
        .into_iter()
        .zip(COLORS[epoch as usize])
        .all(|(at, color)| region(at, color))
            && (ui.x <= 648.
                || [-1., 1.]
                    .into_iter()
                    .all(|side| region(Vec2::new(side * (320. + ui.x * 0.5) * 0.5, 0.), [0; 3])))
        {
            return Ok(epoch);
        }
    }
    anyhow::bail!("window regions differ from known movie frames {first}..={last}")
}

#[derive(Resource)]
struct Probe {
    output: PathBuf,
    stalls: bool,
    known_content: bool,
    began: Instant,
    first: Option<Instant>,
    completed: Option<Instant>,
    frame_interval: Duration,
    final_timestamp: Duration,
    audio_duration: Duration,
    updates: u64,
    injected_stalls: u64,
    stalled: bool,
    selected: u32,
    last: Option<Selection>,
    samples: [u64; 2],
    max_age: [Duration; 2],
    max_lag: [Duration; 2],
    invalid_selection: bool,
    dropped: u64,
    title_ready: bool,
    readbacks: Vec<Readback>,
    capture_pending: bool,
    last_capture_frame: Option<u32>,
    readback_error: Option<String>,
    finished: bool,
}
#[derive(serde::Serialize)]
struct Readback {
    path: String,
    selected_frame_at_request: Option<u32>,
    selected_frame_at_callback: Option<u32>,
    selected_timestamp_at_request: Option<Duration>,
    injected_stalls_at_request: u64,
    width: u32,
    height: u32,
    visible: bool,
    rgba_sha256: String,
    known_epoch: Option<u32>,
}
impl Readback {
    fn new(
        image: &Image,
        expected: UVec2,
        frame: Option<u32>,
        timestamp: Option<Duration>,
        stalls: u64,
        path: String,
    ) -> Result<Self> {
        ensure!(
            image.size() == expected,
            "movie readback dimensions changed"
        );
        let pixels = image.clone().try_into_dynamic()?.to_rgba8();
        let mut lit = 0;
        let mut low = [255u8; 3];
        let mut high = [0u8; 3];
        for pixel in pixels.pixels() {
            lit += usize::from(pixel.0[..3].iter().any(|&channel| channel > 16));
            for channel in 0..3 {
                low[channel] = low[channel].min(pixel.0[channel]);
                high[channel] = high[channel].max(pixel.0[channel]);
            }
        }
        // Initial black movie frames are valid, but cannot prove a working draw.
        let visible = lit as f64 > f64::from(expected.x) * f64::from(expected.y) * 0.01
            && (0..3).any(|channel| high[channel].saturating_sub(low[channel]) >= 24);
        Ok(Self {
            path,
            selected_frame_at_request: frame,
            selected_frame_at_callback: None,
            selected_timestamp_at_request: timestamp,
            injected_stalls_at_request: stalls,
            width: expected.x,
            height: expected.y,
            visible,
            known_epoch: None,
            rgba_sha256: format!("{:x}", Sha256::digest(pixels.as_raw())),
        })
    }
}
#[derive(serde::Serialize)]
struct Selection {
    frame: u32,
    timestamp: Duration,
    audible_position: Duration,
}
impl Selection {
    fn age(&self) -> Duration {
        self.audible_position.saturating_sub(self.timestamp)
    }
}
impl Probe {
    fn new(
        output: &Path,
        stalls: bool,
        frame_interval: Duration,
        final_timestamp: Duration,
        audio_duration: Duration,
    ) -> Self {
        Self {
            output: output.into(),
            stalls,
            known_content: false,
            began: Instant::now(),
            first: None,
            completed: None,
            frame_interval,
            final_timestamp,
            audio_duration,
            updates: 0,
            injected_stalls: 0,
            stalled: false,
            selected: 0,
            last: None,
            samples: [0; 2],
            max_age: [Duration::ZERO; 2],
            max_lag: [Duration::ZERO; 2],
            invalid_selection: false,
            dropped: 0,
            title_ready: false,
            readbacks: Vec::new(),
            capture_pending: false,
            last_capture_frame: None,
            readback_error: None,
            finished: false,
        }
    }
    fn sample(&mut self, frame: u32, timestamp: Duration, position: Duration) {
        self.first.get_or_insert_with(Instant::now);
        self.updates += 1;
        if self.last.as_ref().is_none_or(|last| last.frame != frame) {
            self.selected += 1;
        }
        self.invalid_selection |= self.last.as_ref().is_some_and(|last| {
            frame < last.frame || timestamp < last.timestamp || position < last.audible_position
        }) || timestamp > position
            || timestamp > self.final_timestamp;
        let sample = Selection {
            frame,
            timestamp,
            audible_position: position,
        };
        let group = usize::from(self.stalled);
        self.samples[group] += 1;
        self.max_age[group] = self.max_age[group].max(sample.age());
        self.max_lag[group] =
            self.max_lag[group].max(position.min(self.final_timestamp).saturating_sub(timestamp));
        self.last = Some(sample);
    }
    fn rendered_progress(&self) -> (usize, bool) {
        let mut previous: Option<&Readback> = None;
        let mut frames = 0;
        let mut after_stall = false;
        for readback in &self.readbacks {
            if readback.visible
                && readback.selected_frame_at_request.is_some()
                && (!self.known_content || readback.known_epoch.is_some())
                && previous.is_none_or(|previous| {
                    readback.selected_frame_at_request > previous.selected_frame_at_request
                })
            {
                after_stall |= frames > 0 && readback.injected_stalls_at_request > 0;
                frames += 1;
                previous = Some(readback);
            }
        }
        (frames, after_stall)
    }
    fn known_content_complete(&self) -> bool {
        !self.known_content
            || (self
                .readbacks
                .iter()
                .filter(|r| r.known_epoch == Some(0))
                .count()
                >= 2
                && (0..COLORS.len() as u32)
                    .all(|epoch| self.readbacks.iter().any(|r| r.known_epoch == Some(epoch))))
    }
    fn title_readback(&self) -> Option<&Readback> {
        self.readbacks
            .iter()
            .find(|readback| readback.selected_frame_at_request.is_none())
    }
    fn validate(&self) -> Result<()> {
        ensure!(self.selected >= 2, "movie did not present advancing frames");
        ensure!(self.completed.is_some(), "movie did not complete naturally");
        ensure!(
            self.title_ready,
            "movie did not return to a rendered, input-ready title owner"
        );
        ensure!(
            !self.stalls || self.injected_stalls > 0,
            "requested stalls were not exercised"
        );
        ensure!(
            !self.invalid_selection,
            "movie presentation regressed or selected an early/invalid frame"
        );
        ensure!(
            self.max_lag.iter().all(|lag| *lag <= self.frame_interval),
            "movie selection missed the newest due frame by more than one native frame interval"
        );
        // Completion is observed on render updates, so allow one authored video
        // interval at the boundary, just as the newest-due selection check does.
        // A completion flag cannot replace progress through either media track.
        ensure!(
            self.last.as_ref().is_some_and(|last| {
                self.final_timestamp.saturating_sub(last.timestamp) <= self.frame_interval
                    && self.audio_duration.saturating_sub(last.audible_position)
                        <= self.frame_interval
            }),
            "movie completed before the expected video/audio endpoint"
        );
        let (rendered, after_stall) = self.rendered_progress();
        ensure!(
            rendered >= 2,
            "movie GPU readbacks did not show advancing visible images"
        );
        ensure!(
            !self.stalls || after_stall,
            "movie GPU readback did not advance after an injected stall"
        );
        ensure!(
            self.known_content_complete(),
            "known movie did not render repeated frames and every distinct plateau, including the final one"
        );
        let title = self
            .title_readback()
            .context("title GPU readback did not complete")?;
        ensure!(
            title.visible
                && self
                    .readbacks
                    .iter()
                    .rev()
                    .find(|readback| readback.selected_frame_at_request.is_some())
                    .is_some_and(|movie| movie.rgba_sha256 != title.rgba_sha256),
            "title GPU readback is blank or still contains the last movie image"
        );
        ensure!(!self.capture_pending, "movie GPU readback is still pending");
        ensure!(
            self.readback_error.is_none(),
            "movie GPU readback failed: {:?}",
            self.readback_error
        );
        Ok(())
    }
    fn publish(&self, interruption: Option<&str>) -> Result<()> {
        let validation = self.validate();
        let failure = interruption
            .map(str::to_owned)
            .or_else(|| validation.err().map(|error| error.to_string()));
        let summary = |group: usize| {
            serde_json::json!({
                "samples":self.samples[group],
                "raw_age_max_ms":self.max_age[group].as_secs_f64() * 1000.,
                "newest_due_lag_max_ms":self.max_lag[group].as_secs_f64() * 1000.,
            })
        };
        let result = serde_json::json!({
            "accepted":failure.is_none(), "failure":failure,
            "known_content":self.known_content,
            "silent":true, "forced_stall_ms":if self.stalls {200}else{0},
            "injected_stalls":self.injected_stalls, "total_seconds":self.began.elapsed().as_secs_f64(),
            "playback_seconds":self.first.zip(self.completed).map(|(first,end)|end.duration_since(first).as_secs_f64()),
            "title_wait_seconds":self.completed.map(|end|end.elapsed().as_secs_f64()),
            "selection_observations":self.updates, "selected_cpu_frames":self.selected,
            "skipped_due_frames":self.dropped, "completed_naturally":self.completed.is_some(),
            "title_ready_for_input":self.title_ready,
            "freshness_bound_ms":self.frame_interval.as_secs_f64() * 1000.,
            "final_video_timestamp":self.final_timestamp,
            "expected_audio_duration":self.audio_duration,
            "ordinary_selections":summary(0), "after_forced_stall":summary(1),
            "last_selection":self.last,
            "window_readbacks":self.readbacks,
            "readback_timing":"asynchronous primary-window GPU images after output conversion; selected frame/timestamp identify the request, not the later callback; playback continues during readback",
            "timing":"audible position at every CPU frame-selection attempt, including retained frames; excludes physical display latency; newest-due target is capped at the final video timestamp",
        });
        std::fs::write(
            self.output.join("movie.json"),
            serde_json::to_vec_pretty(&result)?,
        )?;
        if let Some(failure) = failure {
            anyhow::bail!("{failure}");
        }
        Ok(())
    }
}
fn stall(mut probe: ResMut<Probe>, movie: Res<crate::movie::Playback>) {
    if probe.stalls
        && movie.is_presenting()
        && probe.rendered_progress().0 >= 2
        && probe.updates > 0
        && probe.updates.is_multiple_of(30)
    {
        std::thread::sleep(Duration::from_millis(200));
        probe.injected_stalls += 1;
        probe.stalled = true;
    }
}
fn observe(world: &mut World) {
    let movie = world.resource::<crate::movie::Playback>();
    let (active, completed, frame, timestamp, position, dropped) = (
        movie.active,
        movie.completed_naturally,
        movie.presented_frame,
        movie.presented_timestamp,
        movie.selection_position,
        movie.dropped_frames,
    );
    let camera_active = world
        .query_filtered::<&Camera, With<crate::movie::MovieCamera>>()
        .iter(world)
        .any(|camera| camera.is_active);
    let title_ready = world.contains_resource::<crate::TitleActive>()
        && !active
        && !world.resource::<crate::boot::Playback>().active()
        && !world.contains_resource::<crate::new_game::Request>()
        && !world.contains_resource::<crate::new_game::Session>()
        && !world.contains_resource::<crate::loading::Pending>()
        && !world.contains_resource::<crate::saves::title::LoadMenu>()
        && world.resource::<crate::timing::Ready>().0
        && world.resource::<crate::Menu>().0.accepts_input();
    let exiting = !world.resource::<Messages<AppExit>>().is_empty();
    let mut probe = world.resource_mut::<Probe>();
    if probe.finished {
        return;
    }
    // Playback retains its final selection when it retires its camera. Observe
    // that completion update too; it may select the final frame before finishing.
    if ((active && camera_active) || (completed && probe.completed.is_none()))
        && let (Some(frame), Some(timestamp), Some(position)) = (frame, timestamp, position)
    {
        probe.sample(frame, timestamp, position);
    }
    probe.stalled = false;
    probe.dropped = dropped;
    if completed {
        probe.completed.get_or_insert_with(Instant::now);
    }
    probe.title_ready = title_ready;
    let interruption = if let Some(error) = &probe.readback_error {
        Some(error.clone())
    } else if active && probe.first.is_some() && !camera_active {
        Some("movie camera is missing or inactive during presentation".to_owned())
    } else if exiting {
        Some("application exited before movie probe acceptance".to_owned())
    } else if probe.began.elapsed() > Duration::from_secs(240) {
        Some("movie probe timed out".to_owned())
    } else {
        None
    };
    let ready_to_finish = title_ready && probe.title_readback().is_some() && !probe.capture_pending;
    if interruption.is_some() || ready_to_finish {
        probe.finished = true;
        let recording_error = interruption
            .is_none()
            .then(|| {
                world
                    .resource_mut::<super::Monitor>()
                    .finish_recording()
                    .err()
                    .map(|error| error.to_string())
            })
            .flatten();
        let result = world
            .resource::<Probe>()
            .publish(interruption.as_deref().or(recording_error.as_deref()));
        if let Err(error) = &result {
            error!("Movie window probe rejected: {error:#}");
        }
        world.write_message(if result.is_ok() {
            AppExit::Success
        } else {
            AppExit::error()
        });
        return;
    }
    let (rendered, after_stall) = probe.rendered_progress();
    let needs_movie = rendered < 2
        || (probe.stalls && !after_stall && probe.injected_stalls > 0)
        || !probe.known_content_complete();
    let request_movie = active
        && camera_active
        && needs_movie
        && frame.is_some_and(|frame| {
            (!probe.known_content || (frame >= 6 && frame % 90 < 60))
                && probe
                    .last_capture_frame
                    .is_none_or(|last| frame >= last.saturating_add(30))
        })
        && timestamp
            .is_some_and(|timestamp| timestamp + Duration::from_secs(1) < probe.final_timestamp);
    let request_title = title_ready && probe.title_readback().is_none();
    if !probe.capture_pending && (request_movie || request_title) {
        let frame = request_movie.then_some(frame).flatten();
        let timestamp = request_movie.then_some(timestamp).flatten();
        probe.capture_pending = true;
        if frame.is_some() {
            probe.last_capture_frame = frame;
        }
        if let Err(error) = request_readback(world, frame, timestamp) {
            let mut probe = world.resource_mut::<Probe>();
            probe.capture_pending = false;
            probe.readback_error = Some(format!("movie window readback failed: {error:#}"));
        }
    }
}

fn request_readback(
    world: &mut World,
    frame: Option<u32>,
    timestamp: Option<Duration>,
) -> Result<()> {
    let display = world.resource::<crate::display::Display>().0;
    let (window, expected) = world
        .query_filtered::<(Entity, &Window), With<PrimaryWindow>>()
        .single(world)
        .map(|(entity, window)| (entity, window.physical_size()))?;
    let source = world.resource::<crate::display::Targets>().source.clone();
    let title_camera = frame
        .is_none()
        .then(|| {
            world
                .query_filtered::<Entity, With<crate::FieldCamera>>()
                .single(world)
                .ok()
        })
        .flatten();
    if let Some(entity) = title_camera {
        // This is the probe's terminal draw. Clear the retained source through
        // its actual title camera so an empty title cannot reuse movie pixels
        // when the final output pass renders into the captured window.
        if let Some(mut camera) = world.get_mut::<Camera>(entity) {
            camera.clear_color = ClearColorConfig::Custom(Color::BLACK);
        }
    }
    let probe = world.resource::<Probe>();
    let stalls = probe.injected_stalls;
    let name = frame.map_or_else(
        || "title.png".to_owned(),
        |_| format!("movie-{:03}.png", probe.readbacks.len()),
    );
    let path = probe.output.join(&name);
    // Include the output material and window camera: a healthy source texture
    // cannot prove that the player sees the movie or the returning title.
    world.spawn(Screenshot::primary_window()).observe(
        move |event: On<ScreenshotCaptured>,
              mut probe: ResMut<Probe>,
              movie: Res<crate::movie::Playback>,
              title: Option<Res<crate::TitleActive>>,
              menu: Res<crate::Menu>,
              targets: Res<crate::display::Targets>,
              cameras: Query<(&Camera, &RenderTarget), With<crate::movie::MovieCamera>>,
              output_cameras: Query<
            (&Camera, &RenderTarget),
            With<crate::display::OutputCamera>,
        >,
              title_cameras: Query<
            (&Camera, &Projection, &RenderTarget),
            With<crate::FieldCamera>,
        >| {
            let result = (|| -> Result<()> {
                ensure!(probe.capture_pending, "unexpected movie readback callback");
                ensure!(
                    targets.source == source,
                    "movie source changed before GPU readback"
                );
                ensure!(
                    output_cameras
                        .iter()
                        .any(|(camera, target)| camera.is_active
                            && matches!(target, RenderTarget::Window(reference) if match reference {
                                WindowRef::Primary => true,
                                WindowRef::Entity(entity) => *entity == window,
                            })),
                    "movie output camera no longer renders to the primary window"
                );
                let mut readback = Readback::new(
                    &event.image,
                    expected,
                    frame,
                    timestamp,
                    stalls,
                    name.clone(),
                )?;
                readback.selected_frame_at_callback = frame.and(movie.presented_frame);
                let content = if probe.known_content && let Some(first) = frame {
                    Some(movie.presented_frame.context("known movie lost its selected frame")
                        .and_then(|last| check_known_pixels(&event.image, display, first, last)))
                } else { None };
                readback.known_epoch = content.as_ref().and_then(|result| result.as_ref().ok().copied());
                crate::screenshot::write(
                    &event.image,
                    &path,
                    Some(&serde_json::to_value(&readback)?),
                )?;
                if let Some(content) = content { content?; }
                if frame.is_some() {
                    ensure!(movie.active && cameras.iter().any(|(camera, target)| camera.is_active
                        && matches!(target, RenderTarget::Image(image) if image.handle == source)),
                        "movie owner or source camera disappeared before GPU readback");
                } else {
                    ensure!(
                        cameras.iter().all(|(camera, _)| !camera.is_active),
                        "movie camera still renders over the title"
                    );
                    ensure!(
                        title_camera.is_some_and(|entity| title_cameras.get(entity).is_ok_and(
                            |(camera, projection, target)| {
                                super::window_probe::scene_camera(
                                    camera, projection, target, &source, display,
                                ) && matches!(camera.clear_color, ClearColorConfig::Custom(color) if color == Color::BLACK)
                            }
                        )),
                        "title camera no longer owns the readback source"
                    );
                    ensure!(
                        !movie.active
                            && title.is_some()
                            && probe.title_ready
                            && menu.0.accepts_input(),
                        "title owner lost input readiness before GPU readback"
                    );
                }
                probe.readbacks.push(readback);
                Ok(())
            })();
            probe.capture_pending = false;
            if let Err(error) = result {
                probe.readback_error = Some(format!("movie GPU readback failed: {error:#}"));
            }
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(color: u8) -> Image {
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        Image::new(
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![
                0, 0, 0, 255, color, 0, 0, 255, 0, color, 0, 255, 0, 0, color, 255,
            ],
            TextureFormat::Rgba8UnormSrgb,
            default(),
        )
    }
    fn known_pixels(epoch: usize) -> Image {
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let mut rgba = vec![0; 192 * 108 * 4];
        for y in 0..108 {
            for x in 0..192 {
                let at = (y * 192 + x) * 4;
                rgba[at + 3] = 255;
                if (24..168).contains(&x) {
                    rgba[at..at + 3].copy_from_slice(
                        &COLORS[epoch][usize::from(x >= 96) + 2 * usize::from(y >= 54)],
                    );
                }
            }
        }
        Image::new(
            Extent3d {
                width: 192,
                height: 108,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            rgba,
            TextureFormat::Rgba8UnormSrgb,
            default(),
        )
    }
    fn readback(color: u8, frame: Option<u32>, stalls: u64) -> Readback {
        Readback::new(
            &pixels(color),
            UVec2::splat(2),
            frame,
            None,
            stalls,
            "fixture.png".into(),
        )
        .unwrap()
    }
    fn completed_probe() -> Probe {
        let interval = Duration::from_micros(33_366);
        let mut probe = Probe::new(Path::new(""), false, interval, interval * 10, interval * 11);
        probe.sample(0, Duration::ZERO, Duration::ZERO);
        probe.sample(1, interval, interval);
        probe.sample(10, interval * 10, interval * 11);
        probe.completed = Some(Instant::now());
        probe.title_ready = true;
        probe.readbacks = vec![
            readback(64, Some(0), 0),
            readback(128, Some(1), 0),
            readback(255, None, 0),
        ];
        probe
    }

    #[test]
    fn readback_requires_primary_window_output_and_live_scene_ownership() -> Result<()> {
        let output = std::env::temp_dir().join(format!(
            "resonance-title-readback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir(&output)?;
        for case in [
            "valid",
            "movie_active",
            "title_missing",
            "title_inactive",
            "wrong_target",
            "title_replaced",
            "title_clear_removed",
            "title_blank",
            "output_missing",
            "output_inactive",
            "output_wrong_target",
            "movie_valid",
            "movie_output_inactive",
            "movie_blank",
            "movie_repeated",
            "known_valid",
            "known_repeated",
            "known_stale",
            "known_wrong_channels",
            "known_wrong_content",
            "known_crossed_plateau",
            "known_final",
            "known_late_frozen",
        ] {
            let mut world = World::new();
            let known = case.starts_with("known_");
            let movie = known || (case.starts_with("movie_") && case != "movie_active");
            let size = if known { (192, 108) } else { (2, 2) };
            let requested = if matches!(case, "known_repeated" | "known_crossed_plateau") {
                31
            } else if matches!(case, "known_final" | "known_late_frozen") {
                190
            } else if known {
                100
            } else {
                1
            };
            let source = Handle::<Image>::default();
            let mut probe = completed_probe();
            if movie {
                probe.readbacks.remove(1);
            } else {
                probe.readbacks.pop();
            }
            if known {
                probe.known_content = true;
                probe.readbacks[0] = Readback::new(
                    &known_pixels(0),
                    UVec2::new(192, 108),
                    Some(0),
                    None,
                    0,
                    "first.png".into(),
                )?;
                probe.readbacks[0].known_epoch = Some(0);
                if matches!(case, "known_final" | "known_late_frozen") {
                    for (frame, epoch) in [(30, 0), (100, 1)] {
                        let mut readback = Readback::new(
                            &known_pixels(epoch),
                            UVec2::new(192, 108),
                            Some(frame),
                            None,
                            0,
                            "earlier.png".into(),
                        )?;
                        readback.known_epoch = Some(epoch as u32);
                        probe.readbacks.push(readback);
                    }
                    assert!(
                        !probe.known_content_complete(),
                        "earlier plateaus cannot prove the movie's final content"
                    );
                }
            }
            probe.output = output.join(case);
            probe.capture_pending = true;
            world.insert_resource(probe);
            world.insert_resource(crate::display::Display(crate::Resolution {
                width: if known { 1920 } else { 2 },
                height: if known { 1080 } else { 2 },
            }));
            world.insert_resource(crate::display::Targets {
                source: source.clone(),
                output: None,
            });
            world.spawn((
                PrimaryWindow,
                Window {
                    resolution: size.into(),
                    ..default()
                },
            ));
            let output_camera = world
                .spawn((
                    crate::display::OutputCamera,
                    Camera::default(),
                    RenderTarget::default(),
                ))
                .id();
            let mut playback = crate::movie::Playback::default();
            playback.active = movie;
            playback.presented_frame = Some(if case == "known_crossed_plateau" {
                100
            } else {
                requested + 1
            });
            world.insert_resource(playback);
            world.insert_resource(crate::TitleActive);
            world.insert_resource(crate::Menu(resonance_game::TitleState {
                revealed: true,
                opacity: u8::MAX,
                ..Default::default()
            }));
            let movie_camera = world
                .spawn((
                    crate::movie::MovieCamera,
                    Camera {
                        is_active: movie,
                        ..default()
                    },
                    RenderTarget::Image(source.clone().into()),
                ))
                .id();
            let title_camera = world
                .spawn((
                    crate::FieldCamera,
                    Camera {
                        clear_color: ClearColorConfig::None,
                        ..default()
                    },
                    Projection::custom(crate::camera::TitleProjection(PerspectiveProjection {
                        aspect_ratio: 1.,
                        ..default()
                    })),
                    RenderTarget::Image(source.clone().into()),
                ))
                .id();
            // Register the production screenshot observer, then change ownership
            // before its real completion event rather than editing report metadata.
            request_readback(&mut world, movie.then_some(requested), None)?;
            world.flush();
            let request = world
                .query_filtered::<Entity, With<Screenshot>>()
                .single(&world)?;
            assert!(matches!(
                world.get::<Screenshot>(request).unwrap().0,
                RenderTarget::Window(WindowRef::Primary)
            ));
            if !movie {
                assert!(
                    matches!(world.get::<Camera>(title_camera).unwrap().clear_color,
                        ClearColorConfig::Custom(color) if color == Color::BLACK),
                    "the actual title pass must erase retained movie pixels"
                );
            }
            match case {
                "movie_active" => world.get_mut::<Camera>(movie_camera).unwrap().is_active = true,
                "title_missing" => {
                    world.despawn(title_camera);
                }
                "title_inactive" => {
                    world.get_mut::<Camera>(title_camera).unwrap().is_active = false
                }
                "title_clear_removed" => {
                    world.get_mut::<Camera>(title_camera).unwrap().clear_color =
                        ClearColorConfig::None
                }
                "wrong_target" => {
                    *world.get_mut::<RenderTarget>(title_camera).unwrap() = RenderTarget::default()
                }
                "title_replaced" => {
                    world.despawn(title_camera);
                    world.spawn((
                        crate::FieldCamera,
                        Camera::default(),
                        Projection::custom(crate::camera::TitleProjection(PerspectiveProjection {
                            aspect_ratio: 1.,
                            ..default()
                        })),
                        RenderTarget::Image(source.into()),
                    ));
                }
                "output_missing" => {
                    world.despawn(output_camera);
                }
                "output_inactive" | "movie_output_inactive" => {
                    world.get_mut::<Camera>(output_camera).unwrap().is_active = false
                }
                "output_wrong_target" => {
                    *world.get_mut::<RenderTarget>(output_camera).unwrap() =
                        RenderTarget::Image(source.clone().into())
                }
                _ => {}
            }
            let mut image = if known {
                known_pixels(if case == "known_final" {
                    2
                } else {
                    usize::from(!matches!(case, "known_repeated" | "known_stale"))
                })
            } else {
                pixels(match case {
                    "movie_blank" | "title_blank" => 0,
                    "movie_repeated" => 64,
                    _ if movie => 128,
                    _ => 255,
                })
            };
            if case == "known_wrong_channels" {
                for pixel in image.data.as_mut().unwrap().chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
            }
            if case == "known_wrong_content" {
                image.data.as_mut().unwrap().fill(127);
            }
            world.trigger(ScreenshotCaptured {
                entity: request,
                image,
            });
            let probe = world.resource::<Probe>();
            assert!(
                !probe.capture_pending,
                "observer did not complete for {case}"
            );
            if matches!(
                case,
                "known_valid" | "known_repeated" | "known_crossed_plateau" | "known_final"
            ) {
                assert!(probe.readback_error.is_none(), "{:?}", probe.readback_error);
                assert_eq!(
                    probe.rendered_progress().0,
                    if case == "known_final" { 4 } else { 2 }
                );
                assert_eq!(probe.known_content_complete(), case == "known_final");
                assert_eq!(
                    probe.readbacks.last().unwrap().known_epoch,
                    Some(if case == "known_final" {
                        2
                    } else {
                        u32::from(case != "known_repeated")
                    })
                );
            } else if case == "valid" || case == "movie_valid" || case == "movie_repeated" {
                assert!(probe.readback_error.is_none());
                probe.validate()?;
            } else {
                if !matches!(case, "movie_blank" | "title_blank") {
                    assert!(probe.readback_error.is_some(), "accepted {case}");
                }
                assert!(probe.validate().is_err(), "accepted {case}");
            }
        }
        std::fs::remove_dir_all(output)?;
        Ok(())
    }

    #[test]
    fn movie_readback_acceptance_allows_repeated_content_and_rejects_missing_or_stale_title_images()
    -> Result<()> {
        let mut probe = completed_probe();
        probe.readbacks.clear();
        assert!(
            probe.validate().is_err(),
            "CPU progress cannot substitute for GPU images"
        );
        probe.readbacks = vec![
            readback(0, Some(0), 0),
            readback(0, Some(1), 0),
            readback(255, None, 0),
        ];
        assert!(
            probe.validate().is_err(),
            "black screenshots cannot prove presentation"
        );
        probe.readbacks = vec![
            readback(64, Some(0), 0),
            readback(64, Some(1), 0),
            readback(255, None, 0),
        ];
        probe.validate()?; // A movie may intentionally repeat identical decoded frames.
        probe.readbacks[1] = readback(128, Some(1), 0);
        probe.readbacks[2] = readback(0, None, 0);
        assert!(
            probe.validate().is_err(),
            "input readiness does not prove a visible title"
        );
        probe.readbacks[2] = readback(128, None, 0);
        assert!(
            probe.validate().is_err(),
            "a retained movie image is not a rendered title"
        );
        probe.readbacks[2] = readback(255, None, 0);
        probe.validate()?;
        probe.stalls = true;
        probe.injected_stalls = 1;
        assert!(
            probe.validate().is_err(),
            "the post-stall readback must complete"
        );
        probe.readbacks.insert(2, readback(128, Some(7), 1));
        probe.validate()?; // Repeated decoded content remains valid after a stall.
        probe.readbacks[2] = readback(192, Some(7), 1);
        probe.validate()?;
        assert!(
            Readback::new(&pixels(255), UVec2::new(3, 2), None, None, 0, String::new()).is_err()
        );
        probe.capture_pending = true;
        assert!(probe.validate().is_err());
        probe.capture_pending = false;
        probe.readback_error = Some("failed image write".into());
        assert!(probe.validate().is_err());
        Ok(())
    }

    #[test]
    fn acceptance_publishes_metrics_before_rejecting_missing_or_stale_progress() -> Result<()> {
        let output = std::env::temp_dir().join(format!(
            "resonance-movie-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir(&output)?;
        let mut probe = completed_probe();
        probe.output = output.clone();
        probe.title_ready = false;
        assert!(probe.publish(None).is_err());
        probe.title_ready = true;
        probe.publish(None)?;
        probe.stalls = true;
        probe.injected_stalls = 1;
        probe.stalled = true;
        probe.final_timestamp *= 2;
        probe.sample(10, probe.frame_interval * 10, probe.frame_interval * 17);
        assert_eq!(
            probe.selected, 3,
            "retaining a frame is not another selection"
        );
        assert!(probe.publish(None).is_err());
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output.join("movie.json"))?)?;
        assert_eq!(report["accepted"], false);
        assert!(
            report["failure"]
                .as_str()
                .unwrap()
                .contains("newest due frame")
        );
        assert_eq!(report["after_forced_stall"]["samples"], 1);

        // Observe the real camera query: prebuffering is allowed, but losing the
        // camera after the first frame must publish a failure immediately.
        for missing in [false, true] {
            let mut world = World::new();
            let mut movie = crate::movie::Playback::default();
            movie.active = true;
            world.insert_resource(movie);
            world.insert_resource(Messages::<AppExit>::default());
            world.insert_resource(crate::display::Display::default());
            world.insert_resource(crate::display::Targets {
                source: Handle::default(),
                output: None,
            });
            world.spawn((PrimaryWindow, Window::default()));
            world.insert_resource(Probe::new(
                &output,
                false,
                probe.frame_interval,
                probe.final_timestamp + Duration::from_secs(2),
                probe.audio_duration + Duration::from_secs(2),
            ));
            observe(&mut world);
            assert!(!world.resource::<Probe>().finished);
            let camera = world
                .spawn((Camera::default(), crate::movie::MovieCamera))
                .id();
            let mut movie = world.resource_mut::<crate::movie::Playback>();
            movie.presented_frame = Some(0);
            movie.presented_timestamp = Some(Duration::ZERO);
            movie.selection_position = Some(Duration::ZERO);
            observe(&mut world);
            assert!(world.resource::<Probe>().capture_pending);
            assert_eq!(world.query::<&Screenshot>().iter(&world).count(), 1);
            if missing {
                world.despawn(camera);
            } else {
                world.get_mut::<Camera>(camera).unwrap().is_active = false;
            }
            observe(&mut world);
            assert!(world.resource::<Probe>().finished);
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(output.join("movie.json"))?)?;
            assert_eq!(report["accepted"], false);
            assert!(report["failure"].as_str().unwrap().contains("camera"));
        }
        std::fs::remove_dir_all(output)?;
        Ok(())
    }

    #[test]
    fn hitches_require_fresh_frames_but_terminal_frame_has_no_newer_target() -> Result<()> {
        let mut probe = completed_probe();
        probe.stalls = true;
        probe.injected_stalls = 1;
        probe.readbacks.insert(2, readback(192, Some(7), 1));
        probe.stalled = true;
        probe.final_timestamp = probe.frame_interval * 20;
        probe.audio_duration = probe.frame_interval * 21;
        probe.sample(
            17,
            probe.frame_interval * 17,
            probe.frame_interval * 17 + Duration::from_millis(2),
        );
        assert!(
            probe
                .validate()
                .unwrap_err()
                .to_string()
                .contains("endpoint")
        );
        // Exercise the completion update after the movie camera has retired.
        // The final selection must still reach the probe through observe().
        probe.completed = None;
        let mut world = World::new();
        let mut movie = crate::movie::Playback::default();
        movie.completed_naturally = true;
        movie.presented_frame = Some(20);
        movie.presented_timestamp = Some(probe.final_timestamp);
        movie.selection_position = Some(probe.final_timestamp + Duration::from_millis(200));
        world.insert_resource(movie);
        world.insert_resource(Messages::<AppExit>::default());
        world.insert_resource(probe);
        observe(&mut world);
        let mut probe = world.remove_resource::<Probe>().unwrap();
        assert!(probe.completed.is_some());
        assert_eq!(probe.last.as_ref().unwrap().frame, 20);
        // Title ownership/readback is covered by the callback regression above.
        probe.title_ready = true;
        probe.validate()?;
        assert_eq!(probe.max_lag[1], Duration::from_millis(2));
        assert_eq!(
            probe.last.as_ref().unwrap().age(),
            Duration::from_millis(200)
        );
        Ok(())
    }

    #[test]
    fn movie_acceptance_rejects_timing_write_and_final_flush_failures() -> Result<()> {
        let output = std::env::temp_dir().join(format!(
            "resonance-movie-recording-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir(&output)?;
        let path = output.join("frames.jsonl");
        std::fs::write(&path, b"existing timing evidence")?;
        for capacity in [1, 8192] {
            let mut app = App::new();
            super::super::install(&mut app, Default::default(), true)?;
            app.world_mut().resource_mut::<super::super::Monitor>().file = Some(
                std::io::BufWriter::with_capacity(capacity, std::fs::File::open(&path)?),
            );
            if capacity == 1 {
                app.update();
                app.update();
            }
            assert_eq!(
                app.world()
                    .resource::<super::super::Monitor>()
                    .recording_error
                    .is_some(),
                capacity == 1
            );
            let mut probe = completed_probe();
            probe.output = output.clone();
            app.insert_resource(probe)
                .insert_resource(crate::movie::Playback::default())
                .insert_resource(crate::boot::Playback::default())
                .insert_resource(crate::TitleActive)
                .insert_resource(crate::timing::Ready(true))
                .insert_resource(crate::Menu(resonance_game::TitleState {
                    revealed: true,
                    opacity: u8::MAX,
                    ..Default::default()
                }))
                .init_resource::<Messages<AppExit>>();
            observe(app.world_mut());
            assert_eq!(app.should_exit(), Some(AppExit::error()));
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(output.join("movie.json"))?)?;
            assert_eq!(report["accepted"], false);
            assert!(
                report["failure"]
                    .as_str()
                    .unwrap()
                    .contains("performance recording failed")
            );
            assert!(
                app.world_mut()
                    .resource_mut::<super::super::Monitor>()
                    .finish_recording()
                    .is_err()
            );
            assert_eq!(std::fs::read(&path)?, b"existing timing evidence");
        }
        std::fs::remove_dir_all(output)?;
        Ok(())
    }
}
