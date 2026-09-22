use super::audio_output::Sink as AudioSink;
use super::{PendingInput, RunOptions, materials::TitleText};
use crate::audio_output::{PlaybackSettings, Player as AudioPlayer};
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use resonance_content::{HEIGHT, MovieAsset, WIDTH};
use resonance_media::{MovieDecoder, MovieEvent, MovieStream, VideoFrame};
use resonance_playback::{ChannelCount, Decodable, SampleRate, Source};
use std::{
    collections::VecDeque,
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) mod pacing;
#[cfg(test)]
mod tests;

type AudioBuffer = resonance_playback::Pcm;

#[derive(Asset, TypePath, Clone)]
pub(super) struct MovieAudio {
    buffer: Arc<AudioBuffer>,
    rate: SampleRate,
    mono: bool,
}
pub(super) struct MovieSamples {
    source: resonance_playback::PcmSource,
    rate: SampleRate,
}
impl Iterator for MovieSamples {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.source.next()
    }
}

impl Source for MovieSamples {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).expect("stereo")
    }
    fn sample_rate(&self) -> SampleRate {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for MovieAudio {
    type Decoder = MovieSamples;
    fn decoder(&self) -> Self::Decoder {
        MovieSamples {
            source: self.buffer.source(self.mono),
            rate: self.rate,
        }
    }
}

#[derive(Resource, Default)]
pub(super) struct Playback {
    pub active: bool,
    pub completed_naturally: bool,
    pub resource: Option<u32>,
    pub asset: Option<MovieAsset>,
    pub presented_frame: Option<u32>,
    pub presented_timestamp: Option<Duration>,
    decoder: Option<MovieDecoder>,
    pending_events: VecDeque<MovieEvent>,
    stream: Option<MovieStream>,
    pub dropped_frames: u64,
    frames: VecDeque<VideoFrame>,
    buffer: Arc<AudioBuffer>,
    captured: Option<VideoFrame>,
    audio_entity: Option<Entity>,
    texture: Handle<Image>,
    ended: bool,
    paused: bool,
    started: Option<Instant>,
    prebuffer_started: Option<Instant>,
    completion: Option<resonance_events::Operation>,
    ignore_initial_skip: bool,
    pub(super) mono: bool,
}

/// Bounded decoder startup, performed by the field worker before activation.
/// The decoder blocks on its own bounded queue until the script plays it.
pub(super) struct Prepared {
    decoder: MovieDecoder,
    events: VecDeque<MovieEvent>,
}
impl Prepared {
    pub(super) fn load(
        root: &Path,
        asset: &MovieAsset,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let decoder = MovieDecoder::open(&root.join(&asset.path), asset.clone())?;
        let mut events = VecDeque::new();
        let (mut video, mut audio, mut chunks) = (0, 0, 0);
        let started = Instant::now();
        loop {
            ensure!(!cancelled(), "movie preparation cancelled");
            ensure!(
                started.elapsed() < Duration::from_secs(30),
                "movie preparation timed out"
            );
            let Some(event) = decoder.try_next()? else {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            };
            match &event {
                MovieEvent::Video(_) => video += 1,
                MovieEvent::Audio(chunk) => {
                    audio += chunk.samples.len();
                    chunks += 1;
                }
                MovieEvent::End => anyhow::bail!("movie ended during preparation"),
            }
            events.push_back(event);
            if video > 0 && audio >= asset.sample_rate as usize {
                break;
            }
            ensure!(
                video < 32 && chunks < 64,
                "movie exceeds startup buffer limits"
            );
        }
        Ok(Self { decoder, events })
    }
}

impl Playback {
    /// Reuse the movie surface for a field-script request. The decoder and
    /// audio queues belong to this playback; the token belongs to the scene.
    pub(super) fn start_script_movie(
        &mut self,
        resource: u32,
        prepared: Prepared,
        asset: MovieAsset,
        completion: resonance_events::Operation,
        images: &mut Assets<Image>,
    ) -> Result<()> {
        ensure!(!self.active, "another movie is still playing");
        asset.validate()?;
        ensure!(
            asset.width == WIDTH && asset.height == HEIGHT,
            "movie surface size differs"
        );
        let texture = self.texture.clone();
        // The preceding movie's final image must not flash while this one
        // prebuffers. Its surface remains black until the first decoded frame.
        images
            .get_mut(&texture)
            .context("movie surface is not initialized")?
            .data = Some(vec![0; WIDTH as usize * HEIGHT as usize * 4]);
        *self = Self {
            active: true,
            resource: Some(resource),
            asset: Some(asset),
            decoder: Some(prepared.decoder),
            pending_events: prepared.events,
            texture,
            completion: Some(completion),
            ignore_initial_skip: true,
            ..Default::default()
        };
        Ok(())
    }
    pub fn load(root: &Path, options: &RunOptions) -> Result<Self> {
        if options.skip_intro
            || options.tick.is_some()
            || options.replay.is_some()
            || options.boot_frame.is_some()
        {
            return Ok(Self::default());
        }
        let asset: MovieAsset =
            serde_json::from_slice(&fs::read(root.join("movies/0.json")).context(
                "missing opening movie; run resonance-import cook-all or use --skip-intro",
            )?)?;
        asset.validate()?;
        let path = root.join(&asset.path);
        let (decoder, captured) = if let Some(index) = options.movie_frame {
            (
                None,
                Some(MovieDecoder::frame(&path, asset.clone(), index)?),
            )
        } else {
            (Some(MovieDecoder::open(&path, asset.clone())?), None)
        };
        Ok(Self {
            active: true,
            resource: Some(0),
            asset: Some(asset),
            decoder,
            captured,
            ..Default::default()
        })
    }

    pub(super) fn is_presenting(&self) -> bool {
        self.active && self.started.is_some() && !self.paused
    }

    fn finish(&mut self, commands: &mut Commands, pending: &mut PendingInput) {
        self.active = false;
        self.buffer.finish();
        self.stream.take();
        self.decoder.take();
        self.frames.clear();
        if let Some(entity) = self.audio_entity.take() {
            commands.entity(entity).despawn();
        }
        if let Some(completion) = self.completion.take()
            && completion.is_pending()
        {
            // Skip and natural completion both release the script's movie wait.
            if let Err(error) = completion.complete(None) {
                error!("movie completion failed: {error}");
            }
        }
        // The skip press belongs to the movie, not the next menu update.
        let held = pending.held;
        *pending = PendingInput::default();
        pending.held = held;
    }
}

#[derive(Component)]
pub(super) struct MovieCamera;

pub(super) fn setup(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<TitleText>,
    movie: &mut Playback,
    source: Handle<Image>,
) {
    // Keep a movie surface available even when startup used --skip-intro.
    let (width, height) = movie
        .asset
        .as_ref()
        .map_or((WIDTH, HEIGHT), |a| (a.width, a.height));
    let bytes = movie.captured.take().map_or_else(
        || vec![0; width as usize * height as usize * 4],
        |frame| frame.rgba,
    );
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8Unorm,
        default(),
    );
    image.sampler = ImageSampler::linear();
    movie.texture = images.add(image);
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(width as f32, height as f32))),
        MeshMaterial2d(materials.add(TitleText {
            source: movie.texture.clone(),
            opacity_pulse: Vec4::new(1., 0., 0., 0.),
        })),
        RenderLayers::layer(3),
    ));
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        RenderLayers::layer(3),
        MovieCamera,
        Camera {
            order: -2,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        super::camera::overlay_alignment(),
        RenderTarget::Image(source.into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
}

#[allow(clippy::too_many_arguments)] // Movie decoding, input, texture upload, and playback ownership.
pub(super) fn update(
    mut commands: Commands,
    mut movie: ResMut<Playback>,
    options: Res<RunOptions>,
    input: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut pending: ResMut<PendingInput>,
    mut cameras: Query<&mut Camera, With<MovieCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut audio_assets: ResMut<Assets<MovieAudio>>,
    sinks: Query<&AudioSink>,
    mut exit: MessageWriter<AppExit>,
    recording: Option<Res<super::playthrough::Recording>>,
    ready: Res<super::timing::Ready>,
    boot: Res<super::boot::Playback>,
) {
    if !ready.0 || recording.is_some_and(|r| !r.started) {
        return;
    }
    for mut camera in &mut cameras {
        camera.is_active = movie.active && !boot.active();
    }
    if !movie.active || options.movie_frame.is_some() || boot.active() {
        return;
    }
    let skip_pressed = input.just_pressed(KeyCode::Enter)
        || input.just_pressed(KeyCode::Escape)
        || gamepads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::Start));
    let ignore_skip = std::mem::take(&mut movie.ignore_initial_skip);
    let skip = skip_pressed && !ignore_skip;
    if skip {
        info!("Movie skipped");
        movie.finish(&mut commands, &mut pending);
        for mut camera in &mut cameras {
            camera.is_active = false;
        }
        return;
    }
    if input.just_pressed(KeyCode::Space) {
        movie.paused = !movie.paused;
    }
    if let Err(error) = advance(
        &mut movie,
        &mut commands,
        &mut images,
        &mut audio_assets,
        &sinks,
    ) {
        error!("movie playback failed: {error:#}");
        if let Some(completion) = movie.completion.take() {
            completion.cancel();
        }
        movie.finish(&mut commands, &mut pending);
        exit.write(AppExit::error());
    } else if movie.ended
        && let Some(entity) = movie.audio_entity
        && let Ok(sink) = sinks.get(entity)
        && sink.empty()
    {
        info!("Movie playback complete");
        movie.completed_naturally = true;
        movie.finish(&mut commands, &mut pending);
        for mut camera in &mut cameras {
            camera.is_active = false;
        }
    }
}

fn advance(
    movie: &mut Playback,
    commands: &mut Commands,
    images: &mut Assets<Image>,
    audio_assets: &mut Assets<MovieAudio>,
    sinks: &Query<&AudioSink>,
) -> Result<()> {
    movie.prebuffer_started.get_or_insert_with(Instant::now);
    let asset = movie
        .asset
        .as_ref()
        .context("active movie has no manifest")?;
    if movie.stream.is_none() {
        let decoder = movie.decoder.take().context("movie decoder missing")?;
        let stream = MovieStream::start(
            decoder,
            std::mem::take(&mut movie.pending_events),
            asset.sample_rate,
        )?;
        movie.buffer = stream.audio();
        movie.stream = Some(stream);
    }
    let stream = movie.stream.as_ref().unwrap();
    stream.check()?;
    // Free stale presentation frames before draining the producer. Otherwise a
    // long stall with a full local queue would flash an old frame for one update.
    let mut latest = None;
    if let Some(position) = movie
        .audio_entity
        .and_then(|entity| sinks.get(entity).ok())
        .map(AudioSink::position)
    {
        while movie
            .frames
            .front()
            .is_some_and(|frame| frame.timestamp <= position)
        {
            if latest.is_some() {
                movie.dropped_frames += 1;
            }
            latest = movie.frames.pop_front();
        }
    }
    while movie.frames.len() < 32 {
        let Some(frame) = stream.try_video() else {
            break;
        };
        movie.frames.push_back(frame);
    }
    movie.ended = stream.complete();
    if movie.audio_entity.is_none() {
        let buffered = movie.buffer.buffered();
        if !movie.frames.is_empty() && (buffered >= u64::from(asset.sample_rate) / 2 || movie.ended)
        {
            let audio = MovieAudio {
                buffer: movie.buffer.clone(),
                mono: movie.mono,
                rate: SampleRate::new(asset.sample_rate).context("invalid movie sample rate")?,
            };
            movie.audio_entity = Some(
                commands
                    .spawn((
                        AudioPlayer(audio_assets.add(audio)),
                        PlaybackSettings {
                            paused: movie.paused,
                        },
                    ))
                    .id(),
            );
            movie.started = Some(Instant::now());
        } else {
            ensure!(
                movie
                    .prebuffer_started
                    .is_none_or(|loaded| loaded.elapsed() < Duration::from_secs(30)),
                "movie prebuffer timed out"
            );
        }
    }
    let Some(entity) = movie.audio_entity else {
        return Ok(());
    };
    let Ok(sink) = sinks.get(entity) else {
        ensure!(
            movie
                .started
                .is_none_or(|started| started.elapsed() < Duration::from_secs(10)),
            "movie audio output did not start"
        );
        return Ok(());
    };
    if movie.paused {
        sink.pause();
    } else {
        sink.play();
    }
    let position = sink.position();
    while movie
        .frames
        .front()
        .is_some_and(|frame| frame.timestamp <= position)
    {
        if latest.is_some() {
            movie.dropped_frames += 1;
        }
        latest = movie.frames.pop_front();
    }
    if let Some(frame) = latest {
        movie.presented_frame = Some(frame.index);
        movie.presented_timestamp = Some(frame.timestamp);
        images
            .get_mut(&movie.texture)
            .context("movie texture missing")?
            .data = Some(frame.rgba);
    }
    Ok(())
}

impl Playback {
    pub(super) fn wait_for_audio(&self, frames: u64) -> Result<()> {
        if self.is_presenting() {
            // The offline mixer pulls exactly these native stereo frames;
            // source attachment rejects other rates, so no resampler lookahead.
            self.stream
                .as_ref()
                .context("presenting movie has no decoder stream")?
                .wait_for_audio(frames, Duration::from_secs(30))?;
        }
        Ok(())
    }

    /// Authored subtitle cues retain their original frame numbers, but advance
    /// from media time even when the renderer is holding an older video frame.
    pub(super) fn timeline_frame(&self, position: Option<Duration>) -> Option<u32> {
        if !self.active {
            return None;
        }
        match (position, self.asset.as_ref()) {
            (Some(position), Some(asset)) => Some(
                (position.as_micros() / u128::from(asset.frame_micros))
                    .min(u128::from(asset.frames - 1)) as u32,
            ),
            _ => self.presented_frame,
        }
    }
    pub(super) fn audio_sink<'a>(&self, sinks: &'a Query<&AudioSink>) -> Option<&'a AudioSink> {
        self.audio_entity.and_then(|entity| sinks.get(entity).ok())
    }
    pub(super) fn position(&self, world: &World) -> Option<Duration> {
        self.audio_entity
            .and_then(|entity| world.get::<AudioSink>(entity))
            .map(AudioSink::position)
    }
}
